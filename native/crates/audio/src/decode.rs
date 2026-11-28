use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use rtrb::PushError;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::common::Tier;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::deck::{DeckShared, Marker, MarkerKind, Rings, NO_EOF};
use crate::output::MixerParams;
use crate::resample::StereoResampler;
use crate::source::{SourceCtx, EXPIRED};
use crate::stretch::Stretcher;
use crate::{DeckId, SourceSpec};

#[derive(thiserror::Error, Debug)]
#[error("{message}")]
pub struct StreamError {
    pub message: String,
    /// The format or codec isn't one symphonia can handle, so a fallback may apply.
    pub unsupported: bool,
}

impl StreamError {
    pub fn new(message: impl Into<String>) -> StreamError {
        StreamError {
            message: message.into(),
            unsupported: false,
        }
    }

    fn from_sym(context: &str, e: &SymError) -> StreamError {
        if let SymError::IoError(io) = e {
            if io.to_string() == EXPIRED {
                return StreamError::new(EXPIRED);
            }
        }
        StreamError {
            message: format!("{context}: {e}"),
            unsupported: matches!(e, SymError::Unsupported(_)),
        }
    }
}

static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder_at_tier::<symphonia_opus::OpusDecoder>(Tier::Fallback);
    registry
});

/// Decoded audio as interleaved stereo f32 at the stream's own sample rate.
pub trait AudioStream: Send {
    fn sample_rate(&self) -> u32;
    fn duration(&self) -> Option<f64>;
    /// Seeks and returns the position actually reached.
    fn seek(&mut self, secs: f64) -> Result<f64, StreamError>;
    /// Fills `out` with the next chunk and returns its start time, or `None` at the end.
    fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<Option<f64>, StreamError>;
}

pub struct SymphoniaStream {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: u32,
    duration: Option<f64>,
    time_base: Option<symphonia::core::units::TimeBase>,
    pos: f64,
    skip_until: Option<f64>,
    scratch: Vec<f32>,
}

impl SymphoniaStream {
    pub fn new(
        source: Box<dyn MediaSource>,
        extension: Option<&str>,
        duration_hint: Option<f64>,
        after_probe: Option<Box<dyn FnOnce() + Send>>,
    ) -> Result<SymphoniaStream, StreamError> {
        let mut hint = Hint::new();
        if let Some(ext) = extension {
            hint.with_extension(ext);
        }
        let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| StreamError::from_sym("This format isn't supported", &e))?;
        if let Some(done) = after_probe {
            done();
        }

        let track = format
            .default_track(TrackType::Audio)
            .or_else(|| format.first_track(TrackType::Audio))
            .ok_or_else(|| StreamError::new("No audio track found"))?;
        let Some(CodecParameters::Audio(params)) = &track.codec_params else {
            return Err(StreamError::new("No audio track found"));
        };
        let reg = CODECS
            .get_audio_decoder(params.codec)
            .ok_or_else(|| StreamError {
                message: "This format needs ffmpeg installed".into(),
                unsupported: true,
            })?;
        let decoder = (reg.factory)(params, &AudioDecoderOptions::default())
            .map_err(|e| StreamError::from_sym("Could not start decoder", &e))?;

        let rate = params
            .sample_rate
            .ok_or_else(|| StreamError::new("Unknown sample rate"))?;
        let time_base = track.time_base;
        let from_frames = track.num_frames.map(|n| n as f64 / f64::from(rate));
        let from_duration = track
            .duration
            .zip(time_base)
            .and_then(|(d, tb)| tb.calc_duration(d))
            .map(|t| t.as_secs_f64());
        let duration = from_frames.or(from_duration).or(duration_hint);

        Ok(SymphoniaStream {
            track_id: track.id,
            format,
            decoder,
            rate,
            duration,
            time_base,
            pos: 0.0,
            skip_until: None,
            scratch: Vec::new(),
        })
    }
}

fn to_stereo(src: &[f32], channels: usize, out: &mut Vec<f32>) {
    match channels {
        0 => {}
        1 => out.extend(src.iter().flat_map(|s| [*s, *s])),
        2 => out.extend_from_slice(src),
        n => {
            for frame in src.chunks_exact(n) {
                let (mut l, mut r) = (frame[0], frame[1]);
                let rest: f32 = frame[2..].iter().sum::<f32>() * 0.5;
                l += rest;
                r += rest;
                out.push(l * 0.7);
                out.push(r * 0.7);
            }
        }
    }
}

impl SymphoniaStream {
    /// Drops decoded audio until `rel_secs` into the stream (used after restarting at a segment boundary).
    pub fn skip_to(&mut self, rel_secs: f64) {
        self.pos = 0.0;
        self.skip_until = (rel_secs > 0.0).then_some(rel_secs);
    }
}

