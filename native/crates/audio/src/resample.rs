use anyhow::{anyhow, Result};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

const CHUNK: usize = 1024;

/// Stereo sample-rate converter that reports the source time of every block it emits.
pub struct StereoResampler {
    inner: Option<Inner>,
    in_rate: u32,
}

struct Inner {
    fft: Fft<f32>,
    pending: Vec<f32>,
    pending_start: f64,
    scratch: Vec<f32>,
    skip: usize,
    in_total: u64,
    out_total: u64,
    ratio: f64,
}

impl StereoResampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Result<StereoResampler> {
        if in_rate == out_rate {
            return Ok(StereoResampler {
                inner: None,
                in_rate,
            });
        }
        let fft = Fft::<f32>::new(
            in_rate as usize,
            out_rate as usize,
            CHUNK,
            2,
            FixedSync::Input,
        )
        .map_err(|e| anyhow!("resampler: {e}"))?;
        let scratch = vec![0.0; fft.output_frames_max() * 2];
        let skip = fft.output_delay();
        Ok(StereoResampler {
            inner: Some(Inner {
                fft,
                pending: Vec::new(),
                pending_start: 0.0,
                scratch,
                skip,
                in_total: 0,
                out_total: 0,
                ratio: f64::from(out_rate) / f64::from(in_rate),
            }),
            in_rate,
        })
    }

    pub fn reset(&mut self) {
        if let Some(i) = &mut self.inner {
            i.fft.reset();
            i.pending.clear();
            i.skip = i.fft.output_delay();
            i.in_total = 0;
            i.out_total = 0;
        }
    }

    /// Feeds interleaved stereo starting at `start_secs`; `emit` gets each output block and its source time.
    pub fn process<E>(
        &mut self,
        input: &[f32],
        start_secs: f64,
        emit: &mut impl FnMut(&[f32], f64) -> Result<(), E>,
    ) -> Result<(), E> {
        let Some(i) = &mut self.inner else {
            return emit(input, start_secs);
        };
        if i.pending.is_empty() {
            i.pending_start = start_secs;
        }
        i.pending.extend_from_slice(input);
        i.in_total += (input.len() / 2) as u64;

        while i.pending.len() / 2 >= i.fft.input_frames_next() {
            let n = i.fft.input_frames_next();
            let (used, made) = {
                let input = InterleavedSlice::new(&i.pending[..n * 2], 2, n)
                    .map_err(|_| ())
                    .ok()
                    .and_then(|a| {
                        let cap = i.scratch.len() / 2;
                        let mut out = InterleavedSlice::new_mut(&mut i.scratch, 2, cap).ok()?;
                        i.fft.process_into_buffer(&a, &mut out, None).ok()
                    });
                input.unwrap_or((n, 0))
            };
            let at = i.pending_start;
            i.pending.drain(..used * 2);
            i.pending_start += used as f64 / f64::from(self.in_rate);
            let skip = i.skip.min(made);
            i.skip -= skip;
            if made > skip {
                i.out_total += (made - skip) as u64;
                emit(&i.scratch[skip * 2..made * 2], at)?;
            }
        }
        Ok(())
    }

    /// Flushes the filter tail so the output is exactly `ratio * input` frames long.
    pub fn finish<E>(
        &mut self,
        emit: &mut impl FnMut(&[f32], f64) -> Result<(), E>,
    ) -> Result<(), E> {
        let Some(i) = &mut self.inner else {
            return Ok(());
        };
        let expected = (i.in_total as f64 * i.ratio).floor() as u64;
        let mut partial = Some(i.pending.len() / 2);
        let mut guard = 0;
        while i.out_total < expected && guard < 8 {
            guard += 1;
            let n = i.fft.input_frames_next();
            let mut buf = std::mem::take(&mut i.pending);
            buf.resize(n * 2, 0.0);
            let have = partial.take().unwrap_or(0);
            let made = {
                let cap = i.scratch.len() / 2;
                let indexing = Indexing::new().partial_len(have);
                match (
                    InterleavedSlice::new(&buf, 2, n),
                    InterleavedSlice::new_mut(&mut i.scratch, 2, cap),
                ) {
                    (Ok(a), Ok(mut out)) => i
                        .fft
                        .process_into_buffer(&a, &mut out, Some(&indexing))
                        .map(|r| r.1)
                        .unwrap_or(0),
                    _ => 0,
                }
            };
            let skip = i.skip.min(made);
            i.skip -= skip;
            let take = (made - skip).min((expected - i.out_total) as usize);
            if take > 0 {
                i.out_total += take as u64;
                emit(&i.scratch[skip * 2..(skip + take) * 2], i.pending_start)?;
            }
        }
        i.pending.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v = (2.0 * std::f32::consts::PI * freq * n as f32 / rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    fn run(in_rate: u32, out_rate: u32, frames: usize) -> Vec<f32> {
        let mut r = StereoResampler::new(in_rate, out_rate).unwrap();
        let mut out = Vec::new();
        let input = sine(1000.0, in_rate, frames);
        for (k, chunk) in input.chunks(1152 * 2).enumerate() {
            r.process::<()>(
                chunk,
                (k * 1152) as f64 / f64::from(in_rate),
                &mut |b, _| {
                    out.extend_from_slice(b);
                    Ok(())
                },
            )
            .unwrap();
        }
        r.finish::<()>(&mut |b, _| {
            out.extend_from_slice(b);
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn output_length_matches_ratio() {
        let out = run(44100, 48000, 44100);
        let frames = out.len() / 2;
        assert!((frames as i64 - 48000).abs() <= 1, "got {frames}");
    }

    #[test]
    fn passthrough_when_rates_match() {
        let out = run(48000, 48000, 1000);
        assert_eq!(out.len(), 2000);
    }

    #[test]
    fn preserves_pitch_and_level() {
        let out = run(44100, 48000, 44100);
        let mid = &out[2 * 20000..2 * 28000];
        let rms = (mid.iter().map(|s| s * s).sum::<f32>() / mid.len() as f32).sqrt();
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.01, "rms {rms}");
        let crossings = mid
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let hz = crossings as f32 / (8000.0 / 48000.0);
        assert!((hz - 1000.0).abs() < 20.0, "hz {hz}");
    }
}
