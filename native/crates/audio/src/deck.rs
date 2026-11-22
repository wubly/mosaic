use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer};

use crate::dsp::fade::FadeRun;
use crate::{AfterFade, Curve, DeckId};

pub const NO_EOF: u64 = u64::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkerKind {
    Pos,
    Flush,
}

/// Ties a frame index in the sample ring to a position in the source.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub kind: MarkerKind,
    pub epoch: u32,
    pub frame: u64,
    pub pos: f64,
}

#[derive(Clone, Copy, Debug)]
pub enum DeckMsg {
    Play,
    Pause,
    SetGain(f32),
    Fade {
        to: f32,
        frames: u32,
        curve: Curve,
        then: AfterFade,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum CbEvent {
    FadeDone { deck: DeckId, then: AfterFade },
    Ended { deck: DeckId, epoch: u32 },
}

/// State both sides of a deck read and write without locking.
pub struct DeckShared {
    pub flush_epoch: AtomicU32,
    pub position: AtomicU64,
    pub eof_frame: AtomicU64,
    pub starved: AtomicBool,
}

impl DeckShared {
    pub fn new() -> DeckShared {
        DeckShared {
            flush_epoch: AtomicU32::new(0),
            position: AtomicU64::new(0f64.to_bits()),
            eof_frame: AtomicU64::new(NO_EOF),
            starved: AtomicBool::new(false),
        }
    }

    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }

    pub fn set_position(&self, secs: f64) {
        self.position.store(secs.to_bits(), Ordering::Relaxed);
    }
}

/// The decoder side of a deck's rings. `pushed` counts frames ever written.
pub struct Rings {
    pub samples: Producer<f32>,
    pub markers: Producer<Marker>,
    pub pushed: u64,
}

/// The audio-callback side of a deck.
pub struct DeckPlayer {
    id: DeckId,
    samples: Consumer<f32>,
    markers: Consumer<Marker>,
    msgs: Consumer<DeckMsg>,
    shared: Arc<DeckShared>,
    playing: bool,
    gain: f32,
    fade: Option<FadeRun>,
    popped: u64,
    base_frame: u64,
    base_pos: f64,
    seen_epoch: u32,
    awaiting: bool,
    ended: bool,
}

impl DeckPlayer {
    pub fn new(
        id: DeckId,
        samples: Consumer<f32>,
        markers: Consumer<Marker>,
        msgs: Consumer<DeckMsg>,
        shared: Arc<DeckShared>,
    ) -> DeckPlayer {
        DeckPlayer {
            id,
            samples,
            markers,
            msgs,
            shared,
            playing: false,
            gain: 1.0,
            fade: None,
            popped: 0,
            base_frame: 0,
            base_pos: 0.0,
            seen_epoch: 0,
            awaiting: false,
            ended: false,
        }
    }

    fn drain_msgs(&mut self, events: &mut Producer<CbEvent>) {
        while let Ok(msg) = self.msgs.pop() {
            match msg {
                DeckMsg::Play => self.playing = true,
                DeckMsg::Pause => self.playing = false,
                DeckMsg::SetGain(g) => {
                    self.gain = g;
                    self.fade = None;
                }
                DeckMsg::Fade {
                    to,
                    frames,
                    curve,
                    then,
                } => {
                    if frames == 0 {
                        self.gain = to;
                        self.fade = None;
                        self.finish_fade(then, events);
                    } else {
                        self.fade = Some(FadeRun::new(self.gain, to, frames, curve, then));
                    }
                }
            }
        }
    }

    fn finish_fade(&mut self, then: AfterFade, events: &mut Producer<CbEvent>) {
        if then == AfterFade::Pause {
            self.playing = false;
        }
        let _ = events.push(CbEvent::FadeDone {
            deck: self.id,
            then,
        });
    }

    fn sync_epoch(&mut self) {
        let epoch = self.shared.flush_epoch.load(Ordering::Acquire);
        if epoch != self.seen_epoch {
            self.seen_epoch = epoch;
            self.awaiting = true;
            self.ended = false;
        }
    }

