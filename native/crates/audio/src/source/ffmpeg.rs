use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

use super::SourceCtx;
use crate::decode::{AudioStream, StreamError};

const CHUNK_FRAMES: usize = 4096;
const MISSING: &str = "This format needs ffmpeg installed";

/// Decodes through an `ffmpeg` child process, for formats symphonia can't read (WMA and friends).
pub struct FfmpegStream {
    path: PathBuf,
    rate: u32,
    duration: Option<f64>,
    child: Child,
    stdout: ChildStdout,
    pos: f64,
}

fn spawn(path: &Path, start: f64, rate: u32) -> Result<(Child, ChildStdout), StreamError> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-ss",
            &format!("{start:.3}"),
            "-i",
        ])
        .arg(path)
        .args([
            "-vn",
            "-f",
            "f32le",
            "-ac",
            "2",
            "-ar",
            &rate.to_string(),
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|_| StreamError::new(MISSING))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| StreamError::new(MISSING))?;
    Ok((child, stdout))
}

fn probe_duration(path: &Path) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

pub fn open(path: &Path, ctx: &SourceCtx) -> Result<Box<dyn AudioStream>, StreamError> {
    let (child, stdout) = spawn(path, 0.0, ctx.out_rate)?;
    let duration = probe_duration(path).or(ctx.duration_hint);
    Ok(Box::new(FfmpegStream {
        path: path.to_path_buf(),
        rate: ctx.out_rate,
        duration,
        child,
        stdout,
        pos: 0.0,
    }))
}

impl Drop for FfmpegStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl AudioStream for FfmpegStream {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn seek(&mut self, secs: f64) -> Result<f64, StreamError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (child, stdout) = spawn(&self.path, secs.max(0.0), self.rate)?;
        self.child = child;
        self.stdout = stdout;
        self.pos = secs.max(0.0);
        Ok(self.pos)
    }

    fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<Option<f64>, StreamError> {
        let mut bytes = vec![0u8; CHUNK_FRAMES * 8];
        let mut filled = 0;
        while filled < bytes.len() {
            match self.stdout.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) => return Err(StreamError::new(format!("Playback failed: {e}"))),
            }
        }
        let frames = filled / 8;
        if frames == 0 {
            return if self.pos == 0.0 {
                Err(StreamError::new("Could not play this file"))
            } else {
                Ok(None)
            };
        }
        out.clear();
        out.extend(
            bytes[..frames * 8]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b)),
        );
        let start = self.pos;
        self.pos += frames as f64 / f64::from(self.rate);
        Ok(Some(start))
    }
}
