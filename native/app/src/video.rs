//! Picture for the in-app video view: an `ffmpeg` child decodes a video-only stream into raw RGBA
//! frames, and the app shows whichever frame the audio clock says is due. The sound is never
//! touched here; the engine keeps playing it, so pause, seek and volume work as they always do.
//!
//! The decoder runs ahead of the clock only as far as a three-frame queue allows, so a paused
//! video costs nothing, and a seek is a restart of ffmpeg at the new position.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use iced::widget::image::Handle;

const QUEUE: usize = 3;
/// Widest frame decoded. The view is a window, and every pixel is copied through a pipe and
/// uploaded to the GPU thirty times a second.
const MAX_WIDTH: u32 = 960;
/// A frame this far ahead of the audio means the audio was seeked back.
const AHEAD: f64 = 0.75;
/// With nothing queued, this far behind the audio means it was seeked forward (or ffmpeg is
/// too slow to keep up).
const BEHIND: f64 = 2.0;
/// How long ffmpeg gets to produce its first frame before the view gives up on it.
const FIRST_FRAME: Duration = Duration::from_secs(20);
const MISSING: &str = "Watching a video needs ffmpeg installed";

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

struct Frame {
    pts: f64,
    handle: Handle,
}

/// What one look at the decoder found.
#[derive(Default)]
pub struct Poll {
    /// The newest frame that is due, if there is one.
    pub frame: Option<Handle>,
    /// The audio left the part of the video that is decoded; start again at the clock.
    pub restart: bool,
    /// The decoder died before producing anything.
    pub failed: Option<String>,
}

pub struct Decoder {
    child: Child,
    frames: Receiver<Frame>,
    pending: Option<Frame>,
    started: Instant,
    /// Source-time of the last frame handed out (or where decoding began).
    last_pts: f64,
    any: bool,
}

/// `(width, height)` of the decoded frames for a stream: even, and at most `MAX_WIDTH` wide.
pub fn output_size(width: u32, height: u32) -> (u32, u32) {
    let w = width.clamp(2, MAX_WIDTH) & !1;
    let h = ((u64::from(w) * u64::from(height) / u64::from(width.max(1))) as u32).max(2) & !1;
    (w, h)
}

impl Decoder {
    pub fn start(
        url: &str,
        width: u32,
        height: u32,
        fps: u32,
        from: f64,
    ) -> Result<Decoder, String> {
        let (w, h) = output_size(width, height);
        let fps = fps.clamp(1, 30);
        let mut child = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-user_agent", USER_AGENT])
            .args(["-reconnect", "1", "-reconnect_streamed", "1"])
            .args(["-ss", &format!("{:.3}", from.max(0.0)), "-i", url])
            .args(["-an", "-sn", "-vf"])
            .arg(format!("fps={fps},scale={w}:{h}:flags=fast_bilinear"))
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    MISSING.to_string()
                } else {
                    format!("Could not start ffmpeg: {e}")
                }
            })?;
        let mut stdout = child.stdout.take().ok_or_else(|| MISSING.to_string())?;
        let (tx, rx) = sync_channel::<Frame>(QUEUE);
        let size = w as usize * h as usize * 4;
        let start = from.max(0.0);
        std::thread::Builder::new()
            .name("video-decode".into())
            .spawn(move || {
                let mut n = 0u64;
                loop {
                    let mut buf = vec![0u8; size];
                    if stdout.read_exact(&mut buf).is_err() {
                        break;
                    }
                    let frame = Frame {
                        pts: start + n as f64 / f64::from(fps),
                        handle: Handle::from_rgba(w, h, buf),
                    };
                    n += 1;
                    if tx.send(frame).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("Could not start the video decoder: {e}"))?;
        Ok(Decoder {
            child,
            frames: rx,
            pending: None,
            started: Instant::now(),
            last_pts: start,
            any: false,
        })
    }

    /// Takes the frames due at `clock` (source seconds). Frames already late are dropped, so a
    /// slow moment catches up instead of falling behind for good.
    pub fn poll(&mut self, clock: f64) -> Poll {
        let mut out = Poll::default();
        let mut ended = false;
        loop {
            if self.pending.is_none() {
                match self.frames.try_recv() {
                    Ok(f) => self.pending = Some(f),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
            let Some(f) = &self.pending else { break };
            if f.pts > clock + AHEAD {
                out.restart = true;
                return out;
            }
            if f.pts > clock + 0.02 {
                break;
            }
            if let Some(f) = self.pending.take() {
                self.last_pts = f.pts;
                self.any = true;
                out.frame = Some(f.handle);
            }
        }
        if out.frame.is_some() {
            return out;
        }
        if ended && !self.any {
            out.failed = Some("ffmpeg couldn't decode this video".into());
        } else if !self.any {
            if self.started.elapsed() > FIRST_FRAME {
                out.failed = Some("The video took too long to start".into());
            }
        } else if self.pending.is_none() && !ended && clock - self.last_pts > BEHIND {
            out.restart = true;
        }
        out
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // Killing ffmpeg ends the decoder thread's read; the queue's receiver is dropped right
        // after, which wakes it if it is blocked on a full queue.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_even_and_capped() {
        assert_eq!(output_size(1280, 720), (960, 540));
        assert_eq!(output_size(854, 480), (854, 480));
        assert_eq!(output_size(640, 360), (640, 360));
        // A portrait video keeps its shape.
        assert_eq!(output_size(405, 720), (404, 718));
        assert_eq!(output_size(1, 1), (2, 2));
    }

    /// Live check: `MOSAIC_TEST_VIDEO_URL=<url> cargo test -p mosaic video -- --ignored`.
    #[test]
    #[ignore]
    fn decodes_a_live_stream_and_follows_the_clock() {
        let url = std::env::var("MOSAIC_TEST_VIDEO_URL").expect("MOSAIC_TEST_VIDEO_URL");
        let mut d = Decoder::start(&url, 1280, 720, 25, 30.0).expect("ffmpeg starts");
        let wait = |d: &mut Decoder, clock: f64| {
            let t = Instant::now();
            loop {
                let p = d.poll(clock);
                if p.frame.is_some() || p.restart || p.failed.is_some() {
                    return p;
                }
                assert!(t.elapsed() < Duration::from_secs(20), "no frame");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        // The first frame is at 30.0 s and is due as soon as the clock reaches it.
        let p = wait(&mut d, 30.0);
        assert!(p.frame.is_some() && !p.restart && p.failed.is_none());
        // Half a second on, a frame for that moment is there.
        assert!(wait(&mut d, 30.5).frame.is_some());
        // A seek back leaves the decoded part, and so does one far forward.
        let t = Instant::now();
        while !d.poll(10.0).restart {
            assert!(t.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut d = Decoder::start(&url, 1280, 720, 25, 30.0).unwrap();
        wait(&mut d, 30.0);
        let t = Instant::now();
        while !d.poll(90.0).restart {
            assert!(t.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
