use std::path::Path;

use super::SourceCtx;
use crate::decode::{AudioStream, StreamError, SymphoniaStream};

pub fn open(path: &Path, ctx: &SourceCtx) -> Result<Box<dyn AudioStream>, StreamError> {
    let file = std::fs::File::open(path)
        .map_err(|e| StreamError::new(format!("Could not open file: {e}")))?;
    let extension = path.extension().and_then(|e| e.to_str());
    match SymphoniaStream::new(Box::new(file), extension, ctx.duration_hint, None) {
        Ok(stream) => Ok(Box::new(stream)),
        Err(e) if e.unsupported => super::ffmpeg::open(path, ctx),
        Err(e) => Err(e),
    }
}
