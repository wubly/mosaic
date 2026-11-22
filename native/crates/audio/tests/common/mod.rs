#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use mosaic_audio::{Command, DeckId, Engine, Event, OfflineRenderer, SourceSpec};

pub const OUT_RATE: u32 = 48_000;

pub fn write_wav(dir: &Path, name: &str, rate: u32, segments: &[(f32, f32)], amp: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    let mut n = 0u64;
    for (freq, secs) in segments {
        for _ in 0..(*secs * rate as f32) as usize {
            let v = (2.0 * std::f32::consts::PI * freq * n as f32 / rate as f32).sin() * amp;
            let s = (v * 32767.0) as i16;
            w.write_sample(s).unwrap();
            w.write_sample(s).unwrap();
            n += 1;
        }
    }
    w.finalize().unwrap();
    path
}

pub struct Rig {
    pub engine: Engine,
    pub renderer: OfflineRenderer,
    pub seen: Vec<Event>,
}

impl Rig {
    pub fn new() -> Rig {
        let (engine, renderer) = Engine::start_offline(OUT_RATE);
        Rig {
            engine,
            renderer,
            seen: Vec::new(),
        }
    }

    pub fn load(
        &mut self,
        deck: DeckId,
        path: &Path,
        start_at: f64,
        autoplay: bool,
        fade_in_ms: u32,
    ) {
        self.engine.send(Command::Load {
            deck,
            gen: 1,
            source: SourceSpec::File(path.to_path_buf()),
            start_at,
            autoplay,
            fade_in_ms,
            duration_hint: None,
        });
    }

    /// Collects events until one matches, or fails after a timeout.
    pub fn wait_for(&mut self, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
        let rx = self.engine.events();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        if let Some(i) = self.seen.iter().position(&pred) {
            return self.seen.remove(i);
        }
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(e) if pred(&e) => return e,
                Ok(e) => self.seen.push(e),
                Err(_) => panic!("timed out waiting for {what}; saw {:?}", self.seen),
            }
        }
    }

    pub fn drain(&mut self) {
        self.engine.sync();
        while let Ok(e) = self.engine.events().try_recv() {
            self.seen.push(e);
        }
    }

    pub fn render_secs(&mut self, secs: f32) -> Vec<f32> {
        let mut out = vec![0.0; (secs * OUT_RATE as f32) as usize * 2];
        self.renderer.render(&mut out);
        self.engine.sync();
        out
    }
}

pub fn mono(stereo: &[f32]) -> Vec<f32> {
    stereo.as_chunks::<2>().0.iter().map(|f| f[0]).collect()
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

pub fn db(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

/// Power at `freq` (Goertzel), for picking the dominant tone.
pub fn power_at(x: &[f32], freq: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * freq / OUT_RATE as f32;
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for v in x {
        let s0 = v + 2.0 * w.cos() * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2
}

pub fn peak_freq(x: &[f32], lo: f32, hi: f32) -> f32 {
    let mut best = (lo, 0.0f32);
    let mut f = lo;
    while f <= hi {
        let p = power_at(x, f);
        if p > best.1 {
            best = (f, p);
        }
        f += 1.0;
    }
    best.0
}
