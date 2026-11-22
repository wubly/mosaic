mod control;
mod deck;
pub mod decode;
pub mod dsp;
mod output;
pub mod resample;
pub mod source;
pub mod stretch;

use std::path::PathBuf;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

pub use dsp::analyser::{Analyser, BINS as ANALYSER_BINS};
pub use output::OfflineRenderer;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum DeckId {
    A,
    B,
}

impl DeckId {
    pub fn other(self) -> DeckId {
        match self {
            DeckId::A => DeckId::B,
            DeckId::B => DeckId::A,
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            DeckId::A => 0,
            DeckId::B => 1,
        }
    }
}

/// Shapes of a volume ramp, matching `fade.js`: `In` is sin, `Out` is 1 - cos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Curve {
    Linear,
    In,
    Out,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterFade {
    Nothing,
    Pause,
    Unload,
}

#[derive(Clone, Debug)]
pub enum SourceSpec {
    File(PathBuf),
    Http { url: String },
    Hls { playlist_url: String },
}

#[derive(Clone, Debug)]
pub enum Command {
    Load {
        deck: DeckId,
        gen: u64,
        source: SourceSpec,
        start_at: f64,
        autoplay: bool,
        fade_in_ms: u32,
        duration_hint: Option<f64>,
    },
    Play {
        deck: DeckId,
    },
    Pause {
        deck: DeckId,
    },
    Unload {
        deck: DeckId,
    },
    Seek {
        deck: DeckId,
        secs: f64,
    },
    Fade {
        deck: DeckId,
        to: f32,
        ms: u32,
        curve: Curve,
        then: AfterFade,
    },
    SetVolume(f32),
    SetEq {
        enabled: bool,
        gains_db: [f32; 5],
    },
    SetLeveling(bool),
    SetSpeed(f32),
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Ready {
        deck: DeckId,
        gen: u64,
        duration: Option<f64>,
    },
    Playing {
        deck: DeckId,
        gen: u64,
    },
    Paused {
        deck: DeckId,
        gen: u64,
    },
    Buffering {
        deck: DeckId,
        gen: u64,
        on: bool,
    },
    Position {
        deck: DeckId,
        gen: u64,
        secs: f64,
    },
    Seeked {
        deck: DeckId,
        gen: u64,
    },
    FadeDone {
        deck: DeckId,
        gen: u64,
    },
    Ended {
        deck: DeckId,
        gen: u64,
    },
    Error {
        deck: DeckId,
        gen: u64,
        message: String,
    },
    DeviceChanged,
}

pub(crate) enum Msg {
    Cmd(Command),
    Sync(Sender<()>),
}

pub struct Engine {
    tx: Sender<Msg>,
    events: Receiver<Event>,
    analyser: Analyser,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Opens the default output device and starts the control thread.
    pub fn start() -> anyhow::Result<Engine> {
        control::spawn(control::Mode::Device).map(|(engine, _)| engine)
    }

    /// An engine with no device; the returned renderer pulls the mixer (for tests).
    pub fn start_offline(sample_rate: u32) -> (Engine, OfflineRenderer) {
        let (engine, renderer) = control::spawn(control::Mode::Offline(sample_rate))
            .expect("offline engine always starts");
        (engine, renderer.expect("offline mode returns a renderer"))
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }

    pub fn events(&self) -> Receiver<Event> {
        self.events.clone()
    }

    pub fn analyser(&self) -> Analyser {
        self.analyser.clone()
    }

    /// Blocks until the control thread has processed everything sent so far.
    pub fn sync(&self) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        if self.tx.send(Msg::Sync(tx)).is_ok() {
            let _ = rx.recv();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Cmd(Command::Shutdown));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
