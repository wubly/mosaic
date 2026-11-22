use super::silence::Silence;
use super::tracks::Codec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamKind {
    Progressive,
    Hls(Codec),
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub kind: StreamKind,
    pub url: String,
    pub silence: Option<Silence>,
}
