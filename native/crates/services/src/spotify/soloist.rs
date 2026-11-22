//! Spotify playback through Spotify Soloist, the official headless client, for accounts that
//! Spotify no longer gives librespot decryption keys (see `playback::KEY_REFUSED`).
//!
//! Soloist plays to the system audio server itself, so Mosaic only drives it: the daemon is
//! spawned with a local WebSocket, commands go out as JSON, and playback events come back. The
//! engine still gets a push source, fed with silence paced by Soloist's own position, so the
//! deck's clock, seeking and end-of-track handling work unchanged. EQ, leveling and the
//! visualizer don't see Soloist's audio.
//!
//! Soloist may not be redistributed, and each user needs their own API key, so both the binary
//! and the key come from the user (see `SoloistSettings`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::playback::Target;
use crate::Services;
use mosaic_core::model::Track;
use mosaic_core::storage::{read_json, write_json_atomic};

const SETTINGS_FILE: &str = "soloist.json";
pub const DEVICE_NAME: &str = "Mosaic";
pub const DOWNLOAD_URL: &str =
    "https://developer.spotify.com/documentation/soloist/reference/downloads-and-updates";
pub const NOT_PAIRED: &str = "Spotify Soloist isn't paired yet. Open Spotify on your phone or computer (same network), open the devices menu, pick \u{201c}Mosaic\u{201d}, then press play here again";
const EXPIRED: &str = "Your Spotify Soloist build has expired (they last 90 days). Download the latest one and restart Mosaic";
const STOPPED: &str = "Spotify Soloist stopped unexpectedly";

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the up-next list is asked for again, on top of Soloist's own announcements.
const QUEUE_REFRESH: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Interleaved stereo f32 at the push source's 44.1 kHz: about 46 ms per chunk.
const CHUNK_FRAMES: usize = 2048;
const RATE: f64 = 44_100.0;
/// How far the silent feed may run ahead of Soloist's position. Enough that ordinary jitter in
/// the feed never starves the deck (which would show the track as loading).
const LEAD_SECS: f64 = 0.6;

/// What the user configured. Stored apart from `config.json` because the key is a secret.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoloistSettings {
    /// Path to the `soloist` executable; empty means search `PATH` and `~/.local/bin`.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub api_key: String,
}

impl SoloistSettings {
    pub fn configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}

impl Services {
    pub fn soloist_settings(&self) -> SoloistSettings {
        read_json(&self.inner.dirs.file(SETTINGS_FILE))
    }

    pub fn soloist_save(&self, settings: &SoloistSettings) -> Result<()> {
        let path = self.inner.dirs.file(SETTINGS_FILE);
        let clean = SoloistSettings {
            path: settings.path.trim().to_string(),
            api_key: settings.api_key.trim().to_string(),
        };
        write_json_atomic(&path, &clean)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

/// The configured binary, or `soloist` on `PATH`, or the documented install locations.
pub fn find_binary(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let p = expand_home(configured);
        return is_executable(&p).then_some(p);
    }
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join("soloist"))
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(Path::new(&home).join(".local/bin/soloist"));
    }
    candidates.push(PathBuf::from("/usr/local/bin/soloist"));
    candidates.into_iter().find(|p| is_executable(p))
}

fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(p),
    }
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// What the event handler wants done after an event.
#[derive(Debug, PartialEq)]
enum Action {
    None,
    /// The loaded track started: apply the requested start position.
    Seek(u32),
    /// The loaded track ended (or Soloist moved on): finish the source and stop autoplay.
    Finish,
    /// Our track already finished and Soloist has since started another one by itself.
    Follow,
    Fail(String),
}

/// Soloist's playback position, anchored to when we heard it.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    secs: f64,
    at: Instant,
    speed: f64,
}

impl Anchor {
    /// Playback rate while playing. Soloist doesn't always report one (or reports 0), and a
    /// stalled clock stalls the feed, so playing always means moving.
    fn rate(&self) -> f64 {
        if self.speed > 0.0 {
            self.speed
        } else {
            1.0
        }
    }
}

/// The track URI an event is about, wherever this Soloist build puts it: the usual spots
/// first, then any track URI in the event outside its queue/next/previous lists.
fn event_uri(ev: &Value) -> Option<String> {
    fn scan(v: &Value) -> Option<String> {
        match v {
            Value::String(s) if s.starts_with("spotify:track:") => Some(s.clone()),
            Value::Object(map) => map
                .iter()
                .filter(|(k, _)| {
                    !["queue", "next", "prev", "upcoming", "context"]
                        .iter()
                        .any(|x| k.contains(x))
                })
                .find_map(|(_, v)| scan(v)),
            _ => None,
        }
    }
    [
        &ev["item"]["uri"],
        &ev["track"]["uri"],
        &ev["item"]["track"]["uri"],
        &ev["uri"],
    ]
    .into_iter()
    .find_map(|v| v.as_str().filter(|s| s.starts_with("spotify:")))
    .map(str::to_string)
    .or_else(|| scan(ev))
}

