//! Spotify playback through Spotify's own Web Playback SDK, running in the operating system's web
//! view: WebView2 on Windows, WKWebView on macOS. The web view decrypts and plays with the
//! browser vendor's licensed DRM; Mosaic never sees the audio. See docs/spotify-cef-backend.md.
//!
//! Pieces:
//! - the host: `mosaic --spotify-host`, a hidden web view in its own process (app crate,
//!   `spotify_host.rs`), started on first use and stopped after `IDLE_SHUTDOWN`;
//! - the bridge (`bridge.rs`): a loopback server that serves the player page and its socket;
//! - the page (`player.html`): the SDK, reporting state and taking commands;
//! - the tracker (`tracker.rs`): start, end and errors of the track Mosaic asked for.
//!
//! Like Soloist, the engine gets a push source fed with silence paced by Spotify's position, so
//! the deck's clock, seeking and end-of-track handling work unchanged. EQ, leveling, crossfade and
//! the visualizer don't see Spotify's audio.

mod bridge;
pub mod tracker;

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{oneshot, watch};

use self::bridge::Wiring;
use self::tracker::{Action, PlayerState, Tracker};
use super::playback::{Target, NEEDS_PREMIUM};
use crate::Services;

pub const NOT_CONNECTED: &str = "Connect Spotify in the Spotify tab to play Spotify tracks";
pub const NOT_STREAMING: &str =
    "Reconnect Spotify in the Spotify tab: playback needs a permission your current sign-in is missing";
const NOT_STARTED: &str = "Spotify's player didn't start";
const STOPPED: &str = "Spotify's player stopped unexpectedly";
const TRACK_TIMEOUT: &str = "Spotify didn't start this track";

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Without a Spotify track loaded for this long, the host is stopped to free its memory
/// (WebView2 holds about 170 MB even idle). Starting it again takes about a second.
const IDLE_SHUTDOWN: Duration = Duration::from_secs(45);
/// How often the idle check runs.
const IDLE_CHECK: Duration = Duration::from_secs(10);
/// How often Spotify's up-next list is asked for while it's being shown or followed.
const QUEUE_REFRESH: Duration = Duration::from_secs(5);
/// Interleaved stereo f32 at the push source's 44.1 kHz: about 46 ms per chunk.
const CHUNK_FRAMES: usize = 2048;
const RATE: f64 = 44_100.0;

struct Shared {
    tracker: Mutex<Tracker>,
    wiring: Arc<Wiring>,
    services: Services,
    device: Mutex<Option<String>>,
    alive: AtomicBool,
    last_used: Mutex<Instant>,
    /// The startup outcome: the device id, or why the player couldn't start.
    ready: watch::Sender<Option<Result<String, String>>>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
    stop_bridge: Mutex<Option<oneshot::Sender<()>>>,
}

/// A running web player (host process + page). Cheap to clone.
#[derive(Clone)]
pub struct WebPlayer {
    shared: Arc<Shared>,
}

/// Turns a page error into what the player bar shows.
fn error_message(kind: &str, message: &str) -> String {
    match kind {
        "account" => NEEDS_PREMIUM.to_string(),
        "auth" => NOT_STREAMING.to_string(),
        "init" => format!("{NOT_STARTED}: {message}"),
        "autoplay" => "Spotify's player was blocked from starting playback".to_string(),
        _ => format!("Spotify couldn't play this track ({message})"),
    }
}

impl WebPlayer {
    fn with<R>(&self, f: impl FnOnce(&mut Tracker) -> R) -> Option<R> {
        self.shared.tracker.lock().ok().map(|mut t| f(&mut t))
    }

    fn send(&self, msg: Value) {
        if let Some(tx) = self
            .shared
            .wiring
            .outbox
            .lock()
            .ok()
            .and_then(|s| s.clone())
        {
            let _ = tx.send(msg.to_string());
        }
    }

    fn touch(&self) {
        if let Ok(mut t) = self.shared.last_used.lock() {
            *t = Instant::now();
        }
    }

    pub fn alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    pub fn describe(&self) -> String {
        if !self.alive() {
            return STOPPED.into();
        }
        let engine = if cfg!(target_os = "macos") {
            "Spotify plays through the system web player (WKWebView, experimental)"
        } else {
            "Spotify plays through the system web player (WebView2)"
        };
        engine.into()
    }

