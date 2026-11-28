//! Loudness of a track as it decodes (ITU-R BS.1770: K-weighting, 400 ms windows, absolute
//! and relative gating) and the gain that brings it to a common level, so SoundCloud, YouTube
//! and Monochrome play about as loud as each other and as Spotify, which normalizes to the
//! same −14 LUFS.

/// The level every track is brought to (Spotify's and YouTube's default).
pub const TARGET_LUFS: f32 = -14.0;
/// Quiet masters are lifted at most this much (and never into clipping); loud ones are cut at
/// most `MAX_CUT_DB`.
const MAX_BOOST_DB: f32 = 9.0;
const MAX_CUT_DB: f32 = 18.0;
/// Headroom kept under the loudest sample when lifting.
const PEAK_HEADROOM_DB: f32 = 0.5;

const BLOCK_SECS: f64 = 0.1;
/// Gating windows are four blocks (400 ms) long, a new one every block (75 % overlap).
const WINDOW_BLOCKS: usize = 4;
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;

/// One biquad section (direct form I) per channel.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [[f64; 2]; 2],
    y: [[f64; 2]; 2],
}

impl Biquad {
    fn run(&mut self, ch: usize, x0: f64) -> f64 {
        let [x1, x2] = self.x[ch];
        let [y1, y2] = self.y[ch];
        let y0 = self.b[0] * x0 + self.b[1] * x1 + self.b[2] * x2 - self.a[0] * y1 - self.a[1] * y2;
        self.x[ch] = [x0, x1];
        self.y[ch] = [y0, y1];
        y0
    }
}

