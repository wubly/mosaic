use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use symphonia::core::io::MediaSource;

use super::{agent, SourceCtx};
use crate::decode::{AudioStream, StreamError, SymphoniaStream};

const CHUNK: usize = 32 * 1024;
const AHEAD_LIMIT: u64 = 24 * 1024 * 1024;
const BEHIND_LIMIT: u64 = 8 * 1024 * 1024;
const NEAR_GAP: u64 = 256 * 1024;
const STALL_AFTER: Duration = Duration::from_millis(150);

struct State {
    start: u64,
    data: Vec<u8>,
    done: bool,
    error: Option<String>,
    generation: u64,
    want_from: Option<u64>,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    stop: AtomicBool,
    read_pos: AtomicU64,
}

/// A seekable view of an HTTP resource, filled by a background download that restarts with a Range request on far seeks.
pub struct HttpSource {
    shared: Arc<Shared>,
    pos: u64,
    len: Option<u64>,
    hide_len: Arc<AtomicBool>,
    buffering: Arc<dyn Fn(bool) + Send + Sync>,
    cancel: Arc<AtomicBool>,
}

struct Head {
    total: Option<u64>,
}

fn request(
    agent: &ureq::Agent,
    url: &str,
    from: u64,
) -> Result<(Head, Box<dyn Read + Send>), String> {
    // Always ranged, even from 0: YouTube serves an unranged GET at a throttled crawl, slower
    // than the audio plays. Servers that ignore Range answer 200 and are handled below.
    let resp = agent
        .get(url)
        .header("Range", format!("bytes={from}-"))
        .call()
        .map_err(|e| format!("Could not reach the stream: {e}"))?;
    let status = resp.status().as_u16();
    if status == 403 || status == 410 {
        return Err(super::EXPIRED.into());
    }
    if status >= 400 {
        return Err(format!("Stream request failed ({status})"));
    }
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let total = if status == 206 {
        header("content-range").and_then(|v| v.rsplit('/').next().and_then(|t| t.parse().ok()))
    } else {
        header("content-length").and_then(|v| v.parse().ok())
    };
    let mut reader: Box<dyn Read + Send> = Box::new(resp.into_body().into_reader());
    if status == 200 && from > 0 {
        io::copy(&mut reader.by_ref().take(from), &mut io::sink()).map_err(|e| e.to_string())?;
    }
    Ok((Head { total }, reader))
}

fn download(shared: Arc<Shared>, url: String, first: Result<(Head, Box<dyn Read + Send>), String>) {
    let agent = agent();
    let mut next = Some(first);
    loop {
        let (generation, from) = {
            let Ok(mut st) = shared.state.lock() else {
                return;
            };
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(from) = st.want_from.take() {
                    break (st.generation, from);
                }
                st = match shared.cv.wait_timeout(st, Duration::from_millis(100)) {
                    Ok((s, _)) => s,
                    Err(_) => return,
                };
            }
        };
        let response = next.take().unwrap_or_else(|| request(&agent, &url, from));
        let mut reader = match response {
            Ok((_, r)) => r,
            Err(e) => {
                if let Ok(mut st) = shared.state.lock() {
                    if st.generation == generation {
                        st.error = Some(e);
                    }
                }
                shared.cv.notify_all();
                continue;
            }
        };
        let mut buf = vec![0u8; CHUNK];
        loop {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            {
                let Ok(st) = shared.state.lock() else { return };
                if st.generation != generation {
                    break;
                }
                let ahead = (st.start + st.data.len() as u64)
                    .saturating_sub(shared.read_pos.load(Ordering::Relaxed));
                if ahead > AHEAD_LIMIT {
                    let _ = shared.cv.wait_timeout(st, Duration::from_millis(50));
                    continue;
                }
            }
            let n = match reader.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    if let Ok(mut st) = shared.state.lock() {
                        if st.generation == generation {
                            st.error = Some(format!("Stream interrupted: {e}"));
                        }
                    }
                    shared.cv.notify_all();
                    break;
                }
            };
            let Ok(mut st) = shared.state.lock() else {
                return;
            };
            if st.generation != generation {
                break;
            }
            if n == 0 {
                st.done = true;
                shared.cv.notify_all();
                break;
            }
            st.data.extend_from_slice(&buf[..n]);
            let read = shared.read_pos.load(Ordering::Relaxed);
            if st.data.len() as u64 > AHEAD_LIMIT + BEHIND_LIMIT && read > st.start + BEHIND_LIMIT {
                let drop = (read - st.start - BEHIND_LIMIT) as usize;
                st.data.drain(..drop);
                st.start += drop as u64;
            }
            shared.cv.notify_all();
        }
    }
}

