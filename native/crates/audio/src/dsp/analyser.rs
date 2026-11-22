use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const FFT_SIZE: usize = 512;
pub const BINS: usize = FFT_SIZE / 2;
const SMOOTHING: f32 = 0.82;
const MIN_DB: f32 = -100.0;
const MAX_DB: f32 = -30.0;

struct Shared {
    samples: [AtomicU32; FFT_SIZE],
    write: AtomicUsize,
}

/// The mixer's end of the analyser: the last 512 mono samples, written without locking.
pub struct AnalyserTap {
    shared: Arc<Shared>,
}

impl AnalyserTap {
    pub fn push(&self, stereo: &[f32]) {
        let mut w = self.shared.write.load(Ordering::Relaxed);
        for frame in stereo.as_chunks::<2>().0 {
            self.shared.samples[w % FFT_SIZE]
                .store(((frame[0] + frame[1]) * 0.5).to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.shared.write.store(w, Ordering::Release);
    }
}

/// Reproduces `AnalyserNode.getByteFrequencyData` (fft 512, Blackman, smoothing 0.82, -100..-30 dB).
pub struct Analyser {
    shared: Arc<Shared>,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    smoothed: [f32; BINS],
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

pub fn analyser_pair() -> (AnalyserTap, Analyser) {
    let shared = Arc::new(Shared {
        samples: std::array::from_fn(|_| AtomicU32::new(0)),
        write: AtomicUsize::new(0),
    });
    (
        AnalyserTap {
            shared: Arc::clone(&shared),
        },
        Analyser::from_shared(shared),
    )
}

impl Analyser {
    fn from_shared(shared: Arc<Shared>) -> Analyser {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|n| {
                let x = n as f32 / FFT_SIZE as f32;
                0.42 - 0.5 * (2.0 * std::f32::consts::PI * x).cos()
                    + 0.08 * (4.0 * std::f32::consts::PI * x).cos()
            })
            .collect();
        Analyser {
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            shared,
            window,
            smoothed: [0.0; BINS],
        }
    }

    pub fn byte_frequency_data(&mut self, out: &mut [u8; BINS]) {
        let end = self.shared.write.load(Ordering::Acquire);
        for n in 0..FFT_SIZE {
            let idx = end.wrapping_sub(FFT_SIZE).wrapping_add(n) % FFT_SIZE;
            self.input[n] =
                f32::from_bits(self.shared.samples[idx].load(Ordering::Relaxed)) * self.window[n];
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            return;
        }
        for ((bin, smoothed), slot) in self
            .spectrum
            .iter()
            .zip(self.smoothed.iter_mut())
            .zip(out.iter_mut())
            .take(BINS)
        {
            let magnitude = bin.norm() / FFT_SIZE as f32;
            *smoothed = SMOOTHING * *smoothed + (1.0 - SMOOTHING) * magnitude;
            let db = 20.0 * smoothed.max(1e-12).log10();
            *slot = (((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
}

impl Clone for Analyser {
    /// A clone reads the same audio but keeps its own smoothing state.
    fn clone(&self) -> Analyser {
        Analyser::from_shared(Arc::clone(&self.shared))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_peaks_in_the_right_bin() {
        let (tap, mut analyser) = analyser_pair();
        let mut out = [0u8; BINS];
        let mut n = 0usize;
        for _ in 0..40 {
            let block: Vec<f32> = (0..FFT_SIZE)
                .flat_map(|_| {
                    let v = (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / 48000.0).sin() * 0.02;
                    n += 1;
                    [v, v]
                })
                .collect();
            tap.push(&block);
            analyser.byte_frequency_data(&mut out);
        }
        let peak = out.iter().enumerate().max_by_key(|(_, v)| **v).unwrap().0;
        assert_eq!(peak, 11);
        assert!(out[11] > 100);
        assert!(out[100] < 20);
    }

    #[test]
    fn silence_is_zero() {
        let (_tap, mut analyser) = analyser_pair();
        let mut out = [1u8; BINS];
        analyser.byte_frequency_data(&mut out);
        assert!(out.iter().all(|b| *b == 0));
    }
}
