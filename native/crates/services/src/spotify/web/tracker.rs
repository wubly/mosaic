//! Playback state mirrored from the web player page, plus the track Mosaic asked for. Pure: no
//! I/O, so every rule here is unit-tested. See docs/spotify-cef-backend.md, section 6.3.
//!
//! What the Web Playback SDK reports, as measured (WebView2, 2026-11):
//! - `position_ms` arrives already extrapolated by the page (the SDK's own position is a
//!   snapshot at `timestamp`); we anchor it to when we heard it.
//! - Spotify relinks tracks to the market's version: the requested URI may come back as
//!   `linked_from`, with a different `uri`.
//! - A single-URI play ends as `paused` at position 0 on the same track.

use std::time::Instant;

use serde::Deserialize;

use crate::spotify::playback::Target;

/// One `state` message from the page.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PlayerState {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub linked_from: Option<String>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub loading: bool,
    #[serde(default)]
    pub position_ms: f64,
    #[serde(default)]
    pub duration_ms: f64,
}

/// What the player wants done after a message.
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    /// The loaded track ended, or Spotify moved on to another one: finish the source and pause
    /// Spotify (Mosaic plays its own next track).
    Finish,
    Fail(String),
}

pub struct Tracker {
    /// Bumped on every load/stop so stale feeders and timeouts bow out.
    pub gen: u64,
    pub want: Option<String>,
    pub target: Option<Target>,
    pub started: bool,
    /// Mosaic paused it (a pause at 0 is then not an end of track).
    pub paused_by_us: bool,
    playing: bool,
    anchor_ms: f64,
    anchor_at: Instant,
    /// Where the silent feed is, in track seconds.
    pub fed: f64,
    pub volume: Option<f64>,
    /// The page's latest report, whichever track it was about.
    pub last: Option<PlayerState>,
    /// Spotify's up-next list, the track it's for and when it was last asked for.
    pub queue: (Option<String>, Vec<mosaic_core::model::Track>),
    pub queue_asked: Option<Instant>,
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker {
            gen: 0,
            want: None,
            target: None,
            started: false,
            paused_by_us: false,
            playing: false,
            anchor_ms: 0.0,
            anchor_at: Instant::now(),
            fed: 0.0,
            volume: None,
            last: None,
            queue: (None, Vec::new()),
            queue_asked: None,
        }
    }
}

/// How far the silent feed may run ahead of the player's position.
pub const LEAD_SECS: f64 = 0.6;

impl Tracker {
    fn ours(&self, st: &PlayerState) -> bool {
        let Some(want) = self.want.as_deref() else {
            return false;
        };
        st.uri.as_deref() == Some(want) || st.linked_from.as_deref() == Some(want)
    }

