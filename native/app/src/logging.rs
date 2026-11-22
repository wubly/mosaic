//! `tracing` to stderr and `<cache>/mosaic.log` (rotated at 5 MB). `MOSAIC_LOG` sets the filter.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mosaic_core::storage::Dirs;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

const LIMIT: u64 = 5 * 1024 * 1024;

struct Rotating {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

impl Rotating {
    fn open(path: PathBuf) -> Rotating {
        let written = std::fs::metadata(&path).map_or(0, |m| m.len());
        let mut log = Rotating {
            path,
            file: None,
            written,
        };
        if written >= LIMIT {
            log.rotate();
        } else {
            log.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log.path)
                .ok();
        }
        log
    }

    fn rotate(&mut self) {
        let old = self.path.with_extension("log.1");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&self.path, old);
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }
}

#[derive(Clone)]
struct Tee(Arc<Mutex<Rotating>>);

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        if let Ok(mut log) = self.0.lock() {
            if log.written >= LIMIT {
                log.rotate();
            }
            if let Some(f) = &mut log.file {
                let _ = f.write_all(buf);
            }
            log.written += buf.len() as u64;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

impl<'a> MakeWriter<'a> for Tee {
    type Writer = Tee;

    fn make_writer(&'a self) -> Tee {
        self.clone()
    }
}

pub fn init(dirs: &Dirs) {
    let cache = dirs
        .images()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let _ = std::fs::create_dir_all(&cache);
    let tee = Tee(Arc::new(Mutex::new(Rotating::open(
        cache.join("mosaic.log"),
    ))));
    let filter = EnvFilter::try_from_env("MOSAIC_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(tee)
        .with_ansi(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_logs_rotate_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mosaic.log");
        std::fs::write(&path, vec![b'x'; LIMIT as usize + 1]).unwrap();
        let log = Rotating::open(path.clone());
        assert_eq!(log.written, 0);
        assert!(dir.path().join("mosaic.log.1").exists());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    #[test]
    fn small_logs_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mosaic.log");
        std::fs::write(&path, "old\n").unwrap();
        let log = Rotating::open(path);
        assert_eq!(log.written, 4);
        assert!(!dir.path().join("mosaic.log.1").exists());
    }
}
