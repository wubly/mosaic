//! A source fed by another thread (the Spotify sink): interleaved stereo f32 at a fixed rate.

use std::fmt;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};

use super::SourceCtx;
use crate::decode::{AudioStream, StreamError};

pub const RATE: u32 = 44_100;
/// The decoder only looks at commands between chunks, so an idle wait hands back an empty one.
const IDLE_WAIT: Duration = Duration::from_millis(40);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Roughly two seconds of audio in typical 4096-sample packets.
const QUEUE_PACKETS: usize = 24;

enum Msg {
    Samples(Vec<f32>),
    Eof,
    Error(String),
}

/// The writing half; cheap to clone.
#[derive(Clone)]
pub struct PushProducer {
    tx: Sender<Msg>,
}

impl PushProducer {
    /// Blocks while the queue is full (natural back-pressure); `false` if the deck went away or
    /// nothing drained for two seconds.
    pub fn write(&self, samples: Vec<f32>) -> bool {
        self.tx
            .send_timeout(Msg::Samples(samples), WRITE_TIMEOUT)
            .is_ok()
    }

    pub fn finish(&self) {
        let _ = self.tx.send_timeout(Msg::Eof, WRITE_TIMEOUT);
    }

    pub fn fail(&self, message: impl Into<String>) {
        let _ = self
            .tx
            .send_timeout(Msg::Error(message.into()), WRITE_TIMEOUT);
    }
}

pub struct PushSource {
    rx: Receiver<Msg>,
    on_seek: Arc<dyn Fn(f64) + Send + Sync>,
    start: f64,
    duration: Option<f64>,
}

/// What `SourceSpec::Push` carries: the source is taken by the decoder when the deck loads.
#[derive(Clone)]
pub struct PushHandle(Arc<Mutex<Option<PushSource>>>);

impl fmt::Debug for PushHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PushHandle")
    }
}

/// Creates a producer and the handle for `SourceSpec::Push`. `on_seek` is asked to reposition the
/// producer (librespot) whenever the deck seeks.
pub fn push_pair(
    start_at: f64,
    duration: Option<f64>,
    on_seek: impl Fn(f64) + Send + Sync + 'static,
) -> (PushProducer, PushHandle) {
    let (tx, rx) = bounded(QUEUE_PACKETS);
    let source = PushSource {
        rx,
        on_seek: Arc::new(on_seek),
        start: start_at,
        duration,
    };
    (
        PushProducer { tx },
        PushHandle(Arc::new(Mutex::new(Some(source)))),
    )
}

struct PushStream {
    source: PushSource,
    pos: f64,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

pub fn open(handle: &PushHandle, ctx: &SourceCtx) -> Result<Box<dyn AudioStream>, StreamError> {
    let source = handle
        .0
        .lock()
        .ok()
        .and_then(|mut s| s.take())
        .ok_or_else(|| StreamError::new("This stream was already opened"))?;
    Ok(Box::new(PushStream {
        pos: source.start,
        source,
        cancel: Arc::clone(&ctx.cancel),
    }))
}

impl AudioStream for PushStream {
    fn sample_rate(&self) -> u32 {
        RATE
    }

    fn duration(&self) -> Option<f64> {
        self.source.duration
    }

    fn seek(&mut self, secs: f64) -> Result<f64, StreamError> {
        // The initial position was already requested from the producer when it was created.
        if (secs - self.pos).abs() > 0.05 || self.pos != self.source.start {
            while self.source.rx.try_recv().is_ok() {}
            (self.source.on_seek)(secs);
        }
        self.pos = secs;
        Ok(secs)
    }

    fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<Option<f64>, StreamError> {
        out.clear();
        if self.cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        match self.source.rx.recv_timeout(IDLE_WAIT) {
            Ok(Msg::Samples(samples)) => {
                let start = self.pos;
                self.pos += samples.len() as f64 / 2.0 / f64::from(RATE);
                *out = samples;
                Ok(Some(start))
            }
            Ok(Msg::Eof) | Err(RecvTimeoutError::Disconnected) => Ok(None),
            Ok(Msg::Error(message)) => Err(StreamError::new(message)),
            Err(RecvTimeoutError::Timeout) => Ok(Some(self.pos)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    use super::*;

    fn ctx() -> SourceCtx {
        SourceCtx {
            duration_hint: None,
            out_rate: 48_000,
            buffering: Arc::new(|_| {}),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn chunks_carry_their_start_time_and_eof_ends_the_stream() {
        let (producer, handle) = push_pair(2.0, Some(30.0), |_| {});
        let mut stream = open(&handle, &ctx()).unwrap();
        assert_eq!(stream.duration(), Some(30.0));
        assert!(producer.write(vec![0.0; RATE as usize * 2]));
        producer.finish();
        let mut out = Vec::new();
        assert_eq!(stream.next_chunk(&mut out).unwrap(), Some(2.0));
        assert_eq!(out.len(), RATE as usize * 2);
        assert_eq!(stream.next_chunk(&mut out).unwrap(), None);
    }

    #[test]
    fn idle_waits_return_empty_chunks_so_the_decoder_can_poll() {
        let (_producer, handle) = push_pair(0.0, None, |_| {});
        let mut stream = open(&handle, &ctx()).unwrap();
        let mut out = vec![1.0];
        assert_eq!(stream.next_chunk(&mut out).unwrap(), Some(0.0));
        assert!(out.is_empty());
    }

    #[test]
    fn seeking_drops_queued_audio_and_asks_the_producer_except_for_the_initial_position() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let (producer, handle) = push_pair(5.0, None, move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        let mut stream = open(&handle, &ctx()).unwrap();
        stream.seek(5.0).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(producer.write(vec![0.0; 8]));
        stream.seek(60.0).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut out = Vec::new();
        assert_eq!(stream.next_chunk(&mut out).unwrap(), Some(60.0));
        assert!(out.is_empty(), "the queued packet was dropped");
    }

    #[test]
    fn errors_surface_and_a_second_open_fails() {
        let (producer, handle) = push_pair(0.0, None, |_| {});
        let mut stream = open(&handle, &ctx()).unwrap();
        assert!(open(&handle, &ctx()).is_err());
        producer.fail("This track isn't available on Spotify in your region");
        let mut out = Vec::new();
        let err = stream.next_chunk(&mut out).unwrap_err();
        assert!(err.message.contains("region"));
    }
}