/// Upcoming track URIs from a `queue_changed` message, in order.
#[cfg(test)]
fn queue_uris(ev: &Value) -> Vec<String> {
    queue_tracks(ev).into_iter().filter_map(|t| t.uri).collect()
}

/// Upcoming tracks from a `queue_changed` message, in order, built from the metadata Soloist
/// sends along (title, artists, album, cover, length) so showing them needs no Web API calls.
/// A track without a name in it comes back with an empty title.
fn queue_tracks(ev: &Value) -> Vec<Track> {
    fn walk(v: &Value, out: &mut Vec<Track>) {
        match v {
            Value::Object(map) => {
                if let Some(uri) = map.get("uri").and_then(Value::as_str) {
                    if uri.starts_with("spotify:track:") {
                        out.push(track_from_item(v, uri));
                        return;
                    }
                }
                for v in map.values() {
                    walk(v, out);
                }
            }
            Value::Array(list) => list.iter().for_each(|v| walk(v, out)),
            Value::String(s) if s.starts_with("spotify:track:") => {
                out.push(track_from_item(&Value::Null, s));
            }
            _ => {}
        }
    }
    // Soloist 1.4 sends `upcoming` beside `previous` (what already played). Never fall back to
    // walking the whole event: that mixes the history into "up next".
    let Some(list) = ["upcoming", "next_tracks", "tracks", "queue", "items"]
        .into_iter()
        .find_map(|k| ev.get(k).filter(|v| v.is_array()))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(list, &mut out);
    out
}