    /// Starts `uri`. While Spotify leads the queue (nothing of Mosaic's own follows), the track
    /// plays inside `context` so Spotify carries on from it by itself, and a track Spotify has
    /// already moved on to is taken over where it is instead of restarted.
    pub fn load(
        &self,
        uri: &str,
        context: Option<&str>,
        position_ms: u32,
        target: Target,
    ) -> Result<()> {
        if !self.alive() {
            bail!(STOPPED);
        }
        let device = self
            .shared
            .device
            .lock()
            .ok()
            .and_then(|d| d.clone())
            .ok_or_else(|| anyhow!("{NOT_STARTED}: no device yet"))?;
        let leads = self.shared.services.spotify_leads();
        let adopt = leads
            && position_ms < 2_000
            && self.with(|t| t.already_playing(uri)).unwrap_or(false);
        let gen = self
            .with(|t| {
                let gen = t.begin(uri, position_ms, target);
                if adopt {
                    if let Some(last) = t.last.clone() {
                        t.on_state(&last);
                    }
                }
                gen
            })
            .ok_or_else(|| anyhow!(STOPPED))?;
        self.touch();
        if adopt {
            tracing::debug!("spotify web: following Spotify onto {uri}");
            self.spawn_feeder(gen);
            return Ok(());
        }
        let context = if leads { context.map(str::to_string) } else { None };
        if let Some(v) = self.with(|t| t.volume).flatten() {
            self.send(json!({ "type": "volume", "value": v }));
        }
        self.send(json!({ "type": "activate" }));

        let me = self.clone();
        let uri = uri.to_string();
        tokio::spawn(async move {
            if let Err(e) = me
                .start_track(&device, &uri, context.as_deref(), position_ms)
                .await
            {
                me.fail_if_current(gen, e.to_string());
            }
        });
        self.spawn_feeder(gen);
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(LOAD_TIMEOUT).await;
            let stuck = me.with(|t| t.gen == gen && !t.started).unwrap_or(false);
            if stuck {
                me.fail_if_current(gen, TRACK_TIMEOUT.into());
            }
        });
        Ok(())
    }

    /// Asks Spotify to play this track on our device: alone when Mosaic's queue decides what's
    /// next, or inside `context` (from the track) when Spotify does. The device can take a
    /// moment to be known server-side after `ready`, hence the retries.
    async fn start_track(
        &self,
        device: &str,
        uri: &str,
        context: Option<&str>,
        position_ms: u32,
    ) -> Result<()> {
        let body = match context {
            Some(c) => json!({
                "context_uri": c,
                "offset": { "uri": uri },
                "position_ms": position_ms,
            }),
            None => json!({ "uris": [uri], "position_ms": position_ms }),
        };
        let mut last = None;
        for attempt in 0..4 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
            match self
                .shared
                .services
                .spotify_send(
                    reqwest::Method::PUT,
                    "/me/player/play",
                    &[("device_id", device.to_string())],
                    Some(body.clone()),
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let msg = e.to_string();
                    let retry = msg.to_lowercase().contains("device not found")
                        || msg.contains("404")
                        || msg.contains("502")
                        || msg.contains("503");
                    last = Some(e);
                    if !retry {
                        break;
                    }
                }
            }
        }
        let e = last.unwrap_or_else(|| anyhow!("unknown error"));
        let msg = e.to_string();
        if msg.to_lowercase().contains("premium") {
            bail!(NEEDS_PREMIUM);
        }
        if msg.contains("Reconnect Spotify") || msg.contains("403") {
            bail!(NOT_STREAMING);
        }
        Err(e)
    }

    fn fail_if_current(&self, gen: u64, message: String) {
        let target = self
            .with(|t| (t.gen == gen).then(|| t.clear()).flatten())
            .flatten();
        if let Some(target) = target {
            self.send(json!({ "type": "pause" }));
            (target.fail)(message);
        }
    }

    /// Feeds the deck silence at Spotify's pace on a plain thread, since writes block.
    fn spawn_feeder(&self, gen: u64) {
        let me = self.clone();
        std::thread::spawn(move || {
            let chunk_secs = CHUNK_FRAMES as f64 / RATE;
            enum Step {
                Stop,
                Wait,
                Write(Target),
            }
            loop {
                let step = me
                    .with(|t| {
                        if t.gen != gen {
                            Step::Stop
                        } else if t.may_feed(gen) {
                            t.fed += chunk_secs;
                            t.target.clone().map_or(Step::Stop, Step::Write)
                        } else {
                            Step::Wait
                        }
                    })
                    .unwrap_or(Step::Stop);
                match step {
                    Step::Stop => return,
                    Step::Wait => std::thread::sleep(Duration::from_millis(15)),
                    Step::Write(target) => {
                        // A refused write (deck paused or gone for 2 s) is retried until the
                        // generation moves on.
                        if !(target.write)(vec![0.0; CHUNK_FRAMES * 2]) {
                            me.with(|t| {
                                if t.gen == gen {
                                    t.fed -= chunk_secs;
                                }
                            });
                        }
                    }
                }
            }
        });
    }

    /// Spotify's up-next list and the track it's for, as last fetched; asks for a fresh one
    /// when it's stale. Empty until the first answer.
    pub fn queue(&self) -> (Option<String>, Vec<mosaic_core::model::Track>) {
        let stale = self
            .with(|t| {
                let stale = t.queue_asked.is_none_or(|at| at.elapsed() >= QUEUE_REFRESH);
                if stale {
                    t.queue_asked = Some(Instant::now());
                }
                stale
            })
            .unwrap_or(false);
        if stale {
            let me = self.clone();
            tokio::spawn(async move {
                match me.shared.services.spotify_api("/me/player/queue", &[]).await {
                    Ok(json) => {
                        let playing = json["currently_playing"]["uri"]
                            .as_str()
                            .map(str::to_string);
                        let tracks = super::web_api::usable(super::web_api::items(&json["queue"]));
                        me.with(|t| t.queue = (playing, tracks));
                    }
                    Err(e) => tracing::debug!("spotify web: no queue: {e}"),
                }
            });
        }
        self.with(|t| t.queue.clone()).unwrap_or_default()
    }

    /// Skips to Spotify's own next track (its queue or autoplay); the tracker then follows.
    pub fn skip_next(&self) {
        let Some(device) = self.shared.device.lock().ok().and_then(|d| d.clone()) else {
            return;
        };
        self.touch();
        let services = self.shared.services.clone();
        tokio::spawn(async move {
            if let Err(e) = services
                .spotify_send(
                    reqwest::Method::POST,
                    "/me/player/next",
                    &[("device_id", device)],
                    None,
                )
                .await
            {
                tracing::warn!("spotify web: skip failed: {e}");
            }
        });
    }

    pub fn pause(&self) {
        self.with(|t| t.set_paused_by_us(true));
        self.send(json!({ "type": "pause" }));
    }

    pub fn resume(&self) {
        self.with(|t| t.set_paused_by_us(false));
        self.touch();
        self.send(json!({ "type": "resume" }));
    }

    pub fn seek(&self, position_ms: u32) {
        self.with(|t| t.seek(position_ms));
        self.send(json!({ "type": "seek", "position_ms": position_ms }));
    }

    pub fn stop(&self) {
        self.with(|t| {
            t.clear();
            t.set_paused_by_us(true);
        });
        self.send(json!({ "type": "pause" }));
    }

    /// The web view plays to the system itself, so the deck's volume never reaches it: `v` is 0..1.
    pub fn set_volume(&self, v: f64) {
        let v = v.clamp(0.0, 1.0);
        let changed = self
            .with(|t| t.volume.replace(v) != Some(v))
            .unwrap_or(false);
        if changed {
            self.send(json!({ "type": "volume", "value": v }));
        }
    }

    /// Stops the host and the bridge. The next use starts a new player.
    pub fn shutdown(&self) {
        self.shared.alive.store(false, Ordering::SeqCst);
        if let Some(tx) = self.shared.kill.lock().ok().and_then(|mut k| k.take()) {
            let _ = tx.send(());
        }
        if let Some(tx) = self
            .shared
            .stop_bridge
            .lock()
            .ok()
            .and_then(|mut k| k.take())
        {
            let _ = tx.send(());
        }
    }

    fn handle(&self, msg: Value) {
        match msg["type"].as_str().unwrap_or_default() {
            "need_token" => {
                let me = self.clone();
                tokio::spawn(async move {
                    match me.shared.services.spotify_token().await {
                        Ok(token) => me.send(json!({ "type": "token", "token": token })),
                        Err(e) => {
                            tracing::warn!("spotify web: no token: {e}");
                            me.startup_failed(NOT_CONNECTED.into());
                        }
                    }
                });
            }
            "ready" => {
                let id = msg["device_id"].as_str().unwrap_or_default().to_string();
                if let Ok(mut d) = self.shared.device.lock() {
                    *d = Some(id.clone());
                }
                self.shared.ready.send_replace(Some(Ok(id)));
            }
            "not_ready" => {
                if let Ok(mut d) = self.shared.device.lock() {
                    *d = None;
                }
            }
            "state" => {
                let Ok(st) = serde_json::from_value::<PlayerState>(msg) else {
                    return;
                };
                tracing::debug!(
                    "spotify web: state {:?} paused={} loading={} at {}ms",
                    st.uri,
                    st.paused,
                    st.loading,
                    st.position_ms
                );
                if st.uri.is_some() {
                    self.touch();
                }
                match self.with(|t| t.on_state(&st)).unwrap_or(Action::None) {
                    Action::Finish => {
                        let moved = self.with(|t| t.moved_to()).flatten();
                        if let Some(target) = self.with(Tracker::clear).flatten() {
                            match moved {
                                // Spotify carried on with its own queue or autoplay: Mosaic
                                // follows it (and adopts the track where it is).
                                Some(to) if self.shared.services.spotify_leads() => {
                                    (target.moved)(to);
                                }
                                // Mosaic plays its own next track; hold Spotify until then.
                                _ => self.send(json!({ "type": "pause" })),
                            }
                            (target.finish)();
                        }
                    }
                    Action::Fail(m) => self.fail_current(m),
                    Action::None => {}
                }
            }
            "error" => {
                let kind = msg["kind"].as_str().unwrap_or("playback");
                let detail = msg["message"].as_str().unwrap_or_default();
                tracing::warn!("spotify web: {kind} error: {detail}");
                let message = error_message(kind, detail);
                let starting = self.shared.ready.borrow().is_none();
                if starting || matches!(kind, "account" | "init") {
                    self.startup_failed(message.clone());
                }
                if let Some(Action::Fail(m)) = self.with(|t| t.on_error(message)) {
                    self.fail_current(m);
                }
            }
            _ => {}
        }
    }

    fn fail_current(&self, message: String) {
        if let Some(target) = self.with(Tracker::clear).flatten() {
            (target.fail)(message);
        }
    }

    fn startup_failed(&self, message: String) {
        if self
            .shared
            .ready
            .borrow()
            .as_ref()
            .is_none_or(|r| r.is_ok())
        {
            self.shared.ready.send_replace(Some(Err(message)));
        }
    }

    fn died(&self, message: &str) {
        let was_alive = self.shared.alive.swap(false, Ordering::SeqCst);
        self.startup_failed(message.to_string());
        if was_alive {
            self.fail_current(message.to_string());
        }
    }
}

