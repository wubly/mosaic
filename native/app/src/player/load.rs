use std::time::Instant;

use mosaic_audio::Command;
use mosaic_core::model::Track;

use super::{Effect, LoadOpts, PlayerController, Stream, STAT_AFTER};

impl PlayerController {
    pub(super) fn load(
        &mut self,
        track: &Track,
        opts: LoadOpts,
        retry: bool,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        self.gen += 1;
        self.opts = opts;
        self.retried = retry;
        self.pending = super::Pending::None;
        self.silence = None;
        self.end_skipped = None;
        self.track = Some(track.clone());
        self.error.clear();
        self.time = super::Time {
            current: opts.seek_to,
            duration: track.duration as f64 / 1000.0,
            ..super::Time::default()
        };
        if !retry {
            self.record_due = Some(now + STAT_AFTER);
        }
        if let Some(reason) = self.unavailable(track) {
            self.error = reason.into();
            self.loading = false;
            self.loaded = false;
            return;
        }
        self.loaded = opts.autoplay;
        self.loading = opts.autoplay;
        fx.push(Effect::ResolveStream {
            track: track.clone(),
            gen: self.gen,
        });
    }

    pub(super) fn on_stream(
        &mut self,
        gen: u64,
        result: Result<Stream, String>,
        fx: &mut Vec<Effect>,
    ) {
        if gen != self.gen {
            return;
        }
        let stream = match result {
            Ok(s) => s,
            Err(message) => {
                self.error = if message.is_empty() {
                    "Could not play this track".into()
                } else {
                    message
                };
                self.loading = false;
                return;
            }
        };
        self.silence = stream.silence;
        let start_at = if self.opts.seek_to > 0.0 {
            self.opts.seek_to
        } else {
            self.start_position()
        };
        let duration_hint = self
            .track
            .as_ref()
            .map(|t| t.duration as f64 / 1000.0)
            .filter(|d| *d > 0.0);
        fx.push(Effect::Engine(Command::Load {
            deck: self.active,
            gen,
            source: stream.source,
            start_at,
            autoplay: self.opts.autoplay,
            fade_in_ms: (self.opts.fade_in * 1000.0).round() as u32,
            duration_hint,
        }));
    }

    pub(super) fn play_index(
        &mut self,
        index: usize,
        opts: LoadOpts,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        let Some(track) = self.queue.list.get(index).cloned() else {
            return;
        };
        self.queue.index = index as isize;
        self.queue_dirty = true;
        self.load(&track, opts, false, now, fx);
    }

    pub(super) fn play(
        &mut self,
        target: &Track,
        list: Option<&[Track]>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        self.queue.play(target, list);
        self.queue_dirty = true;
        self.load(target, LoadOpts::default(), false, now, fx);
    }
}
