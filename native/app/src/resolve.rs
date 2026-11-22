use mosaic_audio::SourceSpec;
use mosaic_core::model::{Source, Track};
use mosaic_services::soundcloud::client_types::StreamKind;
use mosaic_services::Services;

use crate::player::Stream;

/// Turns a track into something the engine can open. Errors are user-facing messages.
pub async fn resolve_stream(services: &Services, track: &Track) -> Result<Stream, String> {
    match track.source {
        Source::Local => {
            let path = services
                .local_path(&track.id)
                .ok_or_else(|| "Could not play this track".to_string())?;
            Ok(Stream {
                source: SourceSpec::File(path),
                silence: None,
            })
        }
        Source::SoundCloud => {
            let info = services
                .sc_stream(&track.id)
                .await
                .map_err(|e| e.to_string())?;
            let source = match info.kind {
                StreamKind::Progressive => SourceSpec::Http { url: info.url },
                StreamKind::Hls(_) => SourceSpec::Hls {
                    playlist_url: info.url,
                },
            };
            Ok(Stream {
                source,
                silence: info.silence,
            })
        }
        _ => Err("Could not play this track".into()),
    }
}