impl Services {
    /// Whether the stored Spotify token may stream and control playback (granted after the
    /// playback scopes were added; older sign-ins need a reconnect).
    pub fn spotify_can_stream(&self) -> bool {
        self.spotify_granted(&["streaming", "user-modify-playback-state"])
    }

    /// Starts the hidden web player and waits until Spotify's SDK reports its device.
    pub(crate) async fn start_web_player(&self) -> Result<WebPlayer> {
        if !self.spotify_status().await.connected {
            bail!(NOT_CONNECTED);
        }
        if !self.spotify_can_stream() {
            bail!(NOT_STREAMING);
        }
        let data_dir = self.inner.dirs.images().with_file_name("spotify-web");
        std::fs::create_dir_all(&data_dir)?;

        let (ready_tx, mut ready_rx) = watch::channel(None);
        let (kill_tx, kill_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let shared = Arc::new_cyclic(|weak: &Weak<Shared>| {
            let (on_msg, on_close) = (weak.clone(), weak.clone());
            Shared {
                tracker: Mutex::default(),
                wiring: Arc::new(Wiring {
                    on_message: Box::new(move |v| {
                        if let Some(shared) = on_msg.upgrade() {
                            WebPlayer { shared }.handle(v);
                        }
                    }),
                    on_disconnect: Box::new(move || {
                        if let Some(shared) = on_close.upgrade() {
                            WebPlayer { shared }.died(STOPPED);
                        }
                    }),
                    outbox: Mutex::new(None),
                }),
                services: self.clone(),
                device: Mutex::new(None),
                alive: AtomicBool::new(true),
                last_used: Mutex::new(Instant::now()),
                ready: ready_tx,
                kill: Mutex::new(Some(kill_tx)),
                stop_bridge: Mutex::new(Some(stop_tx)),
            }
        });
        let player = WebPlayer { shared };

        let secret = {
            use base64::Engine as _;
            use rand::RngExt as _;
            let mut bytes = [0u8; 32];
            rand::rng().fill(&mut bytes[..]);
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        };
        let bridge =
            bridge::start(secret.clone(), Arc::clone(&player.shared.wiring), stop_rx).await?;

        let exe = std::env::current_exe()?;
        let mut child = Command::new(exe)
            .arg("--spotify-host")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("{NOT_STARTED}: {e}"))?;
        // The page URL carries the secret, so it goes over stdin, never the command line.
        let config = json!({
            "url": bridge.page_url(&secret),
            "data_dir": data_dir.to_string_lossy(),
        });
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!(NOT_STARTED))?;
        stdin.write_all(format!("{config}\n").as_bytes()).await?;
        stdin.flush().await?;

