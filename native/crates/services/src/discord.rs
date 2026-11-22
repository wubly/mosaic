//! Port of `electron/discord/{presence,activity}.js`. The IPC crate is blocking, so everything
//! runs on one std thread driven by a command channel.

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use discord_rich_presence::activity::{
    Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps,
};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

pub const CLIENT_ID: &str = "1562187683592147045";
const RETRY: Duration = Duration::from_secs(15);
const MIN_PUSH_GAP: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Payload {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: String,
    pub url: Option<String>,
    pub duration_ms: f64,
    pub position_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub connected: bool,
    pub error: Option<String>,
}

/// The fields of the Rich Presence activity, independent of the IPC crate.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivitySpec {
    pub details: String,
    pub state: String,
    pub large_text: String,
    pub large_image: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub button: Option<(String, String)>,
}

fn clip(value: &str, fallback: &str) -> String {
    let raw = if value.is_empty() { fallback } else { value };
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = collapsed.chars().take(120).collect();
    if text.chars().count() >= 2 {
        return text;
    }
    let base = if !text.is_empty() {
        text
    } else if !fallback.is_empty() {
        fallback.to_string()
    } else {
        "—".to_string()
    };
    format!("{base}  ")
}

/// `buildActivity`. `now_ms` is the wall clock the timestamps are anchored to.
pub fn build_activity(p: &Payload, now_ms: i64) -> ActivitySpec {
    let album = p.album.trim();
    let album = if album.to_lowercase() != p.title.trim().to_lowercase() {
        album
    } else {
        ""
    };
    let timed = p.duration_ms > 0.0 && p.position_ms >= 0.0;
    let start = now_ms as f64 - p.position_ms;
    ActivitySpec {
        details: clip(&p.title, "Unknown track"),
        state: clip(&p.artist, "Unknown artist"),
        large_text: clip(album, "Mosaic"),
        large_image: p.artwork.starts_with("https://").then(|| p.artwork.clone()),
        start: timed.then(|| start.round() as i64),
        end: timed.then(|| (start + p.duration_ms).round() as i64),
        button: p
            .url
            .as_ref()
            .filter(|u| u.starts_with("https://soundcloud.com/"))
            .map(|u| ("Listen on SoundCloud".to_string(), u.clone())),
    }
}

enum Command {
    Enable(bool),
    Update(Option<Payload>),
    Shutdown,
}

#[derive(Clone)]
pub struct Discord {
    tx: Sender<Command>,
    status: Arc<Mutex<Status>>,
}

impl Discord {
    pub fn spawn() -> Discord {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = Arc::clone(&status);
        std::thread::spawn(move || Worker::new(shared).run(&rx));
        Discord { tx, status }
    }

    pub fn set_enabled(&self, on: bool) {
        let _ = self.tx.send(Command::Enable(on));
    }

