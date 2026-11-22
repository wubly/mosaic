const THRESHOLD_DB: f32 = -24.0;
const RATIO: f32 = 4.0;
const ATTACK_SECS: f32 = 0.010;
const RELEASE_SECS: f32 = 0.250;
const BYPASS_SECS: f32 = 0.020;

/// Hard-knee feed-forward compressor (the "volume leveling" stage). No makeup gain.
pub struct Compressor {
    attack: f32,
    release: f32,
    bypass: f32,
    reduction_db: f32,
}

fn coeff(secs: f32, rate: u32) -> f32 {
    1.0 - (-1.0 / (secs * rate as f32)).exp()
}

impl Compressor {
    pub fn new(rate: u32) -> Compressor {
        let mut c = Compressor {
            attack: 0.0,
            release: 0.0,
            bypass: 0.0,
            reduction_db: 0.0,
        };
        c.set_rate(rate);
        c
    }

    pub fn set_rate(&mut self, rate: u32) {
        self.attack = coeff(ATTACK_SECS, rate);
        self.release = coeff(RELEASE_SECS, rate);
        self.bypass = coeff(BYPASS_SECS, rate);
    }

    pub fn process(&mut self, buf: &mut [f32], enabled: bool) {
        if !enabled && self.reduction_db > -0.01 {
            self.reduction_db = 0.0;
            return;
        }
        for frame in buf.as_chunks_mut::<2>().0.iter_mut() {
            let (target, smoothing) = if enabled {
                let level = frame[0].abs().max(frame[1].abs()).max(1e-9);
                let over = 20.0 * level.log10() - THRESHOLD_DB;
                let target = if over > 0.0 {
                    -over * (1.0 - 1.0 / RATIO)
                } else {
                    0.0
                };
                (
                    target,
                    if target < self.reduction_db {
                        self.attack
                    } else {
                        self.release
                    },
                )
            } else {
                (0.0, self.bypass)
            };
            self.reduction_db += (target - self.reduction_db) * smoothing;
            let gain = 10f32.powf(self.reduction_db / 20.0);
            frame[0] *= gain;
            frame[1] *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin() * amp;
                [v, v]
            })
            .collect()
    }

    fn tail_peak(buf: &[f32]) -> f32 {
        buf[buf.len() - 9600..]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn loud_signal_is_reduced_toward_ratio() {
        let mut c = Compressor::new(48000);
        let mut buf = sine(0.5, 48000 * 3);
        for block in buf.chunks_mut(960) {
            c.process(block, true);
        }
        let reduction = 20.0 * (tail_peak(&buf) / 0.5).log10();
        assert!(reduction < -9.0 && reduction > -15.0, "{reduction}");
    }

    #[test]
    fn quiet_signal_passes_and_disabled_bypasses() {
        let mut c = Compressor::new(48000);
        let mut quiet = sine(0.02, 48000);
        c.process(&mut quiet, true);
        assert!((tail_peak(&quiet) - 0.02).abs() < 0.001);
        let mut loud = sine(0.5, 4800);
        let copy = loud.clone();
        let mut off = Compressor::new(48000);
        off.process(&mut loud, false);
        assert_eq!(loud, copy);
    }
}