/// The two K-weighting stages for `rate` (coefficients as in libebur128).
fn k_weighting(rate: u32) -> [Biquad; 2] {
    let fs = f64::from(rate.max(8000));
    let shelf = {
        let (f0, g, q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        Biquad {
            b: [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            ..Biquad::default()
        }
    };
    let high_pass = {
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        Biquad {
            b: [1.0, -2.0, 1.0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            ..Biquad::default()
        }
    };
    [shelf, high_pass]
}

fn lufs(mean_square: f64) -> f64 {
    -0.691 + 10.0 * mean_square.max(1e-12).log10()
}

pub struct LoudnessMeter {
    filters: [Biquad; 2],
    block_frames: usize,
    /// Sum of squares of the block being filled, and its frames so far.
    acc: f64,
    filled: usize,
    /// The last few blocks' mean squares, for the sliding window.
    recent: [f64; WINDOW_BLOCKS],
    blocks: usize,
    /// Mean square of every window above the absolute gate.
    windows: Vec<f64>,
    peak: f32,
}

impl LoudnessMeter {
    pub fn new(rate: u32) -> LoudnessMeter {
        LoudnessMeter {
            filters: k_weighting(rate),
            block_frames: ((f64::from(rate) * BLOCK_SECS) as usize).max(1),
            acc: 0.0,
            filled: 0,
            recent: [0.0; WINDOW_BLOCKS],
            blocks: 0,
            windows: Vec::new(),
            peak: 0.0,
        }
    }

    /// Measures interleaved stereo frames.
    pub fn push(&mut self, frames: &[f32]) {
        for f in frames.as_chunks::<2>().0 {
            self.peak = self.peak.max(f[0].abs()).max(f[1].abs());
            for (ch, s) in f.iter().enumerate() {
                let mut v = f64::from(*s);
                for stage in &mut self.filters {
                    v = stage.run(ch, v);
                }
                self.acc += v * v;
            }
            self.filled += 1;
            if self.filled == self.block_frames {
                self.end_block();
            }
        }
    }

    fn end_block(&mut self) {
        self.recent[self.blocks % WINDOW_BLOCKS] = self.acc / self.block_frames as f64;
        self.blocks += 1;
        self.acc = 0.0;
        self.filled = 0;
        if self.blocks >= WINDOW_BLOCKS {
            let window = self.recent.iter().sum::<f64>() / WINDOW_BLOCKS as f64;
            if lufs(window) > ABSOLUTE_GATE {
                self.windows.push(window);
            }
        }
    }

    /// Seconds measured so far.
    pub fn measured_secs(&self) -> f64 {
        self.blocks as f64 * BLOCK_SECS
    }

    /// The gated loudness so far, once a window has been measured (silence has none).
    pub fn integrated(&self) -> Option<f32> {
        if self.windows.is_empty() {
            return None;
        }
        let mean = |w: &mut dyn Iterator<Item = &f64>| {
            let (sum, n) = w.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
            (n > 0).then(|| sum / n as f64)
        };
        let ungated = mean(&mut self.windows.iter())?;
        let gate = lufs(ungated) + RELATIVE_GATE;
        let gated = mean(&mut self.windows.iter().filter(|w| lufs(**w) > gate))?;
        Some(lufs(gated) as f32)
    }

    /// The linear gain that brings what was measured to `TARGET_LUFS`, never lifting the
    /// loudest sample past full scale.
    pub fn gain(&self) -> Option<f32> {
        let loudness = self.integrated()?;
        let mut db = (TARGET_LUFS - loudness).clamp(-MAX_CUT_DB, MAX_BOOST_DB);
        if db > 0.0 && self.peak > 0.0 {
            let headroom = -20.0 * self.peak.log10() - PEAK_HEADROOM_DB;
            db = db.min(headroom.max(0.0));
        }
        Some(10f32.powf(db / 20.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_full_scale_sine_in_both_channels_reads_zero_lufs() {
        // BS.1770's reference: a 0 dBFS 997 Hz sine reads −3.01 LUFS in one channel, so 0.0
        // with the same signal in both.
        let mut m = LoudnessMeter::new(48_000);
        m.push(&sine(48_000, 997.0, 1.0, 3.0));
        let l = m.integrated().unwrap();
        assert!((l - 0.0).abs() < 0.3, "{l}");
    }

    #[test]
    fn levels_follow_amplitude_and_gain_targets_minus_fourteen() {
        let mut loud = LoudnessMeter::new(44_100);
        loud.push(&sine(44_100, 1000.0, 0.5, 2.0));
        let mut quiet = LoudnessMeter::new(44_100);
        quiet.push(&sine(44_100, 1000.0, 0.05, 2.0));
        let (l, q) = (loud.integrated().unwrap(), quiet.integrated().unwrap());
        assert!((l - q - 20.0).abs() < 0.2, "{l} {q}");
        // −6 LUFS-ish is cut to −14.
        let g = 20.0 * loud.gain().unwrap().log10();
        assert!((l + g - TARGET_LUFS).abs() < 0.2, "{l} {g}");
        // The quiet one (−26 LUFS) would need +12 dB; lifts stop at +9.
        let g = 20.0 * quiet.gain().unwrap().log10();
        assert!((g - MAX_BOOST_DB).abs() < 0.01, "{q} {g}");
        // A slightly quiet one gets all of it.
        let mut soft = LoudnessMeter::new(44_100);
        soft.push(&sine(44_100, 1000.0, 0.1, 2.0));
        let s = soft.integrated().unwrap();
        let g = 20.0 * soft.gain().unwrap().log10();
        assert!((s + g - TARGET_LUFS).abs() < 0.2, "{s} {g}");
    }

    #[test]
    fn a_lift_never_clips_and_silence_has_no_level() {
        // Quiet on average but with a full-scale click: no lift at all.
        let mut m = LoudnessMeter::new(48_000);
        let mut audio = sine(48_000, 1000.0, 0.02, 2.0);
        audio[1000] = 1.0;
        m.push(&audio);
        assert!(m.gain().unwrap() <= 1.0);
        let mut silent = LoudnessMeter::new(48_000);
        silent.push(&vec![0.0; 48_000 * 2]);
        assert!(silent.integrated().is_none());
        assert!(silent.measured_secs() > 0.9);
    }

    #[test]
    fn quiet_passages_dont_drag_the_level_down() {
        // A loud song with a long near-silent intro: the relative gate leaves the intro out.
        let mut with_intro = LoudnessMeter::new(48_000);
        with_intro.push(&sine(48_000, 1000.0, 0.003, 5.0));
        with_intro.push(&sine(48_000, 1000.0, 0.5, 5.0));
        let mut plain = LoudnessMeter::new(48_000);
        plain.push(&sine(48_000, 1000.0, 0.5, 5.0));
        let (a, b) = (
            with_intro.integrated().unwrap(),
            plain.integrated().unwrap(),
        );
        assert!((a - b).abs() < 0.3, "{a} {b}");
    }
}