/// A Spotify track from one of Soloist's entity objects (`uri` plus `decorations`).
fn track_from_item(item: &Value, uri: &str) -> Track {
    let deco = &item["decorations"];
    let name = |v: &Value| {
        v["identity"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    let artists: Vec<String> = deco["creators"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| name(&c["entity"]["decorations"]))
        .filter(|n| !n.is_empty())
        .collect();
    let cover = |sizes: &[&str]| {
        let list = deco["visual_identity"]["cover"].as_array();
        sizes
            .iter()
            .find_map(|size| {
                list.into_iter()
                    .flatten()
                    .find(|c| c["size"].as_str() == Some(size))
                    .and_then(|c| c["url"].as_str())
            })
            .unwrap_or_default()
            .to_string()
    };
    let id = uri.rsplit(':').next().unwrap_or_default();
    let v = json!({
        "id": format!("sp:{id}"),
        "source": "spotify",
        "uri": uri,
        "title": name(deco),
        "artist": if artists.is_empty() { "Unknown".to_string() } else { artists.join(", ") },
        "album": name(&deco["parent"]["entity"]["decorations"]),
        "artwork": cover(&["xlarge", "large", "default"]),
        "thumb": cover(&["default", "large", "small"]),
        "duration": deco["playback"]["duration_ms"].as_u64()
            .or_else(|| item["playback"]["duration_ms"].as_u64())
            .unwrap_or(0),
        "permalink": format!("https://open.spotify.com/track/{id}"),
    });
    serde_json::from_value(v).unwrap_or_else(|_| unreachable!("a valid track"))
}

/// `Target::moved`: tells Mosaic which track Soloist moved on to.
type MovedFn = Arc<dyn Fn(String) + Send + Sync>;

/// Playback state mirrored from Soloist's events, plus the track Mosaic asked for.
struct Tracker {
    logged_in: bool,
    is_active: bool,
    alive: bool,
    status: String,
    uri: Option<String>,
    anchor: Anchor,
    /// Bumped on every load/stop so stale feeders and timeouts bow out.
    gen: u64,
    want: Option<String>,
    target: Option<Target>,
    started: bool,
    start_ms: u32,
    /// Where the silent feed is, in track seconds.
    fed: f64,
    /// The volume Mosaic last set (0..=100).
    volume: Option<u8>,
    /// Soloist's own up-next list, from the last `queue_changed`.
    queue: Vec<Track>,
    /// When the up-next list was last asked for.
    queue_asked: Option<Instant>,
    /// After our track finished without Soloist naming a successor yet (it went idle or was
    /// still between tracks): the finished URI and whom to tell once another track plays.
    follow: Option<(String, MovedFn)>,
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker {
            logged_in: false,
            is_active: false,
            alive: true,
            status: "idle".into(),
            uri: None,
            anchor: Anchor {
                secs: 0.0,
                at: Instant::now(),
                speed: 0.0,
            },
            gen: 0,
            want: None,
            target: None,
            started: false,
            start_ms: 0,
            fed: 0.0,
            volume: None,
            queue: Vec::new(),
            queue_asked: None,
            follow: None,
        }
    }
}

/// The position in an event: under `position`, or at the top level.
fn event_position(ev: &Value) -> Option<Anchor> {
    read_position(&ev["position"]).or_else(|| read_position(ev))
}

fn read_position(v: &Value) -> Option<Anchor> {
    let ms = v.get("position_ms")?.as_f64()?;
    let speed = v.get("speed").and_then(Value::as_f64).unwrap_or(1.0);
    Some(Anchor {
        secs: ms / 1000.0,
        at: Instant::now(),
        speed,
    })
}

impl Tracker {
    fn playing(&self) -> bool {
        self.status == "playing"
    }

    /// Soloist's position now: running while it plays, held while it doesn't.
    fn position(&self) -> f64 {
        if self.playing() {
            self.anchor.secs + self.anchor.at.elapsed().as_secs_f64() * self.anchor.rate()
        } else {
            self.anchor.secs
        }
    }

    /// Changes the play status, keeping the position continuous across the change.
    fn set_status(&mut self, status: &str) {
        if status == self.status {
            return;
        }
        let secs = self.position();
        self.status = status.to_string();
        self.anchor = Anchor {
            secs,
            at: Instant::now(),
            speed: self.anchor.speed,
        };
    }

    /// Folds one server message into the state and says what to do about it.
    fn on_event(&mut self, ev: &Value) -> Action {
        let kind = ev.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "auth_state" => {
                self.logged_in = ev["logged_in"].as_bool().unwrap_or(false);
                self.is_active = ev["is_active"].as_bool().unwrap_or(false);
            }
            "device_changed" => self.is_active = ev["is_active"].as_bool().unwrap_or(false),
            "playback_state" => {
                // A full snapshot only arrives once logged in.
                self.logged_in = true;
                if let Some(s) = ev["status"].as_str() {
                    self.set_status(s);
                }
                if let Some(uri) = event_uri(ev) {
                    self.uri = Some(uri);
                }
                self.is_active = ev["is_active"].as_bool().unwrap_or(self.is_active);
                if let Some(a) = event_position(ev) {
                    self.anchor = a;
                }
            }
            "track_changed" => {
                if let Some(uri) = event_uri(ev) {
                    self.uri = Some(uri);
                }
            }
            "queue_changed" => {
                self.queue = queue_tracks(ev);
                return Action::None;
            }
            "playback_changed" => {
                if let Some(s) = ev["status"].as_str() {
                    self.set_status(s);
                }
            }
            "position_sync" => {
                if let Some(a) = event_position(ev) {
                    self.anchor = a;
                }
            }
            "error" => {
                let message = ev["message"].as_str().unwrap_or("unknown error");
                if self.target.is_some() && !self.started {
                    return Action::Fail(format!("Spotify Soloist: {message}"));
                }
                tracing::warn!("soloist: {message}");
                return Action::None;
            }
            _ => return Action::None,
        }
        self.check_track()
    }

    fn check_track(&mut self) -> Action {
        let Some(want) = self.want.clone() else {
            return match &self.follow {
                Some((from, _)) if self.playing() && self.successor_of(from).is_some() => {
                    Action::Follow
                }
                _ => Action::None,
            };
        };
        let ours = self.uri.as_deref() == Some(want.as_str());
        if !self.started {
            if ours && self.playing() {
                self.started = true;
                if self.start_ms > 0 {
                    return Action::Seek(self.start_ms);
                }
            }
            return Action::None;
        }
        if !ours || self.status == "idle" {
            return Action::Finish;
        }
        Action::None
    }

    /// Starts tracking a new load; returns its generation. A track Soloist is already playing
    /// (it moved on to it by itself) is adopted where it is instead of restarted.
    fn begin(&mut self, uri: &str, start_ms: u32, target: Target) -> u64 {
        self.gen += 1;
        self.follow = None;
        self.want = Some(uri.to_string());
        self.target = Some(target);
        let adopted = self.uri.as_deref() == Some(uri) && self.playing() && start_ms == 0;
        self.started = adopted;
        self.start_ms = start_ms;
        self.fed = if adopted {
            self.position()
        } else {
            f64::from(start_ms) / 1000.0
        };
        self.gen
    }

    /// The track Soloist is on, if it's a different one than `from`.
    fn successor_of(&self, from: &str) -> Option<String> {
        self.uri
            .clone()
            .filter(|u| u != from && u.starts_with("spotify:track:"))
    }

    /// Our track finished: hands back its target and the track Soloist moved on to, if it's
    /// already on one (whatever its status: it may still be buffering). Otherwise remembers to
    /// report the next track that plays.
    fn finish(&mut self) -> Option<(Target, Option<String>)> {
        let from = self.want.clone()?;
        let target = self.clear()?;
        let moved = (self.status != "idle")
            .then(|| self.successor_of(&from))
            .flatten();
        if moved.is_none() {
            self.follow = Some((from, Arc::clone(&target.moved)));
        }
        Some((target, moved))
    }

    fn clear(&mut self) -> Option<Target> {
        self.gen += 1;
        self.want = None;
        self.started = false;
        self.target.take()
    }

    /// Whether the silent feed may advance: our track is playing and the feed isn't ahead.
    fn may_feed(&self, gen: u64) -> bool {
        gen == self.gen
            && self.started
            && self.playing()
            && self.uri == self.want
            && self.fed < self.position() + LEAD_SECS
    }
}

