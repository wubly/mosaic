//! OS media controls: MPRIS (Linux), SMTC (Windows), MPNowPlayingInfoCenter (macOS).

use std::ffi::c_void;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};

const SEEK_STEP: f64 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Absolute position in seconds.
    SeekTo(f64),
    /// Relative seek in seconds.
    SeekBy(f64),
    Raise,
}

pub fn command_from(event: MediaControlEvent) -> Option<MediaCommand> {
    Some(match event {
        MediaControlEvent::Play => MediaCommand::Play,
        MediaControlEvent::Pause => MediaCommand::Pause,
        MediaControlEvent::Toggle => MediaCommand::Toggle,
        MediaControlEvent::Next => MediaCommand::Next,
        MediaControlEvent::Previous => MediaCommand::Previous,
        MediaControlEvent::Stop => MediaCommand::Stop,
        MediaControlEvent::SetPosition(MediaPosition(p)) => MediaCommand::SeekTo(p.as_secs_f64()),
        MediaControlEvent::SeekBy(dir, by) => MediaCommand::SeekBy(signed(dir, by.as_secs_f64())),
        MediaControlEvent::Seek(dir) => MediaCommand::SeekBy(signed(dir, SEEK_STEP)),
        MediaControlEvent::Raise => MediaCommand::Raise,
        _ => return None,
    })
}

fn signed(dir: SeekDirection, secs: f64) -> f64 {
    match dir {
        SeekDirection::Forward => secs,
        SeekDirection::Backward => -secs,
    }
}

/// `file://` for local covers, URLs untouched.
pub fn cover_url(artwork: &str) -> Option<String> {
    match artwork {
        "" => None,
        a if a.starts_with("http://") || a.starts_with("https://") || a.starts_with("file://") => {
            Some(a.to_string())
        }
        a => Some(format!("file://{a}")),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<String>,
    pub duration: Option<Duration>,
}

pub struct Media {
    controls: MediaControls,
}

/// A raw window handle, only needed by SMTC on Windows.
#[derive(Clone, Copy)]
pub struct Hwnd(pub *mut c_void);

// SAFETY: the pointer is only handed to the OS, never dereferenced by us.
unsafe impl Send for Hwnd {}

impl Media {
    pub fn new(
        hwnd: Option<Hwnd>,
        on_command: impl Fn(MediaCommand) + Send + 'static,
    ) -> Result<Media, String> {
        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "mosaic",
            display_name: "Mosaic",
            hwnd: hwnd.map(|h| h.0),
        })
        .map_err(|e| format!("{e:?}"))?;
        controls
            .attach(move |event| {
                if let Some(cmd) = command_from(event) {
                    on_command(cmd);
                }
            })
            .map_err(|e| format!("{e:?}"))?;
        Ok(Media { controls })
    }

    pub fn set_metadata(&mut self, m: &Metadata) {
        let _ = self.controls.set_metadata(MediaMetadata {
            title: Some(&m.title),
            artist: Some(&m.artist),
            album: (!m.album.is_empty()).then_some(m.album.as_str()),
            cover_url: m.cover.as_deref(),
            duration: m.duration,
        });
    }

    pub fn set_playback(&mut self, track_loaded: bool, playing: bool, position_secs: f64) {
        let progress = Some(MediaPosition(Duration::from_secs_f64(
            position_secs.max(0.0),
        )));
        let state = match (track_loaded, playing) {
            (false, _) => MediaPlayback::Stopped,
            (true, true) => MediaPlayback::Playing { progress },
            (true, false) => MediaPlayback::Paused { progress },
        };
        let _ = self.controls.set_playback(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_map_to_commands() {
        assert_eq!(
            command_from(MediaControlEvent::Toggle),
            Some(MediaCommand::Toggle)
        );
        assert_eq!(
            command_from(MediaControlEvent::SetPosition(MediaPosition(
                Duration::from_secs(90)
            ))),
            Some(MediaCommand::SeekTo(90.0))
        );
        assert_eq!(
            command_from(MediaControlEvent::Seek(SeekDirection::Backward)),
            Some(MediaCommand::SeekBy(-5.0))
        );
        assert_eq!(command_from(MediaControlEvent::Quit), None);
    }

    #[test]
    fn covers_become_urls() {
        assert_eq!(cover_url(""), None);
        assert_eq!(
            cover_url("https://x/a.jpg").as_deref(),
            Some("https://x/a.jpg")
        );
        assert_eq!(
            cover_url("/home/u/c/a.jpg").as_deref(),
            Some("file:///home/u/c/a.jpg")
        );
    }
}
