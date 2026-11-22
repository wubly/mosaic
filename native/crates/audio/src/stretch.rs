use signalsmith_stretch::Stretch;

/// Pitch-preserving time stretch for stereo audio. `speed` > 1 plays faster.
pub struct Stretcher {
    inner: Stretch,
    rate: u32,
    in_total: u64,
    out_total: u64,
    skip: usize,
}

impl Stretcher {
    pub fn new(rate: u32) -> Stretcher {
        let inner = Stretch::preset_default(2, rate);
        let skip = inner.output_latency() + inner.input_latency();
        Stretcher {
            inner,
            rate,
            in_total: 0,
            out_total: 0,
            skip,
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.in_total = 0;
        self.out_total = 0;
        self.skip = self.inner.output_latency() + self.inner.input_latency();
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Stretches interleaved stereo `input`, appending the result to `out`.
    pub fn process(&mut self, input: &[f32], speed: f32, out: &mut Vec<f32>) {
        let frames = input.len() / 2;
        self.in_total += frames as u64;
        let wanted = (self.in_total as f64 / f64::from(speed)).round() as u64;
        let n = wanted.saturating_sub(self.out_total) as usize;
        self.out_total += n as u64;
        if n == 0 {
            self.inner.process(input, &mut []);
            return;
        }
        let start = out.len();
        out.resize(start + n * 2, 0.0);
        self.inner.process(input, &mut out[start..]);
        self.trim_startup(out, start);
    }

    /// Emits the audio still held inside the stretcher.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let n = self.inner.output_latency();
        let start = out.len();
        out.resize(start + n * 2, 0.0);
        self.inner.flush(&mut out[start..]);
        self.trim_startup(out, start);
    }

    fn trim_startup(&mut self, out: &mut Vec<f32>, from: usize) {
        if self.skip == 0 {
            return;
        }
        let have = (out.len() - from) / 2;
        let cut = self.skip.min(have);
        out.drain(from..from + cut * 2);
        self.skip -= cut;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(speed: f32, secs: usize) -> Vec<f32> {
        let rate = 48_000;
        let mut s = Stretcher::new(rate);
        let mut out = Vec::new();
        let total = rate as usize * secs;
        for start in (0..total).step_by(1024) {
            let block: Vec<f32> = (start..(start + 1024).min(total))
                .flat_map(|i| {
                    let v =
                        (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5;
                    [v, v]
                })
                .collect();
            s.process(&block, speed, &mut out);
        }
        s.flush(&mut out);
        out
    }

    #[test]
    fn output_length_follows_speed() {
        let out = run(1.5, 6);
        let secs = out.len() as f32 / 2.0 / 48_000.0;
        assert!((secs - 4.0).abs() < 0.2, "{secs}");
        let slow = run(0.5, 2);
        let secs = slow.len() as f32 / 2.0 / 48_000.0;
        assert!((secs - 4.0).abs() < 0.2, "{secs}");
    }

    #[test]
    fn pitch_is_preserved() {
        let out = run(1.5, 4);
        let mid: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| f[0])
            .skip(48_000)
            .take(48_000)
            .collect();
        let crossings = mid.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((crossings as f32 - 440.0).abs() < 8.0, "{crossings}");
        let lead = out
            .as_chunks::<2>()
            .0
            .iter()
            .take_while(|f| f[0].abs() < 0.01)
            .count();
        assert!(lead < 2400, "leading silence {lead}");
    }
}