struct Shared {
    tracker: Mutex<Tracker>,
    commands: mpsc::UnboundedSender<Value>,
}

/// A running Soloist daemon. Dropping the last clone doesn't stop it; the process task owns the
/// child and kills it when the runtime shuts down.
#[derive(Clone)]
pub struct Soloist {
    shared: Arc<Shared>,
    kill: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

fn command(name: &str) -> Value {
    json!({ "type": "command", "command": name })
}

impl Soloist {
    fn with<R>(&self, f: impl FnOnce(&mut Tracker) -> R) -> Option<R> {
        self.shared.tracker.lock().ok().map(|mut t| f(&mut t))
    }

    fn send(&self, v: Value) {
        let _ = self.shared.commands.send(v);
    }

    pub fn alive(&self) -> bool {
        self.with(|t| t.alive).unwrap_or(false)
    }

    pub fn paired(&self) -> bool {
        self.with(|t| t.logged_in).unwrap_or(false)
    }

    pub fn load(&self, uri: &str, position_ms: u32, target: Target) -> Result<()> {
        let (alive, paired, active) = self
            .with(|t| (t.alive, t.logged_in, t.is_active))
            .ok_or_else(|| anyhow!(STOPPED))?;
        if !alive {
            bail!(STOPPED);
        }
        if !paired {
            bail!(NOT_PAIRED);
        }
        let (gen, adopted) = self
            .with(|t| {
                let gen = t.begin(uri, position_ms, target);
                (gen, t.started)
            })
            .ok_or_else(|| anyhow!(STOPPED))?;
        if !active {
            self.send(command("activate"));
        }
        if let Some(v) = self.with(|t| t.volume).flatten() {
            self.send_volume(v);
        }
        if !adopted {
            let mut play = command("play");
            play["uri"] = json!(uri);
            self.send(play);
        }
        self.spawn_feeder(gen);
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(LOAD_TIMEOUT).await;
            let stuck = me
                .with(|t| (t.gen == gen && !t.started).then(|| t.clear()).flatten())
                .flatten();
            if let Some(target) = stuck {
                (target.fail)("Spotify Soloist didn't start this track".into());
            }
        });
        Ok(())
    }

    /// Feeds the deck silence at Soloist's pace on a plain thread, since writes block.
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

    pub fn pause(&self) {
        self.send(command("pause"));
    }

    /// Soloist plays to the system itself, so the deck's volume never reaches it: `v` is 0..1.
    pub fn set_volume(&self, v: f64) {
        let v = (v.clamp(0.0, 1.0) * 100.0).round() as u8;
        let changed = self
            .with(|t| t.volume.replace(v) != Some(v))
            .unwrap_or(false);
        if changed {
            self.send_volume(v);
        }
    }

    fn send_volume(&self, v: u8) {
        let mut cmd = command("set_volume");
        cmd["volume"] = json!(v);
        self.send(cmd);
    }

    /// Asks for Soloist's up-next list; it arrives as `queue_changed` (see `queue`).
    pub fn request_queue(&self) {
        self.with(|t| t.queue_asked = Some(Instant::now()));
        let mut cmd = command("get_queue");
        cmd["limit"] = json!(30);
        self.send(cmd);
    }

    /// Soloist's up-next list as last reported, and the track it's on. Soloist announces changes
    /// by itself; this also asks again every few seconds in case one was missed.
    pub fn queue(&self) -> (Option<String>, Vec<Track>) {
        let stale = self
            .with(|t| t.queue_asked.is_none_or(|at| at.elapsed() >= QUEUE_REFRESH))
            .unwrap_or(false);
        if stale {
            self.request_queue();
        }
        self.with(|t| (t.uri.clone(), t.queue.clone()))
            .unwrap_or_default()
    }

    /// Skips to Soloist's own next track (its queue or autoplay).
    pub fn skip_next(&self) {
        self.send(command("skip_next"));
    }

    pub fn resume(&self) {
        self.send(command("play"));
    }

    pub fn seek(&self, position_ms: u32) {
        self.with(|t| {
            t.fed = f64::from(position_ms) / 1000.0;
            t.anchor = Anchor {
                secs: t.fed,
                at: Instant::now(),
                speed: t.anchor.speed,
            };
        });
        let mut seek = command("seek");
        seek["position_ms"] = json!(position_ms);
        self.send(seek);
    }

    pub fn stop(&self) {
        self.with(|t| {
            t.follow = None;
            t.clear()
        });
        self.pause();
    }

    pub fn shutdown(&self) {
        if let Some(tx) = self.kill.lock().ok().and_then(|mut k| k.take()) {
            let _ = tx.send(());
        }
    }

    fn handle(&self, ev: &Value) {
        let Some(action) = self.with(|t| t.on_event(ev)) else {
            return;
        };
        match action {
            Action::None => {}
            Action::Seek(ms) => {
                let mut seek = command("seek");
                seek["position_ms"] = json!(ms);
                self.send(seek);
            }
            Action::Follow => {
                let follow = self
                    .with(|t| {
                        let (from, moved) = t.follow.take()?;
                        t.successor_of(&from).map(|uri| (uri, moved))
                    })
                    .flatten();
                if let Some((uri, moved)) = follow {
                    moved(uri);
                }
            }
            Action::Finish => {
                let done = self.with(Tracker::finish).flatten();
                if let Some((target, moved)) = done {
                    // Soloist carried on by itself (autoplay, or someone skipped from another
                    // device). It keeps playing; Mosaic either adopts that track or loads the
                    // next one from its own queue, which replaces it.
                    if let Some(uri) = moved {
                        (target.moved)(uri);
                    }
                    (target.finish)();
                }
            }
            Action::Fail(message) => {
                if let Some(target) = self.with(|t| t.clear()).flatten() {
                    (target.fail)(message);
                }
            }
        }
    }

    fn died(&self, message: &str) {
        let target = self
            .with(|t| {
                t.alive = false;
                t.clear()
            })
            .flatten();
        if let Some(target) = target {
            (target.fail)(message.to_string());
        }
    }
}

