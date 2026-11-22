pub mod file;
pub mod hls;
pub mod http;
pub mod push;

use crate::decode::{AudioStream, StreamError};
use crate::SourceSpec;

pub fn open(
    spec: &SourceSpec,
    duration_hint: Option<f64>,
) -> Result<Box<dyn AudioStream>, StreamError> {
    match spec {
        SourceSpec::File(path) => file::open(path, duration_hint),
    }
}
