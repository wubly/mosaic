use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use rtrb::Producer;

use crate::deck::{CbEvent, DeckPlayer};

const MAX_FRAMES: usize = 4096;
const VOLUME_SMOOTHING_SECS: f32 = 0.010;

/// Values the control thread changes while the callback runs.
pub struct MixerParams {
    pub volume: AtomicU32,
    pub speed: AtomicU32,
    pub rendered: AtomicU64,
}

impl MixerParams {
    pub fn new() -> MixerParams {
        MixerParams {
            volume: AtomicU32::new(1.0f32.to_bits()),
            speed: AtomicU32::new(1.0f32.to_bits()),
            rendered: AtomicU64::new(0),
        }
    }

    pub fn set_volume(&self, v: f32) {
        self.volume
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Relaxed))
    }
}

pub struct Mixer {
    rate: u32,
    decks: [DeckPlayer; 2],
    events: Producer<CbEvent>,
    params: Arc<MixerParams>,
    master: f32,
    blocking: bool,
}

impl Mixer {
    pub fn new(
        rate: u32,
        decks: [DeckPlayer; 2],
        events: Producer<CbEvent>,
        params: Arc<MixerParams>,
        blocking: bool,
    ) -> Mixer {
        let master = params.volume();
        Mixer {
            rate,
            decks,
            events,
            params,
            master,
            blocking,
        }
    }

    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate;
    }

    /// Renders `out.len() / 2` stereo frames.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let speed = self.params.speed();
        for deck in &mut self.decks {
            deck.mix(out, self.rate, speed, self.blocking, &mut self.events);
        }

        let target = self.params.volume();
        let coeff = 1.0 - (-1.0 / (VOLUME_SMOOTHING_SECS * self.rate as f32)).exp();
        for frame in out.as_chunks_mut::<2>().0.iter_mut() {
            self.master += (target - self.master) * coeff;
            frame[0] = (frame[0] * self.master).clamp(-1.0, 1.0);
            frame[1] = (frame[1] * self.master).clamp(-1.0, 1.0);
        }
        self.params
            .rendered
            .fetch_add((out.len() / 2) as u64, Ordering::Relaxed);
    }
}

/// Pulls the mixer without a device, for tests. Waits for decoders instead of underrunning.
pub struct OfflineRenderer {
    mixer: Arc<Mutex<Mixer>>,
}

impl OfflineRenderer {
    pub(crate) fn new(mixer: Arc<Mutex<Mixer>>) -> OfflineRenderer {
        OfflineRenderer { mixer }
    }

    /// Fills `out` with interleaved stereo frames.
    pub fn render(&mut self, out: &mut [f32]) {
        if let Ok(mut mixer) = self.mixer.lock() {
            for block in out.chunks_mut(960) {
                mixer.render(block);
            }
        }
    }
}

pub struct DeviceStream {
    _stream: Stream,
    pub rate: u32,
}

/// Opens the default output device and starts a stream pulling from `mixer`.
pub fn open_device(mixer: &Arc<Mutex<Mixer>>, failed: &Arc<AtomicBool>) -> Result<DeviceStream> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("No audio output device found"))?;
    let supported = device
        .default_output_config()
        .map_err(|e| anyhow!("No audio output device found ({e})"))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let rate = config.sample_rate;

    if let Ok(mut m) = mixer.lock() {
        m.set_rate(rate);
    }

    let mixer = Arc::clone(mixer);
    let failed = Arc::clone(failed);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, mixer, failed)?,
        SampleFormat::I16 => build::<i16>(&device, config, mixer, failed)?,
        SampleFormat::U16 => build::<u16>(&device, config, mixer, failed)?,
        SampleFormat::I32 => build::<i32>(&device, config, mixer, failed)?,
        other => bail!("Unsupported audio format {other}"),
    };
    stream
        .play()
        .map_err(|e| anyhow!("Could not start audio: {e}"))?;
    Ok(DeviceStream {
        _stream: stream,
        rate,
    })
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
    failed: Arc<AtomicBool>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut scratch = vec![0f32; MAX_FRAMES * 2];
    let stream = device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                let Ok(mut mixer) = mixer.try_lock() else {
                    data.fill(T::EQUILIBRIUM);
                    return;
                };
                for block in data.chunks_mut(MAX_FRAMES * channels) {
                    let frames = block.len() / channels;
                    mixer.render(&mut scratch[..frames * 2]);
                    for (out, src) in block
                        .chunks_exact_mut(channels)
                        .zip(scratch.as_chunks::<2>().0.iter())
                    {
                        match channels {
                            1 => out[0] = T::from_sample((src[0] + src[1]) * 0.5),
                            _ => {
                                out[0] = T::from_sample(src[0]);
                                out[1] = T::from_sample(src[1]);
                                out[2..].fill(T::EQUILIBRIUM);
                            }
                        }
                    }
                }
            },
            move |_err| failed.store(true, Ordering::Relaxed),
            None,
        )
        .map_err(|e| anyhow!("Could not open audio output: {e}"))?;
    Ok(stream)
}