    /// The player's position now, in seconds: running while it plays, held otherwise.
    pub fn position(&self) -> f64 {
        let ms = if self.playing {
            self.anchor_ms + self.anchor_at.elapsed().as_secs_f64() * 1000.0
        } else {
            self.anchor_ms
        };
        ms / 1000.0
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Starts tracking a new load; returns its generation.
    pub fn begin(&mut self, uri: &str, start_ms: u32, target: Target) -> u64 {
        self.gen += 1;
        self.want = Some(uri.to_string());
        self.target = Some(target);
        self.started = false;
        self.paused_by_us = false;
        self.playing = false;
        self.anchor_ms = f64::from(start_ms);
        self.anchor_at = Instant::now();
        self.fed = f64::from(start_ms) / 1000.0;
        self.gen
    }

    /// Drops the current load; returns its target.
    pub fn clear(&mut self) -> Option<Target> {
        self.gen += 1;
        self.want = None;
        self.started = false;
        self.playing = false;
        self.target.take()
    }

    /// Mosaic seeked: the feed and the anchor jump there.
    pub fn seek(&mut self, position_ms: u32) {
        self.anchor_ms = f64::from(position_ms);
        self.anchor_at = Instant::now();
        self.fed = f64::from(position_ms) / 1000.0;
    }

    pub fn set_paused_by_us(&mut self, paused: bool) {
        self.paused_by_us = paused;
        if paused {
            // Hold the clock where it is until the player confirms.
            self.anchor_ms = self.position() * 1000.0;
            self.anchor_at = Instant::now();
            self.playing = false;
        }
    }

    /// Whether Spotify is already playing `uri` (it moved on to it by itself), so a load can
    /// take it over where it is instead of restarting it.
    pub fn already_playing(&self, uri: &str) -> bool {
        self.last.as_ref().is_some_and(|st| {
            !st.paused && (st.uri.as_deref() == Some(uri) || st.linked_from.as_deref() == Some(uri))
        })
    }

    /// The track Spotify reported last, when it isn't the one Mosaic asked for.
    pub fn moved_to(&self) -> Option<String> {
        let st = self.last.as_ref()?;
        let uri = st.uri.clone()?;
        let want = self.want.as_deref();
        (Some(uri.as_str()) != want && st.linked_from.as_deref() != want).then_some(uri)
    }

    /// Folds one `state` message in and says what to do about it.
    pub fn on_state(&mut self, st: &PlayerState) -> Action {
        if st.uri.is_some() {
            self.last = Some(st.clone());
        }
        if self.want.is_none() {
            return Action::None;
        }
        let ours = self.ours(st);
        if ours {
            self.playing = !st.paused && !st.loading;
            self.anchor_ms = st.position_ms.max(0.0);
            self.anchor_at = Instant::now();
            if !st.paused {
                self.paused_by_us = false;
            }
        }
        if !self.started {
            if ours && !st.paused && !st.loading {
                self.started = true;
            }
            return Action::None;
        }
        let other_track = st.uri.is_some() && !ours;
        let ended_here = ours && st.paused && st.position_ms <= 0.0 && !self.paused_by_us;
        if other_track || ended_here {
            Action::Finish
        } else {
            Action::None
        }
    }

    /// An `error` message from the page. Fails a track that hasn't started; later errors are
    /// only logged (the SDK reports transient ones while it keeps playing).
    pub fn on_error(&mut self, message: String) -> Action {
        if self.target.is_some() && !self.started {
            Action::Fail(message)
        } else {
            Action::None
        }
    }

    /// Whether the silent feed may advance: our track is playing and the feed isn't ahead.
    pub fn may_feed(&self, gen: u64) -> bool {
        gen == self.gen && self.started && self.playing && self.fed < self.position() + LEAD_SECS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
    const A_RELINKED: &str = "spotify:track:0yNttAVwMr39qyODHNIkrY";
    const B: &str = "spotify:track:3n3Ppam7vgaVa1iaRUc9Lp";

    fn target() -> Target {
        Target {
            write: Arc::new(|_| true),
            finish: Arc::new(|| {}),
            fail: Arc::new(|_| {}),
            moved: Arc::new(|_| {}),
        }
    }

    fn state(
        uri: &str,
        linked: Option<&str>,
        paused: bool,
        loading: bool,
        pos: f64,
    ) -> PlayerState {
        PlayerState {
            uri: Some(uri.into()),
            linked_from: linked.map(str::to_string),
            paused,
            loading,
            position_ms: pos,
            duration_ms: 213_000.0,
        }
    }

    #[test]
    fn a_relinked_track_starts_and_is_not_mistaken_for_moving_on() {
        let mut t = Tracker::default();
        t.begin(A, 0, target());
        assert_eq!(
            t.on_state(&state(A_RELINKED, Some(A), false, true, 0.0)),
            Action::None
        );
        assert!(!t.started);
        assert_eq!(
            t.on_state(&state(A_RELINKED, Some(A), false, false, 10.0)),
            Action::None
        );
        assert!(t.started && t.playing());
        assert_eq!(
            t.on_state(&state(A_RELINKED, Some(A), false, false, 1000.0)),
            Action::None
        );
    }

    #[test]
    fn paused_at_zero_after_playing_is_the_end_of_the_track() {
        let mut t = Tracker::default();
        t.begin(A, 0, target());
        t.on_state(&state(A, None, false, false, 50.0));
        assert_eq!(
            t.on_state(&state(A, None, true, false, 0.0)),
            Action::Finish
        );
    }

    #[test]
    fn a_pause_from_mosaic_at_zero_is_not_an_end() {
        let mut t = Tracker::default();
        t.begin(A, 0, target());
        t.on_state(&state(A, None, false, false, 5.0));
        t.set_paused_by_us(true);
        assert_eq!(t.on_state(&state(A, None, true, false, 0.0)), Action::None);
        // Resuming clears it, so a later natural end still counts.
        t.on_state(&state(A, None, false, false, 10.0));
        assert_eq!(
            t.on_state(&state(A, None, true, false, 0.0)),
            Action::Finish
        );
    }

    #[test]
    fn a_pause_mid_track_is_not_an_end() {
        let mut t = Tracker::default();
        t.begin(A, 0, target());
        t.on_state(&state(A, None, false, false, 5.0));
        assert_eq!(
            t.on_state(&state(A, None, true, false, 30_000.0)),
            Action::None
        );
    }

    #[test]
    fn another_track_after_ours_means_spotify_moved_on() {
        let mut t = Tracker::default();
        t.begin(A, 0, target());
        t.on_state(&state(A, None, false, false, 5.0));
        assert_eq!(
            t.on_state(&state(B, None, false, false, 0.0)),
            Action::Finish
        );
    }

    #[test]
    fn the_previous_track_still_reported_before_ours_starts_is_ignored() {
        let mut t = Tracker::default();
        t.begin(B, 0, target());
        assert_eq!(
            t.on_state(&state(A, None, false, false, 90_000.0)),
            Action::None
        );
        assert!(!t.started);
        t.on_state(&state(B, None, false, false, 0.0));
        assert!(t.started);
    }

    #[test]
    fn errors_fail_only_a_track_that_has_not_started() {
        let mut t = Tracker::default();
        t.begin(A, 0, target());
        assert_eq!(t.on_error("boom".into()), Action::Fail("boom".into()));
        t.on_state(&state(A, None, false, false, 5.0));
        assert_eq!(t.on_error("late".into()), Action::None);
    }

    #[test]
    fn the_feed_follows_the_live_position_and_never_runs_far_ahead() {
        let mut t = Tracker::default();
        let gen = t.begin(A, 0, target());
        assert!(!t.may_feed(gen), "nothing is fed before the track starts");
        t.on_state(&state(A, None, false, false, 0.0));
        assert!(t.may_feed(gen));
        t.fed = t.position() + LEAD_SECS + 0.1;
        assert!(!t.may_feed(gen));
        assert!(!t.may_feed(gen + 1), "a stale feeder bows out");
    }

    #[test]
    fn a_start_position_and_seeks_move_the_feed() {
        let mut t = Tracker::default();
        t.begin(A, 60_000, target());
        assert!((t.fed - 60.0).abs() < 1e-9);
        t.seek(15_000);
        assert!((t.fed - 15.0).abs() < 1e-9);
        assert!((t.position() - 15.0).abs() < 0.05);
    }

    #[test]
    fn clearing_hands_back_the_target_and_ignores_later_states() {
        let finished = Arc::new(AtomicUsize::new(0));
        let f = Arc::clone(&finished);
        let mut t = Tracker::default();
        t.begin(
            A,
            0,
            Target {
                finish: Arc::new(move || {
                    f.fetch_add(1, Ordering::SeqCst);
                }),
                ..target()
            },
        );
        assert!(t.clear().is_some());
        assert_eq!(t.on_state(&state(A, None, true, false, 0.0)), Action::None);
        assert_eq!(finished.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn page_states_parse_with_missing_fields() {
        let st: PlayerState =
            serde_json::from_str(r#"{"uri":"spotify:track:x","paused":true}"#).unwrap();
        assert_eq!(st.uri.as_deref(), Some("spotify:track:x"));
        assert!(st.paused && !st.loading);
        assert_eq!(st.position_ms, 0.0);
    }
}
