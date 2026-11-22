use std::time::Instant;

use mosaic_audio::{AfterFade, Command, Curve};
use mosaic_core::model::{Repeat, Source};

use super::{Effect, PlayerController};

const LEAD: f64 = 1.0;

impl PlayerController {
    pub(super) fn maybe_crossfade(&mut self, secs: f64, now: Instant, fx: &mut Vec<Effect>) {
        let seconds = self.settings.crossfade;
        let length = self.time.duration;
        if seconds <= 0.0 || self.track.is_none() || !self.playing || length <= 0.0 {
            return;
        }
        let left = length - secs;
        if left > seconds + LEAD || left < 0.4 || length < seconds * 2.0 {
            return;
        }
        if self.crossfade_for == Some(self.gen) {
            return;
        }
        if self.settings.repeat == Repeat::One || self.sleep.is_end_of_track() {
            return;
        }
        if self.next_playable(1).is_none() {
            return;
        }
        let upcoming = usize::try_from(self.queue.index + 1)
            .ok()
            .and_then(|i| self.queue.list.get(i));
        if upcoming.is_some_and(|t| t.source == Source::Spotify) {
            return;
        }
        self.crossfade_for = Some(self.gen);
        self.crossfading = seconds;
        self.hand_over((left - 0.1).max(0.5), fx);
        self.step(1, true, false, now, fx);
    }

    fn hand_over(&mut self, seconds: f64, fx: &mut Vec<Effect>) {
        let outgoing = self.active;
        self.active = outgoing.other();
        self.handed_over = true;
        fx.push(Effect::Engine(Command::Fade {
            deck: outgoing,
            to: 0.0,
            ms: (seconds * 1000.0).round() as u32,
            curve: Curve::Out,
            then: AfterFade::Unload,
        }));
    }
}
