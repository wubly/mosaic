use std::time::{Duration, Instant};

use mosaic_audio::{AfterFade, Command, Curve};

use super::{Effect, Pending, PlayerController};

const FADE_MS: u32 = 2500;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sleep {
    Off,
    Timer {
        ends_at: Instant,
        left: u64,
    },
    EndOfTrack,
    /// The final fade is running; the timer must not fire again.
    Fading,
}

#[derive(Clone, Copy, Debug)]
pub enum SleepRequest {
    Off,
    Minutes(u32),
    EndOfTrack,
}

impl Sleep {
    pub fn is_end_of_track(&self) -> bool {
        *self == Sleep::EndOfTrack
    }
}

impl PlayerController {
    pub(super) fn set_sleep(&mut self, req: SleepRequest, now: Instant, fx: &mut Vec<Effect>) {
        let (sleep, message) = match req {
            SleepRequest::Minutes(m) => (
                Sleep::Timer {
                    ends_at: now + Duration::from_secs(u64::from(m) * 60),
                    left: u64::from(m) * 60,
                },
                format!("Sleep timer set for {m} min"),
            ),
            SleepRequest::EndOfTrack => (Sleep::EndOfTrack, "Will stop after this track".into()),
            SleepRequest::Off => (Sleep::Off, "Sleep timer off".into()),
        };
        self.sleep = sleep;
        fx.push(Effect::Toast(message));
    }

    pub(super) fn take_end_of_track(&mut self, fx: &mut Vec<Effect>) -> bool {
        if !self.sleep.is_end_of_track() {
            return false;
        }
        self.sleep = Sleep::Off;
        fx.push(Effect::Toast("Sleep timer: stopped at end of track".into()));
        true
    }

    pub(super) fn tick_sleep(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        let Sleep::Timer { ends_at, .. } = self.sleep else {
            return;
        };
        let left = (ends_at.saturating_duration_since(now).as_secs_f64()).round() as u64;
        if left > 0 {
            self.sleep = Sleep::Timer { ends_at, left };
            return;
        }
        self.sleep = Sleep::Fading;
        self.pending = Pending::SleepEnd;
        fx.push(Effect::Engine(Command::Fade {
            deck: self.active,
            to: 0.0,
            ms: FADE_MS,
            curve: Curve::Linear,
            then: AfterFade::Pause,
        }));
    }
}
