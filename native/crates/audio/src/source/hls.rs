use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use m3u8_rs::{parse_playlist_res, Playlist};
use symphonia::core::io::MediaSource;
use url::Url;

use super::{agent, SourceCtx, EXPIRED};
use crate::decode::{AudioStream, StreamError, SymphoniaStream};

/// How much audio the prefetcher keeps queued ahead of the decoder.
const PREFETCH_SECS: f64 = 20.0;

#[derive(Clone, Debug)]
struct Segment {
    url: String,
    start: f64,
    duration: f64,
}

#[derive(Debug)]
struct Parsed {
    segments: Vec<Segment>,
    init: Option<String>,
    total: f64,
}

fn parse(playlist: &[u8], base: &Url) -> Result<Parsed, String> {
    let media =
        match parse_playlist_res(playlist).map_err(|_| "Unreadable stream playlist".to_string())? {
            Playlist::MediaPlaylist(m) => m,
            Playlist::MasterPlaylist(_) => return Err("Unexpected master playlist".into()),
        };
    let resolve = |uri: &str| {
        base.join(uri)
            .map(|u| u.to_string())
            .map_err(|e| e.to_string())
    };

    let mut segments = Vec::new();
    let mut init = None;
    let mut at = 0.0;
    for seg in &media.segments {
        if init.is_none() {
            if let Some(map) = &seg.map {
                init = Some(resolve(&map.uri)?);
            }
        }
        let duration = f64::from(seg.duration);
        segments.push(Segment {
            url: resolve(&seg.uri)?,
            start: at,
            duration,
        });
        at += duration;
    }
    if segments.is_empty() {
        return Err("Empty stream playlist".into());
    }
    Ok(Parsed {
        segments,
        init,
        total: at,
    })
}

/// Index of the segment containing `secs`.
fn segment_at(segments: &[Segment], secs: f64) -> usize {
    segments.iter().rposition(|s| s.start <= secs).unwrap_or(0)
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("Could not reach the stream: {e}"))?;
    let status = resp.status().as_u16();
    if status == 403 || status == 410 {
        return Err(EXPIRED.into());
    }
    if status >= 400 {
        return Err(format!("Stream request failed ({status})"));
    }
    let mut bytes = Vec::new();
    resp.into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Stream interrupted: {e}"))?;
    Ok(bytes)
}

/// The concatenated bytes of the segments from one index on, as a forward-only stream.
struct SegmentReader {
    rx: Mutex<Receiver<Result<Vec<u8>, String>>>,
    current: Vec<u8>,
    offset: usize,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl SegmentReader {
    fn start(
        segments: Vec<Segment>,
        init: Option<Vec<u8>>,
        cancel: Arc<AtomicBool>,
    ) -> SegmentReader {
        let ahead =
            ((PREFETCH_SECS / segments.first().map_or(10.0, |s| s.duration.max(1.0))).ceil()
                as usize)
                .max(1);
        let (tx, rx) = sync_channel(ahead);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let agent = agent();
            if let Some(init) = init {
                if tx.send(Ok(init)).is_err() {
                    return;
                }
            }
            for seg in segments {
                if worker_stop.load(Ordering::Relaxed) || worker_cancel.load(Ordering::Relaxed) {
                    return;
                }
                let result = fetch(&agent, &seg.url);
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    return;
                }
            }
        });
        SegmentReader {
            rx: Mutex::new(rx),
            current: Vec::new(),
            offset: 0,
            stop,
            cancel,
        }
    }
}

impl Drop for SegmentReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Read for SegmentReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.offset >= self.current.len() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let received = self
                .rx
                .lock()
                .map_err(|_| io::Error::other("poisoned"))?
                .recv_timeout(Duration::from_millis(50));
            match received {
                Ok(Ok(bytes)) => {
                    self.current = bytes;
                    self.offset = 0;
                }
                Ok(Err(message)) => return Err(io::Error::other(message)),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
        }
        let n = out.len().min(self.current.len() - self.offset);
        out[..n].copy_from_slice(&self.current[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

impl Seek for SegmentReader {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stream is not seekable",
        ))
    }
}

impl MediaSource for SegmentReader {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

/// Plays an HLS media playlist; a seek restarts decoding at the segment that holds the target.
pub struct HlsStream {
    segments: Vec<Segment>,
    init: Option<Vec<u8>>,
    total: f64,
    inner: SymphoniaStream,
    base: f64,
    extension: Option<&'static str>,
    cancel: Arc<AtomicBool>,
    hint: Option<f64>,
}

fn guess_extension(first_segment: &str, has_init: bool) -> Option<&'static str> {
    let path = first_segment.split(['?', '#']).next().unwrap_or("");
    if has_init || path.ends_with(".m4s") || path.ends_with(".mp4") || path.ends_with(".m4a") {
        Some("mp4")
    } else if path.ends_with(".aac") {
        Some("aac")
    } else if path.ends_with(".ogg") || path.ends_with(".opus") {
        Some("ogg")
    } else {
        None
    }
}