impl HttpSource {
    fn open(url: &str, ctx: &SourceCtx) -> Result<(HttpSource, Arc<AtomicBool>), StreamError> {
        let first = request(&agent(), url, 0).map_err(StreamError::new)?;
        let len = first.0.total;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                start: 0,
                data: Vec::new(),
                done: false,
                error: None,
                generation: 0,
                want_from: Some(0),
            }),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
            read_pos: AtomicU64::new(0),
        });
        let worker = Arc::clone(&shared);
        let url = url.to_string();
        std::thread::Builder::new()
            .name("mosaic-http".into())
            .spawn(move || download(worker, url, Ok(first)))
            .map_err(|e| StreamError::new(e.to_string()))?;
        let hide_len = Arc::new(AtomicBool::new(true));
        let source = HttpSource {
            shared,
            pos: 0,
            len,
            hide_len: Arc::clone(&hide_len),
            buffering: Arc::clone(&ctx.buffering),
            cancel: Arc::clone(&ctx.cancel),
        };
        Ok((source, hide_len))
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.cv.notify_all();
    }
}

impl Read for HttpSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut stalled_since: Option<Instant> = None;
        let mut reported = false;
        let finish = |reported: bool, this: &HttpSource| {
            if reported {
                (this.buffering)(false);
            }
        };

        let Ok(mut st) = self.shared.state.lock() else {
            return Err(io::Error::other("stream state poisoned"));
        };
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                drop(st);
                finish(reported, self);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let end = st.start + st.data.len() as u64;
            if self.pos >= st.start && self.pos < end {
                let off = (self.pos - st.start) as usize;
                let n = out.len().min(st.data.len() - off);
                out[..n].copy_from_slice(&st.data[off..off + n]);
                self.pos += n as u64;
                self.shared.read_pos.store(self.pos, Ordering::Relaxed);
                self.shared.cv.notify_all();
                drop(st);
                finish(reported, self);
                return Ok(n);
            }
            if st.done && self.pos >= end {
                drop(st);
                finish(reported, self);
                return Ok(0);
            }
            if let Some(e) = st.error.clone() {
                drop(st);
                finish(reported, self);
                return Err(io::Error::other(e));
            }
            if self.pos < st.start || self.pos > end + NEAR_GAP {
                st.generation += 1;
                st.start = self.pos;
                st.data.clear();
                st.done = false;
                st.error = None;
                st.want_from = Some(self.pos);
                self.shared.read_pos.store(self.pos, Ordering::Relaxed);
                self.shared.cv.notify_all();
            }
            let since = *stalled_since.get_or_insert_with(Instant::now);
            if !reported && since.elapsed() >= STALL_AFTER {
                reported = true;
                drop(st);
                (self.buffering)(true);
                st = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| io::Error::other("stream state poisoned"))?;
                continue;
            }
            st = match self.shared.cv.wait_timeout(st, Duration::from_millis(50)) {
                Ok((s, _)) => s,
                Err(_) => return Err(io::Error::other("stream state poisoned")),
            };
        }
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => self.len.and_then(|l| l.checked_add_signed(d)),
        };
        let target =
            target.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad seek"))?;
        self.pos = target;
        Ok(target)
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        self.len.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        if self.hide_len.load(Ordering::Relaxed) {
            None
        } else {
            self.len
        }
    }
}

pub fn open(url: &str, ctx: &SourceCtx) -> Result<Box<dyn AudioStream>, StreamError> {
    let (source, hide_len) = HttpSource::open(url, ctx)?;
    let ext = url
        .split(['?', '#'])
        .next()
        .and_then(|p| p.rsplit_once('.'))
        .map(|(_, e)| e.to_string());
    let after_probe: Box<dyn FnOnce() + Send> =
        Box::new(move || hide_len.store(false, Ordering::Relaxed));
    Ok(Box::new(SymphoniaStream::new(
        Box::new(source),
        ext.as_deref().filter(|e| e.len() <= 4),
        ctx.duration_hint,
        Some(after_probe),
    )?))
}