    /// `None` clears the activity.
    pub fn update(&self, payload: Option<Payload>) {
        let _ = self.tx.send(Command::Update(payload));
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

struct Worker {
    status: Arc<Mutex<Status>>,
    client: Option<DiscordIpcClient>,
    enabled: bool,
    pending: Option<(Option<Payload>, Instant)>,
    last_push: Option<Instant>,
    next_retry: Instant,
}

impl Worker {
    fn new(status: Arc<Mutex<Status>>) -> Worker {
        Worker {
            status,
            client: None,
            enabled: false,
            pending: None,
            last_push: None,
            next_retry: Instant::now(),
        }
    }

    fn publish(&self, error: Option<String>) {
        if let Ok(mut s) = self.status.lock() {
            *s = Status {
                enabled: self.enabled,
                connected: self.client.is_some(),
                error,
            };
        }
    }

    fn error(&self) -> Option<String> {
        self.status.lock().ok().and_then(|s| s.error.clone())
    }

    fn next_deadline(&self, now: Instant) -> Option<Instant> {
        if !self.enabled {
            return None;
        }
        if self.client.is_none() {
            return Some(self.next_retry.max(now));
        }
        self.pending.as_ref()?;
        Some(self.last_push.map_or(now, |t| t + MIN_PUSH_GAP).max(now))
    }

    fn run(mut self, rx: &std::sync::mpsc::Receiver<Command>) {
        loop {
            let now = Instant::now();
            let command = match self.next_deadline(now) {
                Some(at) => match rx.recv_timeout(at.saturating_duration_since(now)) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => break,
                },
            };
            match command {
                Some(Command::Shutdown) => break,
                Some(Command::Enable(on)) => self.set_enabled(on),
                Some(Command::Update(p)) if self.enabled => {
                    self.pending = Some((p, Instant::now()));
                }
                Some(Command::Update(_)) => {}
                None => {}
            }
            self.work();
        }
        self.disconnect();
    }

    fn set_enabled(&mut self, on: bool) {
        if on == self.enabled {
            return;
        }
        if on {
            self.enabled = true;
            self.next_retry = Instant::now();
            self.publish(None);
        } else {
            self.disconnect();
        }
    }

    fn disconnect(&mut self) {
        self.enabled = false;
        self.pending = None;
        if let Some(mut client) = self.client.take() {
            let _ = client.clear_activity();
            let _ = client.close();
        }
        self.publish(None);
    }

    fn work(&mut self) {
        let now = Instant::now();
        if self.enabled && self.client.is_none() && now >= self.next_retry {
            let mut client = DiscordIpcClient::new(CLIENT_ID);
            match client.connect() {
                Ok(()) => {
                    self.client = Some(client);
                    self.last_push = None;
                    self.publish(None);
                }
                Err(_) => {
                    self.next_retry = now + RETRY;
                    self.publish(None);
                }
            }
        }
        let due = self.last_push.is_none_or(|t| now >= t + MIN_PUSH_GAP);
        if !(self.client.is_some() && due) {
            return;
        }
        let Some((payload, received)) = self.pending.clone() else {
            return;
        };
        let result = self.push(payload.as_ref(), received);
        self.last_push = Some(Instant::now());
        match result {
            Ok(()) => {
                self.pending = None;
                if self.error().is_some() {
                    self.publish(None);
                }
            }
            Err(e) => {
                self.client = None;
                self.next_retry = Instant::now() + RETRY;
                self.publish(Some(e.chars().take(140).collect()));
            }
        }
    }

    fn push(&mut self, payload: Option<&Payload>, received: Instant) -> Result<(), String> {
        let client = self.client.as_mut().ok_or("not connected")?;
        let Some(p) = payload else {
            return client.clear_activity().map_err(|e| e.to_string());
        };
        let mut p = p.clone();
        p.position_ms += received.elapsed().as_secs_f64() * 1000.0;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        let spec = build_activity(&p, now_ms);

        let mut assets = Assets::new().large_text(spec.large_text.as_str());
        if let Some(image) = &spec.large_image {
            assets = assets.large_image(image.as_str());
        }
        let mut activity = Activity::new()
            .activity_type(ActivityType::Listening)
            .status_display_type(StatusDisplayType::State)
            .details(spec.details.as_str())
            .state(spec.state.as_str())
            .assets(assets);
        if let (Some(start), Some(end)) = (spec.start, spec.end) {
            activity = activity.timestamps(Timestamps::new().start(start).end(end));
        }
        if let Some((label, url)) = &spec.button {
            activity = activity.buttons(vec![Button::new(label.as_str(), url.as_str())]);
        }
        client.set_activity(activity).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Payload {
        Payload {
            title: "Midnight   City".into(),
            artist: "M83".into(),
            album: "Hurry Up, We're Dreaming".into(),
            artwork: "https://i1.sndcdn.com/a.jpg".into(),
            url: Some("https://soundcloud.com/m83/midnight-city".into()),
            duration_ms: 240_000.0,
            position_ms: 30_000.0,
        }
    }

    #[test]
    fn builds_a_listening_activity_with_timestamps_and_button() {
        let a = build_activity(&payload(), 1_000_000);
        assert_eq!(a.details, "Midnight City");
        assert_eq!(a.state, "M83");
        assert_eq!(a.large_text, "Hurry Up, We're Dreaming");
        assert_eq!(
            a.large_image.as_deref(),
            Some("https://i1.sndcdn.com/a.jpg")
        );
        assert_eq!((a.start, a.end), (Some(970_000), Some(1_210_000)));
        assert_eq!(a.button.map(|b| b.0), Some("Listen on SoundCloud".into()));
    }

    #[test]
    fn omits_optional_parts() {
        let mut p = payload();
        p.title = "Midnight City".into();
        p.album = "midnight city".into();
        p.artwork = "/home/u/cover.jpg".into();
        p.url = Some("https://example.com/x".into());
        p.duration_ms = 0.0;
        let a = build_activity(&p, 5);
        assert_eq!(a.large_text, "Mosaic");
        assert!(a.large_image.is_none());
        assert!(a.button.is_none());
        assert_eq!((a.start, a.end), (None, None));
    }

    #[test]
    fn clip_pads_short_values_and_truncates_long_ones() {
        assert_eq!(clip("", "Unknown track"), "Unknown track");
        assert_eq!(clip("a", "Unknown track"), "a  ");
        assert_eq!(clip("", ""), "—  ");
        assert_eq!(clip("  ", "fb"), "fb  ");
        assert_eq!(clip(&"x".repeat(200), "f").chars().count(), 120);
        assert_eq!(clip("  a \n b ", "f"), "a b");
    }
}