fn build(
    segments: &[Segment],
    init: &Option<Vec<u8>>,
    extension: Option<&str>,
    hint: Option<f64>,
    cancel: &Arc<AtomicBool>,
    from: usize,
) -> Result<SymphoniaStream, StreamError> {
    let reader = SegmentReader::start(segments[from..].to_vec(), init.clone(), Arc::clone(cancel));
    SymphoniaStream::new(Box::new(reader), extension, hint, None)
}

pub fn open(playlist_url: &str, ctx: &SourceCtx) -> Result<Box<dyn AudioStream>, StreamError> {
    let base = Url::parse(playlist_url).map_err(|e| StreamError::new(e.to_string()))?;
    let agent = agent();
    let text = fetch(&agent, playlist_url).map_err(StreamError::new)?;
    let parsed = parse(&text, &base).map_err(StreamError::new)?;
    let init = parsed
        .init
        .as_deref()
        .map(|u| fetch(&agent, u))
        .transpose()
        .map_err(StreamError::new)?;
    let extension = guess_extension(&parsed.segments[0].url, init.is_some());

    let inner = build(
        &parsed.segments,
        &init,
        extension,
        ctx.duration_hint,
        &ctx.cancel,
        0,
    )?;
    Ok(Box::new(HlsStream {
        segments: parsed.segments,
        total: parsed.total,
        extension,
        cancel: Arc::clone(&ctx.cancel),
        hint: ctx.duration_hint,
        init,
        base: 0.0,
        inner,
    }))
}

impl AudioStream for HlsStream {
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn duration(&self) -> Option<f64> {
        Some(self.total)
    }

    fn seek(&mut self, secs: f64) -> Result<f64, StreamError> {
        let target = secs.clamp(0.0, self.total);
        let idx = segment_at(&self.segments, target);
        let mut inner = build(
            &self.segments,
            &self.init,
            self.extension,
            self.hint,
            &self.cancel,
            idx,
        )?;
        inner.skip_to(target - self.segments[idx].start);
        self.inner = inner;
        self.base = self.segments[idx].start;
        Ok(target)
    }

    fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<Option<f64>, StreamError> {
        Ok(self.inner.next_chunk(out)?.map(|t| t + self.base))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST: &str = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXTINF:9.98,\nhttps://cdn.example/a/0.mp3\n#EXTINF:10.0,\n1.mp3?sig=1\n#EXTINF:4.5,\n2.mp3\n#EXT-X-ENDLIST\n";

    #[test]
    fn parses_segments_and_resolves_urls() {
        let base = Url::parse("https://cdn.example/a/index.m3u8?x=1").unwrap();
        let p = parse(PLAYLIST.as_bytes(), &base).unwrap();
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[1].url, "https://cdn.example/a/1.mp3?sig=1");
        assert!((p.total - 24.48).abs() < 1e-3);
        assert!((p.segments[2].start - 19.98).abs() < 1e-3);
        assert!(p.init.is_none());
    }

    #[test]
    fn finds_segment_for_time() {
        let base = Url::parse("https://cdn.example/a/index.m3u8").unwrap();
        let p = parse(PLAYLIST.as_bytes(), &base).unwrap();
        assert_eq!(segment_at(&p.segments, 0.0), 0);
        assert_eq!(segment_at(&p.segments, 9.99), 1);
        assert_eq!(segment_at(&p.segments, 20.0), 2);
        assert_eq!(segment_at(&p.segments, 100.0), 2);
    }

    #[test]
    fn init_segment_and_extension() {
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:5,\nseg0.m4s\n#EXT-X-ENDLIST\n";
        let base = Url::parse("https://x.test/p/list.m3u8").unwrap();
        let p = parse(text.as_bytes(), &base).unwrap();
        assert_eq!(p.init.as_deref(), Some("https://x.test/p/init.mp4"));
        assert_eq!(guess_extension(&p.segments[0].url, true), Some("mp4"));
        assert_eq!(guess_extension("https://x/a.mp3?x=1", false), None);
    }

    #[test]
    fn rejects_garbage() {
        let base = Url::parse("https://x.test/").unwrap();
        assert!(parse(b"nope", &base).is_err());
    }
}