impl AudioStream for SymphoniaStream {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn seek(&mut self, secs: f64) -> Result<f64, StreamError> {
        let target = secs.max(0.0);
        let time =
            Time::try_from_secs_f64(target).ok_or_else(|| StreamError::new("Bad seek position"))?;
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| StreamError::from_sym("Could not seek", &e))?;
        self.decoder.reset();
        let actual = self
            .time_base
            .and_then(|tb| tb.calc_time(seeked.actual_ts))
            .map_or(target, |t| t.as_secs_f64());
        self.pos = actual.min(target);
        self.skip_until = (actual < target).then_some(target);
        Ok(target)
    }

    fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<Option<f64>, StreamError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(None)
                }
                Err(e) => return Err(StreamError::from_sym("Playback failed", &e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(SymError::DecodeError(_)) => continue,
                Err(e) => return Err(StreamError::from_sym("Playback failed", &e)),
            };
            let channels = buf.spec().channels().count();
            self.scratch.clear();
            buf.copy_to_vec_interleaved::<f32>(&mut self.scratch);

            out.clear();
            to_stereo(&self.scratch, channels, out);
            let frames = out.len() / 2;
            let start = self.pos;
            self.pos += frames as f64 / f64::from(self.rate);

            if let Some(target) = self.skip_until {
                if self.pos <= target {
                    continue;
                }
                let drop = (((target - start) * f64::from(self.rate)).round().max(0.0) as usize)
                    .min(frames);
                out.drain(..drop * 2);
                self.skip_until = None;
                return Ok(Some(target));
            }
            if frames == 0 {
                continue;
            }
            return Ok(Some(start));
        }
    }
}

pub(crate) enum DecCmd {
    Seek { secs: f64, epoch: u32 },
}

pub(crate) enum Internal {
    Ready {
        deck: DeckId,
        load_id: u64,
        duration: Option<f64>,
    },
    Seeked {
        deck: DeckId,
        load_id: u64,
        epoch: u32,
    },
    Buffering {
        deck: DeckId,
        load_id: u64,
        on: bool,
    },
    Error {
        deck: DeckId,
        load_id: u64,
        message: String,
    },
}

pub(crate) struct DecoderCtx {
    pub deck: DeckId,
    pub load_id: u64,
    pub epoch: u32,
    pub source: SourceSpec,
    pub start_at: f64,
    pub duration_hint: Option<f64>,
    pub out_rate: u32,
    pub shared: Arc<DeckShared>,
    pub params: Arc<MixerParams>,
    pub events: Sender<Internal>,
    pub cmds: Receiver<DecCmd>,
    pub stop: Arc<AtomicBool>,
}

enum Interrupt {
    Stop,
    Cmd(DecCmd),
}

fn poll(ctx: &DecoderCtx) -> Result<(), Interrupt> {
    if ctx.stop.load(Ordering::Relaxed) {
        return Err(Interrupt::Stop);
    }
    match ctx.cmds.try_recv() {
        Ok(cmd) => Err(Interrupt::Cmd(cmd)),
        Err(_) => Ok(()),
    }
}