        for (pipe, is_stdout) in [
            (
                child
                    .stdout
                    .take()
                    .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
                true,
            ),
            (
                child
                    .stderr
                    .take()
                    .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
                false,
            ),
        ] {
            let Some(pipe) = pipe else { continue };
            let me = player.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(pipe).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!("spotify host: {line}");
                    if !is_stdout {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if v["event"] == "error" {
                            let m = v["message"].as_str().unwrap_or("unknown error");
                            me.startup_failed(format!("{NOT_STARTED}: {m}"));
                        }
                    }
                }
            });
        }
        // The watcher owns the child and its stdin: closing stdin (or killing) ends the host.
        let watcher = player.clone();
        tokio::spawn(async move {
            let _stdin = stdin;
            let status = tokio::select! {
                s = child.wait() => s.ok(),
                _ = kill_rx => {
                    let _ = child.kill().await;
                    None
                }
            };
            if status.is_some() {
                watcher.died(STOPPED);
            }
        });

        let outcome = tokio::time::timeout(STARTUP_TIMEOUT, async {
            loop {
                if let Some(r) = ready_rx.borrow_and_update().clone() {
                    return r;
                }
                if ready_rx.changed().await.is_err() {
                    return Err(NOT_STARTED.to_string());
                }
            }
        })
        .await;
        match outcome {
            Ok(Ok(_device)) => {}
            Ok(Err(message)) => {
                player.shutdown();
                bail!(message);
            }
            Err(_) => {
                player.shutdown();
                bail!(if cfg!(windows) {
                    "Spotify's player didn't start in time. Is the Microsoft Edge WebView2 Runtime installed?"
                } else {
                    "Spotify's player didn't start in time"
                });
            }
        }

        // Free the host's memory when Spotify hasn't been used for a while.
        let idle = player.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(IDLE_CHECK).await;
                if !idle.alive() {
                    break;
                }
                let loaded = idle.with(|t| t.target.is_some()).unwrap_or(false);
                let quiet = idle
                    .shared
                    .last_used
                    .lock()
                    .map(|t| t.elapsed() >= IDLE_SHUTDOWN)
                    .unwrap_or(false);
                if !loaded && quiet {
                    tracing::info!("spotify web: idle, stopping the player");
                    idle.shutdown();
                    break;
                }
            }
        });
        Ok(player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_errors_become_player_bar_messages() {
        assert_eq!(error_message("account", "x"), NEEDS_PREMIUM);
        assert_eq!(error_message("auth", "x"), NOT_STREAMING);
        assert!(error_message("init", "no EME").contains("no EME"));
        assert!(error_message("playback", "boom").contains("boom"));
    }
}
