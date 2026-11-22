use std::time::Instant;

use mosaic_core::model::{Source, Track};
use mosaic_core::queue::{radio_start_toast, Queue, QueueOutcome, RADIO_EMPTY, RADIO_FAILED};

use super::{Effect, PlayerController, RelatedPurpose};

impl PlayerController {
    fn apply(&mut self, outcome: QueueOutcome, now: Instant, fx: &mut Vec<Effect>) {
        if !matches!(outcome, QueueOutcome::AlreadyQueued | QueueOutcome::Nothing) {
            self.queue_dirty = true;
        }
        if let QueueOutcome::PlayNow(track, list) = outcome {
            return self.play(&track, list.as_deref(), now, fx);
        }
        if let Some(message) = outcome.toast() {
            fx.push(Effect::Toast(message));
        }
    }

    pub(super) fn add_to_queue(&mut self, track: &Track, now: Instant, fx: &mut Vec<Effect>) {
        let outcome = self.queue.add(track);
        self.apply(outcome, now, fx);
    }

    pub(super) fn enqueue_many(&mut self, tracks: &[Track], now: Instant, fx: &mut Vec<Effect>) {
        let outcome = self.queue.enqueue_many(tracks);
        self.apply(outcome, now, fx);
    }

    pub(super) fn play_next(&mut self, track: &Track, now: Instant, fx: &mut Vec<Effect>) {
        let outcome = self.queue.play_next(track);
        self.apply(outcome, now, fx);
    }

    pub(super) fn remove_from_queue(&mut self, index: usize) {
        self.queue.remove(index);
        self.queue_dirty = true;
    }

    pub(super) fn start_radio(&mut self, track: Track, now: Instant, fx: &mut Vec<Effect>) {
        fx.push(Effect::Toast(radio_start_toast(&track.title)));
        if track.source == Source::SoundCloud {
            fx.push(Effect::FetchRelated {
                seed: track,
                purpose: RelatedPurpose::Radio,
            });
        } else {
            self.finish_radio(track, Ok(Vec::new()), now, fx);
        }
    }

    pub(super) fn finish_radio(
        &mut self,
        seed: Track,
        result: Result<Vec<Track>, String>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        match result {
            Ok(related) => {
                let empty = related.is_empty();
                let list = Queue::radio_list(&seed, related);
                self.play(&seed, Some(&list), now, fx);
                if empty {
                    fx.push(Effect::Toast(RADIO_EMPTY.into()));
                }
            }
            Err(_) => fx.push(Effect::Toast(RADIO_FAILED.into())),
        }
    }
}
