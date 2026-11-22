use std::f64::consts::PI;

const FREQS: [f64; 5] = [60.0, 230.0, 910.0, 3600.0, 14000.0];
const SMOOTHING_SECS: f32 = 0.020;

#[derive(Clone, Copy)]
enum Kind {
    LowShelf,
    Peaking,
    HighShelf,
}

const KINDS: [Kind; 5] = [
    Kind::LowShelf,
    Kind::Peaking,
    Kind::Peaking,
    Kind::Peaking,
    Kind::HighShelf,
];

/// Normalised biquad coefficients `[b0, b1, b2, a1, a2]` from the RBJ cookbook, as WebAudio defines them (Q = 1, shelf slope 1).
fn coefficients(kind: Kind, freq: f64, rate: f64, gain_db: f64) -> [f64; 5] {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * freq.min(rate * 0.49) / rate;
    let (sin, cos) = w0.sin_cos();
    let (b0, b1, b2, a0, a1, a2);
    match kind {
        Kind::Peaking => {
            let alpha = sin / 2.0;
            b0 = 1.0 + alpha * a;
            b1 = -2.0 * cos;
            b2 = 1.0 - alpha * a;
            a0 = 1.0 + alpha / a;
            a1 = -2.0 * cos;
            a2 = 1.0 - alpha / a;
        }
        Kind::LowShelf => {
            let alpha = sin / 2.0 * 2f64.sqrt();
            let k = 2.0 * a.sqrt() * alpha;
            b0 = a * ((a + 1.0) - (a - 1.0) * cos + k);
            b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos);
            b2 = a * ((a + 1.0) - (a - 1.0) * cos - k);
            a0 = (a + 1.0) + (a - 1.0) * cos + k;
            a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos);
            a2 = (a + 1.0) + (a - 1.0) * cos - k;
        }
        Kind::HighShelf => {
            let alpha = sin / 2.0 * 2f64.sqrt();
            let k = 2.0 * a.sqrt() * alpha;
            b0 = a * ((a + 1.0) + (a - 1.0) * cos + k);
            b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos);
            b2 = a * ((a + 1.0) + (a - 1.0) * cos - k);
            a0 = (a + 1.0) - (a - 1.0) * cos + k;
            a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos);
            a2 = (a + 1.0) - (a - 1.0) * cos - k;
        }
    }
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    c: [f64; 5],
    z1: [f64; 2],
    z2: [f64; 2],
}

impl Biquad {
    fn run(&mut self, ch: usize, x: f64) -> f64 {
        let [b0, b1, b2, a1, a2] = self.c;
        let y = b0 * x + self.z1[ch];
        self.z1[ch] = b1 * x - a1 * y + self.z2[ch];
        self.z2[ch] = b2 * x - a2 * y;
        y
    }
}

/// Five-band equaliser with gains eased over 20 ms to avoid zipper noise.
pub struct Eq {
    rate: u32,
    current: [f32; 5],
    target: [f32; 5],
    filters: [Biquad; 5],
    active: bool,
}

impl Eq {
    pub fn new(rate: u32) -> Eq {
        Eq {
            rate,
            current: [0.0; 5],
            target: [0.0; 5],
            filters: [Biquad::default(); 5],
            active: false,
        }
    }

    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate;
        self.filters = [Biquad::default(); 5];
        self.update_all();
    }

    pub fn set_target(&mut self, gains_db: [f32; 5]) {
        self.target = gains_db;
    }

    fn update_all(&mut self) {
        for i in 0..5 {
            self.filters[i].c = coefficients(
                KINDS[i],
                FREQS[i],
                f64::from(self.rate),
                f64::from(self.current[i]),
            );
        }
    }

    /// Filters interleaved stereo in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        let settled = self.current == self.target;
        if settled && self.current == [0.0; 5] {
            self.active = false;
            return;
        }
        if !self.active {
            self.filters = [Biquad::default(); 5];
            self.update_all();
            self.active = true;
        }
        if !settled {
            let k = 1.0 - (-(buf.len() as f32 / 2.0) / (self.rate as f32 * SMOOTHING_SECS)).exp();
            for i in 0..5 {
                let diff = self.target[i] - self.current[i];
                self.current[i] = if diff.abs() < 0.01 {
                    self.target[i]
                } else {
                    self.current[i] + diff * k
                };
                self.filters[i].c = coefficients(
                    KINDS[i],
                    FREQS[i],
                    f64::from(self.rate),
                    f64::from(self.current[i]),
                );
            }
        }
        for frame in buf.as_chunks_mut::<2>().0.iter_mut() {
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = f64::from(*s);
                for f in &mut self.filters {
                    x = f.run(ch, x);
                }
                *s = x as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_db(eq_gains: [f32; 5], freq: f32) -> f32 {
        let rate = 48_000;
        let mut eq = Eq::new(rate);
        eq.set_target(eq_gains);
        let n = rate as usize;
        let mut buf: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.1;
                [v, v]
            })
            .collect();
        for block in buf.chunks_mut(960) {
            eq.process(block);
        }
        let tail = &buf[n..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * (rms / (0.1 / 2f32.sqrt())).log10()
    }

    #[test]
    fn peaking_band_boosts_its_centre() {
        let g = gain_db([0.0, 0.0, 6.0, 0.0, 0.0], 910.0);
        assert!((g - 6.0).abs() < 0.3, "{g}");
        assert!(gain_db([0.0, 0.0, 6.0, 0.0, 0.0], 60.0).abs() < 0.3);
    }

    #[test]
    fn shelves_reach_full_gain_far_from_corner() {
        assert!((gain_db([6.0, 0.0, 0.0, 0.0, 0.0], 20.0) - 6.0).abs() < 0.5);
        assert!((gain_db([0.0, 0.0, 0.0, 0.0, -6.0], 20000.0) + 6.0).abs() < 0.8);
    }

    #[test]
    fn flat_is_bypassed() {
        let mut eq = Eq::new(48000);
        let mut buf = vec![0.5; 200];
        eq.process(&mut buf);
        assert!(buf.iter().all(|s| *s == 0.5));
    }
}