    /// Waits for the decoder's flush marker, then drops everything queued before it.
    fn resync(&mut self) {
        while let Ok(m) = self.markers.pop() {
            if m.kind != MarkerKind::Flush || m.epoch != self.seen_epoch {
                continue;
            }
            let mut skip = m.frame.saturating_sub(self.popped) as usize * 2;
            while skip > 0 {
                let n = skip.min(self.samples.slots());
                if n == 0 {
                    break;
                }
                if let Ok(chunk) = self.samples.read_chunk(n) {
                    chunk.commit_all();
                }
                skip -= n;
            }
            self.popped = m.frame;
            self.base_frame = m.frame;
            self.base_pos = m.pos;
            self.awaiting = false;
            return;
        }
    }

    fn publish_position(&mut self, rate: u32, speed: f32) {
        while let Ok(m) = self.markers.peek() {
            let m = *m;
            if m.epoch != self.seen_epoch || m.kind == MarkerKind::Flush {
                let _ = self.markers.pop();
            } else if m.frame <= self.popped {
                self.base_frame = m.frame;
                self.base_pos = m.pos;
                let _ = self.markers.pop();
            } else {
                break;
            }
        }
        if self.shared.flush_epoch.load(Ordering::Acquire) != self.seen_epoch {
            return;
        }
        let elapsed = (self.popped - self.base_frame) as f64 / f64::from(rate);
        self.shared
            .set_position(self.base_pos + elapsed * f64::from(speed));
    }

    /// Adds this deck's next `out.len() / 2` stereo frames into `out`.
    pub fn mix(
        &mut self,
        out: &mut [f32],
        rate: u32,
        speed: f32,
        blocking: bool,
        events: &mut Producer<CbEvent>,
    ) {
        self.drain_msgs(events);
        self.sync_epoch();
        if self.awaiting {
            self.resync();
            if self.awaiting {
                return;
            }
        }
        if !self.playing || self.ended {
            self.publish_position(rate, speed);
            return;
        }

        let frames = out.len() / 2;
        let eof = self.shared.eof_frame.load(Ordering::Acquire);
        let want = if eof == NO_EOF {
            frames
        } else {
            frames.min(eof.saturating_sub(self.popped) as usize)
        };

        if blocking && self.samples.slots() / 2 < want {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while self.samples.slots() / 2 < want && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
        }

        let avail = self.samples.slots() / 2;
        let n = want.min(avail);
        let mut consumed = 0;
        let mut gain = self.gain;
        let mut fade = self.fade.take();
        let mut finished: Option<AfterFade> = None;
        if n > 0 {
            if let Ok(chunk) = self.samples.read_chunk(n * 2) {
                let (a, b) = chunk.as_slices();
                for (i, s) in a
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .chain(b.as_chunks::<2>().0.iter())
                    .enumerate()
                {
                    if let Some(f) = &mut fade {
                        let (g, done) = f.step();
                        gain = g;
                        if done {
                            finished = Some(f.then);
                            fade = None;
                        }
                    }
                    out[i * 2] += s[0] * gain;
                    out[i * 2 + 1] += s[1] * gain;
                    consumed = i + 1;
                    if finished == Some(AfterFade::Pause) {
                        break;
                    }
                }
                chunk.commit(consumed * 2);
            }
        }
        self.gain = gain;
        self.fade = fade;
        if let Some(then) = finished {
            self.finish_fade(then, events);
        }
        self.popped += consumed as u64;

        let starving = consumed < frames && eof == NO_EOF && self.playing;
        self.shared.starved.store(starving, Ordering::Relaxed);

        if eof != NO_EOF && self.popped >= eof && !self.ended {
            self.ended = true;
            self.playing = false;
            let _ = events.push(CbEvent::Ended {
                deck: self.id,
                epoch: self.seen_epoch,
            });
        }
        self.publish_position(rate, speed);
    }
}