fn push_marker(rings: &mut Rings, ctx: &DecoderCtx, marker: Marker) -> Result<(), Interrupt> {
    loop {
        poll(ctx)?;
        match rings.markers.push(marker) {
            Ok(()) => return Ok(()),
            Err(PushError::Full(_)) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

/// Feeds the loudness meter and, every half second of audio, publishes its gain.
fn measure(rings: &mut Rings, ctx: &DecoderCtx, block: &[f32]) {
    let Some(meter) = rings.loudness.as_mut() else {
        return;
    };
    let before = meter.measured_secs();
    meter.push(block);
    let after = meter.measured_secs();
    // The first estimate after a second of audio; a first guess off a breath of intro would
    // swing the level.
    if after >= 1.0 && (before < 1.0 || (after * 2.0).floor() > (before * 2.0).floor()) {
        if let Some(g) = meter.gain() {
            ctx.shared.norm_gain.store(g.to_bits(), Ordering::Relaxed);
        }
    }
}

fn push_block(
    rings: &mut Rings,
    ctx: &DecoderCtx,
    block: &[f32],
    pos: f64,
) -> Result<(), Interrupt> {
    push_marker(
        rings,
        ctx,
        Marker {
            kind: MarkerKind::Pos,
            epoch: ctx.shared.flush_epoch.load(Ordering::Acquire),
            frame: rings.pushed,
            pos,
        },
    )?;
    measure(rings, ctx, block);
    let mut rest = block;
    while !rest.is_empty() {
        poll(ctx)?;
        let n = rings.samples.slots().min(rest.len()) & !1;
        if n == 0 {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        if let Ok(mut chunk) = rings.samples.write_chunk(n) {
            let (a, b) = chunk.as_mut_slices();
            let (head, tail) = rest[..n].split_at(a.len());
            a.copy_from_slice(head);
            b.copy_from_slice(tail);
            chunk.commit_all();
        }
        rings.pushed += (n / 2) as u64;
        rest = &rest[n..];
    }
    Ok(())
}

fn flush_marker(
    rings: &mut Rings,
    ctx: &DecoderCtx,
    epoch: u32,
    pos: f64,
) -> Result<(), Interrupt> {
    ctx.shared.eof_frame.store(NO_EOF, Ordering::Release);
    push_marker(
        rings,
        ctx,
        Marker {
            kind: MarkerKind::Flush,
            epoch,
            frame: rings.pushed,
            pos,
        },
    )
}

enum Outcome {
    Stopped,
    Seek { secs: f64, epoch: u32 },
}

impl From<Interrupt> for Outcome {
    fn from(i: Interrupt) -> Outcome {
        match i {
            Interrupt::Stop => Outcome::Stopped,
            Interrupt::Cmd(DecCmd::Seek { secs, epoch }) => Outcome::Seek { secs, epoch },
        }
    }
}

/// Resample, then time-stretch when the speed isn't 1, then queue for the mixer.
struct Pipeline {
    resampler: StereoResampler,
    stretcher: Stretcher,
    stretching: bool,
    tmp: Vec<f32>,
}

impl Pipeline {
    fn new(in_rate: u32, out_rate: u32) -> anyhow::Result<Pipeline> {
        Ok(Pipeline {
            resampler: StereoResampler::new(in_rate, out_rate)?,
            stretcher: Stretcher::new(out_rate),
            stretching: false,
            tmp: Vec::new(),
        })
    }

    fn reset(&mut self) {
        self.resampler.reset();
        self.stretcher.reset();
        self.stretching = false;
    }

    fn feed(
        &mut self,
        chunk: &[f32],
        start: f64,
        rings: &mut Rings,
        ctx: &DecoderCtx,
    ) -> Result<(), Interrupt> {
        let Pipeline {
            resampler,
            stretcher,
            stretching,
            tmp,
        } = self;
        resampler.process(chunk, start, &mut |block, pos| {
            emit(block, pos, stretcher, stretching, tmp, rings, ctx)
        })
    }

    fn finish(&mut self, rings: &mut Rings, ctx: &DecoderCtx, pos: f64) -> Result<(), Interrupt> {
        let Pipeline {
            resampler,
            stretcher,
            stretching,
            tmp,
        } = self;
        resampler
            .finish(&mut |block, pos| emit(block, pos, stretcher, stretching, tmp, rings, ctx))?;
        if *stretching {
            tmp.clear();
            stretcher.flush(tmp);
            push_block(rings, ctx, tmp, pos)?;
        }
        Ok(())
    }
}

fn emit(
    block: &[f32],
    pos: f64,
    stretcher: &mut Stretcher,
    stretching: &mut bool,
    tmp: &mut Vec<f32>,
    rings: &mut Rings,
    ctx: &DecoderCtx,
) -> Result<(), Interrupt> {
    let speed = f32::from_bits(ctx.params.speed.load(Ordering::Relaxed));
    let want = (speed - 1.0).abs() > 0.005;
    if want != *stretching {
        if *stretching {
            tmp.clear();
            stretcher.flush(tmp);
            push_block(rings, ctx, tmp, pos)?;
        }
        stretcher.reset();
        *stretching = want;
    }
    if !want {
        return push_block(rings, ctx, block, pos);
    }
    tmp.clear();
    stretcher.process(block, speed, tmp);
    if tmp.is_empty() {
        return Ok(());
    }
    push_block(rings, ctx, tmp, pos)
}

/// Opens a source on a helper thread so a stalled network can't block stopping.
fn open_stream(ctx: &DecoderCtx) -> Result<Box<dyn AudioStream>, Result<StreamError, Outcome>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let events = ctx.events.clone();
    let (deck, load_id) = (ctx.deck, ctx.load_id);
    let source_ctx = SourceCtx {
        duration_hint: ctx.duration_hint,
        out_rate: ctx.out_rate,
        buffering: Arc::new(move |on| {
            let _ = events.send(Internal::Buffering { deck, load_id, on });
        }),
        cancel: Arc::clone(&ctx.stop),
    };
    let spec = ctx.source.clone();
    let spawned = std::thread::Builder::new()
        .name("mosaic-open".into())
        .spawn(move || {
            let _ = tx.send(crate::source::open(&spec, &source_ctx));
        });
    if let Err(e) = spawned {
        return Err(Ok(StreamError::new(e.to_string())));
    }
    loop {
        if let Err(i) = poll(ctx) {
            return Err(Err(Outcome::from(i)));
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(result) => return result.map_err(Ok),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(Ok(StreamError::new("Could not open the stream")))
            }
        }
    }
}

/// Decodes until told to stop. Returns the rings so the next track can reuse them.
pub(crate) fn run(ctx: DecoderCtx, mut rings: Rings) -> Rings {
    let report = |message: String| {
        let _ = ctx.events.send(Internal::Error {
            deck: ctx.deck,
            load_id: ctx.load_id,
            message,
        });
    };

    if flush_marker(&mut rings, &ctx, ctx.epoch, ctx.start_at).is_err() {
        return rings;
    }
    // A new track: measure it from scratch. Until enough is measured it plays as it is.
    rings.loudness = Some(crate::dsp::loudness::LoudnessMeter::new(ctx.out_rate));
    ctx.shared
        .norm_gain
        .store(1.0f32.to_bits(), Ordering::Relaxed);

    let mut stream = match open_stream(&ctx) {
        Ok(s) => s,
        Err(Ok(e)) => {
            report(e.message);
            wait_for_stop(&ctx);
            return rings;
        }
        Err(Err(_)) => return rings,
    };
    let mut pipeline = match Pipeline::new(stream.sample_rate(), ctx.out_rate) {
        Ok(p) => p,
        Err(e) => {
            report(e.to_string());
            wait_for_stop(&ctx);
            return rings;
        }
    };
    let _ = ctx.events.send(Internal::Ready {
        deck: ctx.deck,
        load_id: ctx.load_id,
        duration: stream.duration(),
    });

    let mut pending_seek = (ctx.start_at > 0.001).then_some((ctx.start_at, None));
    let mut chunk = Vec::new();
    let mut last_pos = ctx.start_at;

    'outer: loop {
        if let Some((secs, epoch)) = pending_seek.take() {
            match stream.seek(secs) {
                Ok(reached) => {
                    pipeline.reset();
                    last_pos = reached;
                    if let Some(epoch) = epoch {
                        if flush_marker(&mut rings, &ctx, epoch, reached).is_err() {
                            return rings;
                        }
                        let _ = ctx.events.send(Internal::Seeked {
                            deck: ctx.deck,
                            load_id: ctx.load_id,
                            epoch,
                        });
                    }
                }
                Err(e) => report(e.message),
            }
        }

        let result = loop {
            if let Err(i) = poll(&ctx) {
                break Outcome::from(i);
            }
            match stream.next_chunk(&mut chunk) {
                Ok(Some(start)) => {
                    last_pos = start;
                    if let Err(i) = pipeline.feed(&chunk, start, &mut rings, &ctx) {
                        break Outcome::from(i);
                    }
                }
                Ok(None) => {
                    if let Err(i) = pipeline.finish(&mut rings, &ctx, last_pos) {
                        break Outcome::from(i);
                    }
                    ctx.shared.eof_frame.store(rings.pushed, Ordering::Release);
                    break idle(&ctx);
                }
                Err(e) => {
                    if ctx.stop.load(Ordering::Relaxed) {
                        break Outcome::Stopped;
                    }
                    report(e.message);
                    break idle(&ctx);
                }
            }
        };

        match result {
            Outcome::Stopped => break 'outer,
            Outcome::Seek { secs, epoch } => pending_seek = Some((secs, Some(epoch))),
        }
    }
    rings
}

/// Waits (after the end of a stream or an error) for a seek or stop.
fn idle(ctx: &DecoderCtx) -> Outcome {
    loop {
        if ctx.stop.load(Ordering::Relaxed) {
            return Outcome::Stopped;
        }
        if let Ok(DecCmd::Seek { secs, epoch }) = ctx.cmds.recv_timeout(Duration::from_millis(20)) {
            return Outcome::Seek { secs, epoch };
        }
    }
}

fn wait_for_stop(ctx: &DecoderCtx) {
    while !ctx.stop.load(Ordering::Relaxed) {
        let _ = ctx.cmds.recv_timeout(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_conversion() {
        let mut out = Vec::new();
        to_stereo(&[0.1, 0.2], 1, &mut out);
        assert_eq!(out, [0.1, 0.1, 0.2, 0.2]);
        out.clear();
        to_stereo(&[0.1, 0.2], 2, &mut out);
        assert_eq!(out, [0.1, 0.2]);
        out.clear();
        to_stereo(&[1.0, 1.0, 0.0, 0.0, 0.0, 0.0], 6, &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.7).abs() < 1e-6);
    }
}
