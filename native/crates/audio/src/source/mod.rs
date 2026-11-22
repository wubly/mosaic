pub mod ffmpeg;
pub mod file;
pub mod hls;
pub mod http;
pub mod push;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::decode::{AudioStream, StreamError};
use crate::SourceSpec;

/// A browser-like agent string; some CDNs reject unknown clients.
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36";
pub(crate) const EXPIRED: &str = "stream expired";

/// What a source needs from the decoder that owns it.
pub struct SourceCtx {
    pub duration_hint: Option<f64>,
    pub out_rate: u32,
    /// Reports whether reads are stalled waiting on the network.
    pub buffering: Arc<dyn Fn(bool) + Send + Sync>,
    /// Set when the decoder is being stopped; blocking reads must give up.
    pub cancel: Arc<AtomicBool>,
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(12)))
        .timeout_recv_body(Some(Duration::from_secs(20)))
        .build()
        .new_agent()
}

pub fn open(spec: &SourceSpec, ctx: &SourceCtx) -> Result<Box<dyn AudioStream>, StreamError> {
    match spec {
        SourceSpec::File(path) => file::open(path, ctx),
        SourceSpec::Http { url } => http::open(url, ctx),
        SourceSpec::Hls { playlist_url } => hls::open(playlist_url, ctx),
        SourceSpec::Push(handle) => push::open(handle, ctx),
    }
}
