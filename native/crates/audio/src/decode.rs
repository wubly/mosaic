use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use rtrb::PushError;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::deck::{DeckShared, Marker, MarkerKind, Rings, NO_EOF};
use crate::resample::StereoResampler;
use crate::{DeckId, SourceSpec};

#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct StreamError(pub String);

impl StreamError {
    fn from_sym(context: &str, e: &SymError) -> StreamError {
        StreamError(format!("{context}: {e}"))
    }
}

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

        let track = format
            .default_track(TrackType::Audio)
            .or_else(|| format.first_track(TrackType::Audio))
            .ok_or_else(|| StreamError("No audio track found".into()))?;
        let Some(CodecParameters::Audio(params)) = &track.codec_params else {
            return Err(StreamError("No audio track found".into()));
        };
        let reg = symphonia::default::get_codecs()
            .get_audio_decoder(params.codec)
            .ok_or_else(|| StreamError("This format needs ffmpeg installed".into()))?;
        let decoder = (reg.factory)(params, &AudioDecoderOptions::default())
            .map_err(|e| StreamError::from_sym("Could not start decoder", &e))?;

        let rate = params
            .sample_rate
            .ok_or_else(|| StreamError("Unknown sample rate".into()))?;
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

impl AudioStream for SymphoniaStream {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn seek(&mut self, secs: f64) -> Result<f64, StreamError> {
        let target = secs.max(0.0);
        let time = Time::try_from_secs_f64(target)
            .ok_or_else(|| StreamError("Bad seek position".into()))?;
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

    let mut stream = match crate::source::open(&ctx.source, ctx.duration_hint) {
        Ok(s) => s,
        Err(e) => {
            report(e.0);
            wait_for_stop(&ctx);
            return rings;
        }
    };
    let mut resampler = match StereoResampler::new(stream.sample_rate(), ctx.out_rate) {
        Ok(r) => r,
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

    'outer: loop {
        if let Some((secs, epoch)) = pending_seek.take() {
            match stream.seek(secs) {
                Ok(reached) => {
                    resampler.reset();
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
                Err(e) => report(e.0),
            }
        }

        let result = loop {
            if let Err(i) = poll(&ctx) {
                break Outcome::from(i);
            }
            match stream.next_chunk(&mut chunk) {
                Ok(Some(start)) => {
                    let sent = resampler.process(&chunk, start, &mut |block, pos| {
                        push_block(&mut rings, &ctx, block, pos)
                    });
                    if let Err(i) = sent {
                        break Outcome::from(i);
                    }
                }
                Ok(None) => {
                    let sent = resampler
                        .finish(&mut |block, pos| push_block(&mut rings, &ctx, block, pos));
                    if let Err(i) = sent {
                        break Outcome::from(i);
                    }
                    ctx.shared.eof_frame.store(rings.pushed, Ordering::Release);
                    break idle(&ctx);
                }
                Err(e) => {
                    report(e.0);
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
