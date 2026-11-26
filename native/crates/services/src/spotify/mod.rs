pub mod auth;
pub mod embed;
pub mod import;
pub mod library;
pub mod matcher;
#[cfg(feature = "spotify")]
pub mod playback;
#[cfg(feature = "spotify")]
pub mod soloist;
pub mod url;
// Used on Windows and macOS; built everywhere so its tests run on every platform.
#[cfg(feature = "spotify")]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub mod web;
pub mod web_api;