/// Ends any Soloist left behind by a previous run (it refuses to share a data dir).
fn kill_stale(data_dir: &Path) {
    let Ok(pid) = std::fs::read_to_string(data_dir.join("soloist.pid")) else {
        return;
    };
    let pid = pid.trim();
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    if comm.trim() == "soloist" {
        tracing::info!("soloist: stopping a leftover daemon (pid {pid})");
        let _ = std::process::Command::new("kill").arg(pid).status();
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Saves the key into Soloist's data dir through `setup-api-key` (stdin), so the daemon reads it
/// from there and it never shows up in its command line for other local users to see.
async fn store_api_key(binary: &Path, data_dir: &Path, key: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut child = Command::new(binary)
        .arg("setup-api-key")
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow!("Could not start Spotify Soloist: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(format!("{key}\n").as_bytes()).await?;
    }
    let out = tokio::time::timeout(STARTUP_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| anyhow!("Spotify Soloist didn't save the API key in time"))??;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        let why = why.trim().trim_start_matches("Error:").trim();
        bail!(
            "Spotify Soloist rejected the API key{}",
            if why.is_empty() {
                String::new()
            } else {
                format!(": {why}")
            }
        );
    }
    Ok(())
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

impl Services {
    /// Spawns Soloist and connects to its WebSocket. Succeeds even when it isn't paired yet, so
    /// the daemon stays up for the user to pick it in a Spotify app.
    pub(crate) async fn start_soloist(&self, settings: &SoloistSettings) -> Result<Soloist> {
        let binary = find_binary(&settings.path).ok_or_else(|| {
            if settings.path.trim().is_empty() {
                anyhow!("Spotify Soloist isn't installed. Download it from Spotify and put `soloist` on your PATH or set its path in Settings")
            } else {
                anyhow!("No executable Spotify Soloist at {}", settings.path.trim())
            }
        })?;
        let data_dir = self.inner.dirs.file("soloist");
        let cache_dir = self.inner.dirs.images().with_file_name("soloist");
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&cache_dir)?;
        let dir = data_dir.clone();
        tokio::task::spawn_blocking(move || kill_stale(&dir)).await?;
        let port_file = data_dir.join("ws.port");
        let addr_file = data_dir.join("ws.addr");
        let _ = std::fs::remove_file(&port_file);

        store_api_key(&binary, &data_dir, settings.api_key.trim()).await?;

        let mut child = Command::new(&binary)
            .arg("--device-name")
            .arg(DEVICE_NAME)
            .arg("--data-dir")
            .arg(&data_dir)
            .arg("--cache-dir")
            .arg(&cache_dir)
            .arg("--cache-size")
            .arg("1024")
            .arg("--ws")
            .arg("127.0.0.1:0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("Could not start Spotify Soloist: {e}"))?;
        let last_line: Arc<Mutex<String>> = Arc::default();
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        ]
        .into_iter()
        .flatten()
        {
            let last = Arc::clone(&last_line);
            tokio::spawn(async move {
                let mut lines = BufReader::new(pipe).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!("soloist: {line}");
                    if let Ok(mut l) = last.lock() {
                        *l = line;
                    }
                }
            });
        }

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let port = loop {
            if let Some(status) = child.try_wait()? {
                let last = last_line.lock().map(|l| l.clone()).unwrap_or_default();
                return Err(match status.code() {
                    Some(10) => anyhow!(EXPIRED),
                    code => anyhow!(
                        "Spotify Soloist exited ({}){}",
                        code.map_or("signal".into(), |c| format!("code {c}")),
                        if last.is_empty() {
                            String::new()
                        } else {
                            format!(": {last}")
                        }
                    ),
                });
            }
            if let Some(port) = read_trimmed(&port_file).and_then(|p| p.parse::<u16>().ok()) {
                break port;
            }
            if Instant::now() > deadline {
                bail!("Spotify Soloist started but its control socket never came up");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let host = read_trimmed(&addr_file).unwrap_or_else(|| "127.0.0.1".into());
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{host}:{port}"))
            .await
            .map_err(|e| anyhow!("Could not connect to Spotify Soloist: {e}"))?;
        let (mut sink, mut stream) = ws.split();

        let (commands, mut outbox) = mpsc::unbounded_channel::<Value>();
        let (kill_tx, kill_rx) = tokio::sync::oneshot::channel();
        let soloist = Soloist {
            shared: Arc::new(Shared {
                tracker: Mutex::default(),
                commands,
            }),
            kill: Arc::new(Mutex::new(Some(kill_tx))),
        };

        tokio::spawn(async move {
            while let Some(v) = outbox.recv().await {
                if sink.send(WsMessage::text(v.to_string())).await.is_err() {
                    break;
                }
            }
        });
        let reader = soloist.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Ok(text) = msg.to_text() else { continue };
                if let Ok(ev) = serde_json::from_str::<Value>(text) {
                    reader.handle(&ev);
                }
            }
            reader.died(STOPPED);
        });
        // Events can be missed (or arrive in a shape this build doesn't know); a cheap state
        // poll while Mosaic is playing through Soloist keeps the tracker honest.
        let poller = soloist.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                match poller.with(|t| (t.alive, t.target.is_some())) {
                    Some((true, true)) => poller.send(command("get_state")),
                    Some((true, false)) => {}
                    _ => break,
                }
            }
        });
        let watcher = soloist.clone();
        tokio::spawn(async move {
            let status = tokio::select! {
                s = child.wait() => s.ok(),
                _ = kill_rx => {
                    let _ = child.kill().await;
                    None
                }
            };
            watcher.died(match status.and_then(|s| s.code()) {
                Some(10) => EXPIRED,
                _ => STOPPED,
            });
        });

        // The server greets with auth_state; give it a moment so `paired()` is meaningful.
        for _ in 0..30 {
            if soloist.paired() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(soloist)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn target(finished: Arc<AtomicUsize>) -> Target {
        Target {
            write: Arc::new(|_| true),
            finish: Arc::new(move || {
                finished.fetch_add(1, Ordering::SeqCst);
            }),
            fail: Arc::new(|_| {}),
            moved: Arc::new(|_| {}),
        }
    }

    fn playing(uri: &str) -> Value {
        json!({ "type": "playback_state", "status": "playing", "item": { "uri": uri },
                "position": { "position_ms": 0, "timestamp_ms": 0, "speed": 1.0 } })
    }

    #[test]
    fn auth_state_drives_pairing() {
        let mut t = Tracker::default();
        t.on_event(&json!({ "type": "auth_state", "logged_in": false, "is_active": false }));
        assert!(!t.logged_in);
        t.on_event(&json!({ "type": "auth_state", "logged_in": true, "is_active": true }));
        assert!(t.logged_in && t.is_active);
    }

    #[test]
    fn a_track_starts_then_finishes_when_soloist_moves_on() {
        let mut t = Tracker::default();
        t.begin("spotify:track:a", 0, target(Arc::default()));
        assert_eq!(t.on_event(&playing("spotify:track:old")), Action::None);
        assert!(!t.started, "the previous track playing doesn't count");
        assert_eq!(t.on_event(&playing("spotify:track:a")), Action::None);
        assert!(t.started);
        let next = json!({ "type": "track_changed", "item": { "uri": "spotify:track:autoplay" } });
        assert_eq!(t.on_event(&next), Action::Finish);
    }

    #[test]
    fn a_move_is_reported_even_while_the_next_track_is_still_buffering() {
        let mut t = Tracker::default();
        t.begin("spotify:track:a", 0, target(Arc::default()));
        t.on_event(&playing("spotify:track:a"));
        let next = json!({ "type": "playback_state", "status": "buffering",
                           "item": { "uri": "spotify:track:b" } });
        assert_eq!(t.on_event(&next), Action::Finish);
        let (_, moved) = t.finish().unwrap();
        assert_eq!(moved.as_deref(), Some("spotify:track:b"));
        assert!(t.follow.is_none());
    }

    #[test]
    fn after_going_idle_the_next_track_soloist_plays_is_followed_once() {
        let mut t = Tracker::default();
        t.begin("spotify:track:a", 0, target(Arc::default()));
        t.on_event(&playing("spotify:track:a"));
        let idle = json!({ "type": "playback_changed", "status": "idle" });
        assert_eq!(t.on_event(&idle), Action::Finish);
        let (_, moved) = t.finish().unwrap();
        assert_eq!(moved, None, "nothing to follow yet");
        assert_eq!(t.on_event(&playing("spotify:track:a")), Action::None);
        assert_eq!(t.on_event(&playing("spotify:track:b")), Action::Follow);
        t.follow = None;
        assert_eq!(t.on_event(&playing("spotify:track:c")), Action::None);
        // A new load from Mosaic drops anything left to follow.
        t.follow = Some(("spotify:track:c".into(), Arc::new(|_| {})));
        t.begin("spotify:track:d", 0, target(Arc::default()));
        assert!(t.follow.is_none());
    }

    #[test]
    fn going_idle_finishes_and_a_start_position_is_applied_once_playing() {
        let mut t = Tracker::default();
        t.begin("spotify:track:a", 30_000, target(Arc::default()));
        assert_eq!(
            t.on_event(&playing("spotify:track:a")),
            Action::Seek(30_000)
        );
        let idle = json!({ "type": "playback_changed", "status": "idle" });
        assert_eq!(t.on_event(&idle), Action::Finish);
    }

    #[test]
    fn pausing_does_not_finish_and_stops_the_feed() {
        let mut t = Tracker::default();
        let gen = t.begin("spotify:track:a", 0, target(Arc::default()));
        t.on_event(&playing("spotify:track:a"));
        assert!(t.may_feed(gen));
        let paused = json!({ "type": "playback_changed", "status": "paused" });
        assert_eq!(t.on_event(&paused), Action::None);
        assert!(!t.may_feed(gen));
    }

    #[test]
    fn the_feed_keeps_going_when_soloist_reports_no_speed_or_resumes() {
        let mut t = Tracker::default();
        let gen = t.begin("spotify:track:a", 0, target(Arc::default()));
        t.on_event(&json!({ "type": "playback_state", "status": "playing",
                            "item": { "uri": "spotify:track:a" } }));
        assert!(t.started);
        t.anchor.at -= Duration::from_secs(2);
        t.fed = 1.0;
        assert!(
            t.may_feed(gen),
            "the clock runs while playing even with no speed reported"
        );

        t.on_event(&json!({ "type": "playback_changed", "status": "paused" }));
        let held = t.position();
        assert!(held >= 2.0 && !t.may_feed(gen));
        t.fed = held;
        t.on_event(&json!({ "type": "playback_changed", "status": "playing" }));
        t.anchor.at -= Duration::from_secs(1);
        assert!(t.may_feed(gen), "resuming starts the clock again");
        assert!(t.position() > held + 0.9);
    }

    #[test]
    fn the_feed_never_runs_far_ahead_of_soloist() {
        let mut t = Tracker::default();
        let gen = t.begin("spotify:track:a", 0, target(Arc::default()));
        t.on_event(&playing("spotify:track:a"));
        t.fed = LEAD_SECS + 0.5;
        assert!(!t.may_feed(gen));
        t.fed = 0.0;
        assert!(t.may_feed(gen));
        assert!(!t.may_feed(gen + 1), "a stale feeder stops");
    }

    #[test]
    fn errors_fail_only_a_track_that_has_not_started() {
        let mut t = Tracker::default();
        t.begin("spotify:track:a", 0, target(Arc::default()));
        let err = json!({ "type": "error", "message": "command requires authentication" });
        assert!(matches!(t.on_event(&err), Action::Fail(m) if m.contains("authentication")));
        t.on_event(&playing("spotify:track:a"));
        assert_eq!(t.on_event(&err), Action::None);
    }

    #[test]
    fn event_and_queue_uris_are_found_wherever_they_sit() {
        assert_eq!(
            event_uri(&json!({ "track": { "uri": "spotify:track:x" } })).as_deref(),
            Some("spotify:track:x")
        );
        assert_eq!(event_uri(&json!({ "item": null })), None);
        assert_eq!(
            event_uri(&json!({ "now": { "song": "spotify:track:y" },
                               "next_tracks": [{ "uri": "spotify:track:z" }] }))
            .as_deref(),
            Some("spotify:track:y")
        );
        let q = json!({ "type": "queue_changed", "next_tracks": [
            { "uri": "spotify:track:1", "name": "a" },
            { "track": { "uri": "spotify:track:2" } },
            { "uri": "spotify:episode:3" }
        ]});
        assert_eq!(queue_uris(&q), ["spotify:track:1", "spotify:track:2"]);
    }

    #[test]
    fn queue_tracks_carry_soloists_metadata() {
        let q = json!({ "type": "queue_changed", "upcoming": [{ "source": "autoplay", "item": {
            "uri": "spotify:track:abc", "entity_type": "track",
            "decorations": {
                "identity": { "name": "Melodyjki" },
                "visual_identity": { "cover": [
                    { "url": "s", "size": "small" }, { "url": "d", "size": "default" },
                    { "url": "x", "size": "xlarge" } ] },
                "parent": { "entity": { "decorations": { "identity": { "name": "Album" } } } },
                "creators": [
                    { "entity": { "decorations": { "identity": { "name": "Borixon" } } } },
                    { "entity": { "decorations": { "identity": { "name": "ReTo" } } } } ],
                "playback": { "duration_ms": 144000 } } } }] });
        let t = &queue_tracks(&q)[0];
        assert_eq!(t.id.key(), "sp:abc");
        assert_eq!(t.uri.as_deref(), Some("spotify:track:abc"));
        assert_eq!(
            (t.title.as_str(), t.artist.as_str()),
            ("Melodyjki", "Borixon, ReTo")
        );
        assert_eq!(t.album, "Album");
        assert_eq!((t.artwork.as_str(), t.thumb.as_str()), ("x", "d"));
        assert_eq!(t.duration, 144_000);
    }

    #[test]
    fn queue_is_the_upcoming_list_never_the_history() {
        // The shape Soloist 1.4 sends: history first, then what's next.
        let q = json!({ "type": "queue_changed",
        "previous": [{ "source": "context", "item": { "uri": "spotify:track:old" } }],
        "upcoming": [
            { "source": "autoplay", "item": { "uri": "spotify:track:n1" } },
            { "source": "autoplay", "item": { "uri": "spotify:track:n2" } }
        ]});
        assert_eq!(queue_uris(&q), ["spotify:track:n1", "spotify:track:n2"]);
        let only_history = json!({ "type": "queue_changed",
            "previous": [{ "item": { "uri": "spotify:track:old" } }] });
        assert!(queue_uris(&only_history).is_empty());
    }

    #[test]
    fn a_track_soloist_is_already_playing_is_adopted_not_restarted() {
        let mut t = Tracker::default();
        t.on_event(&playing("spotify:track:b"));
        t.begin("spotify:track:b", 0, target(Arc::default()));
        assert!(t.started);
        t.begin("spotify:track:c", 0, target(Arc::default()));
        assert!(!t.started);
    }

    #[test]
    fn settings_need_a_key_and_a_bad_path_finds_nothing() {
        assert!(!SoloistSettings::default().configured());
        assert!(SoloistSettings {
            api_key: "k".into(),
            ..Default::default()
        }
        .configured());
        assert!(find_binary("/definitely/not/here/soloist").is_none());
    }
}
