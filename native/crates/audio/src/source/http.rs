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
/// Reconnects tried after a dropped connection before the stream gives up. Servers close
/// connections that sit idle while the buffer is full, so a long track can need several.
const RECONNECTS: u32 = 6;

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
    let total = first.as_ref().ok().and_then(|(h, _)| h.total);
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
        let mut reader: Option<Box<dyn Read + Send>> = match response {
            Ok((_, r)) => Some(r),
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
        let mut failures = 0u32;
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
                    // Let go of the connection while the buffer is full: servers drop idle
                    // ones, and the agent's body timeout counts the whole download.
                    reader = None;
                    let _ = shared.cv.wait_timeout(st, Duration::from_millis(50));
                    continue;
                }
            }
            let Some(r) = reader.as_mut() else {
                failures = 0;
                match reconnect(&shared, &agent, &url, generation, &mut failures, false) {
                    Reconnect::Resumed(r) => reader = Some(r),
                    Reconnect::Superseded => break,
                    Reconnect::Stopped => return,
                    Reconnect::Failed(e) => {
                        if let Ok(mut st) = shared.state.lock() {
                            if st.generation == generation {
                                st.error = Some(e);
                            }
                        }
                        shared.cv.notify_all();
                        break;
                    }
                }
                continue;
            };
            let read = r.read(&mut buf);
            // A dropped connection (an error, or the body ending before the known length)
            // resumes from where the buffer ends instead of ending the track.
            let cut = match &read {
                Err(e) => Some(format!("Stream interrupted: {e}")),
                Ok(0) => {
                    let end = shared
                        .state
                        .lock()
                        .map(|st| st.start + st.data.len() as u64)
                        .unwrap_or(0);
                    total
                        .filter(|t| end < *t)
                        .map(|_| "Stream ended early".to_string())
                }
                Ok(_) => None,
            };
            if let Some(why) = cut {
                match reconnect(&shared, &agent, &url, generation, &mut failures, true) {
                    Reconnect::Resumed(r) => {
                        reader = Some(r);
                        continue;
                    }
                    Reconnect::Superseded => break,
                    Reconnect::Stopped => return,
                    Reconnect::Failed(e) => {
                        if let Ok(mut st) = shared.state.lock() {
                            if st.generation == generation {
                                st.error = Some(if e.is_empty() { why } else { e });
                            }
                        }
                        shared.cv.notify_all();
                        break;
                    }
                }
            }
            let n = read.unwrap_or(0);
            let Ok(mut st) = shared.state.lock() else {
                return;
            };
            if st.generation != generation {
                break;
            }
            if n > 0 {
                failures = 0;
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

enum Reconnect {
    Resumed(Box<dyn Read + Send>),
    /// A seek started a new download meanwhile.
    Superseded,
    Stopped,
    /// Gave up; the message is the last error (empty if there was none to report).
    Failed(String),
}

fn reconnect(
    shared: &Shared,
    agent: &ureq::Agent,
    url: &str,
    generation: u64,
    failures: &mut u32,
    backoff: bool,
) -> Reconnect {
    let mut last = String::new();
    while *failures < RECONNECTS {
        *failures += 1;
        if backoff || *failures > 1 {
            std::thread::sleep(Duration::from_millis(250 * (1 << (*failures - 1).min(4))));
        }
        if shared.stop.load(Ordering::Relaxed) {
            return Reconnect::Stopped;
        }
        let from = {
            let Ok(st) = shared.state.lock() else {
                return Reconnect::Stopped;
            };
            if st.generation != generation {
                return Reconnect::Superseded;
            }
            st.start + st.data.len() as u64
        };
        match request(agent, url, from) {
            Ok((_, r)) => {
                tracing::debug!(from, attempt = *failures, "stream resumed");
                return Reconnect::Resumed(r);
            }
            // An expired link won't come back; the player re-resolves it.
            Err(e) if e == super::EXPIRED => return Reconnect::Failed(e),
            Err(e) => last = e,
        }
    }
    Reconnect::Failed(last)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// Serves `body` with Range support, cutting each of the first `cuts` responses short.
    fn flaky_server(body: Vec<u8>, cuts: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (served, conn) in listener.incoming().enumerate() {
                let Ok(mut conn) = conn else { return };
                let mut from = 0usize;
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                    }
                }
                let rest = &body[from..];
                let _ = write!(
                    conn,
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\
                     Content-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                    rest.len(),
                    from,
                    body.len() - 1,
                    body.len()
                );
                let send = if served < cuts { rest.len().min(100_000) } else { rest.len() };
                let _ = conn.write_all(&rest[..send]);
            }
        });
        format!("http://{addr}/track")
    }

    #[test]
    fn dropped_connections_resume_where_they_stopped() {
        let body: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        let url = flaky_server(body.clone(), 3);
        let ctx = SourceCtx {
            duration_hint: None,
            out_rate: 48_000,
            buffering: Arc::new(|_| {}),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let (mut source, _) = HttpSource::open(&url, &ctx).unwrap();
        let mut got = Vec::new();
        source.read_to_end(&mut got).unwrap();
        assert_eq!(got.len(), body.len());
        assert!(got == body);
    }
}
