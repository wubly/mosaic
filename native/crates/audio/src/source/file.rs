use std::path::Path;

use crate::decode::{AudioStream, StreamError, SymphoniaStream};

pub fn open(path: &Path, duration_hint: Option<f64>) -> Result<Box<dyn AudioStream>, StreamError> {
    let file =
        std::fs::File::open(path).map_err(|e| StreamError(format!("Could not open file: {e}")))?;
    let extension = path.extension().and_then(|e| e.to_str());
    Ok(Box::new(SymphoniaStream::new(
        Box::new(file),
        extension,
        duration_hint,
    )?))
}
