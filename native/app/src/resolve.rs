use mosaic_audio::SourceSpec;
use mosaic_core::model::{Source, Track};
use mosaic_services::soundcloud::client_types::StreamKind;
use mosaic_services::Services;

use crate::player::Stream;

/// Turns a track into something the engine can open. Errors are user-facing messages.
pub async fn resolve_stream(
    services: &Services,
    track: &Track,
    seek_to: f64,
) -> Result<Stream, String> {
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
        Source::YouTube => {
            let id = track
                .video_id
                .clone()
                .or_else(|| track.id.key().strip_prefix("yt:").map(str::to_string))
                .ok_or("Could not play this track")?;
            let playback = services.yt_stream(&id).await.map_err(|e| e.to_string())?;
            Ok(Stream {
                source: SourceSpec::Http { url: playback.url },
                silence: None,
            })
        }
        Source::Monochrome => Ok(Stream {
            source: SourceSpec::Http {
                url: mosaic_services::monochrome::stream_url(&track.id),
            },
            silence: None,
        }),
        #[cfg(feature = "spotify")]
        Source::Spotify => spotify(services, track, seek_to).await,
        _ => Err("Could not play this track".into()),
    }
}

#[cfg(feature = "spotify")]
async fn spotify(services: &Services, track: &Track, seek_to: f64) -> Result<Stream, String> {
    use std::sync::Arc;

    use mosaic_services::spotify::playback::Target;

    let uri = track.uri.clone().ok_or("Could not play this track")?;
    let playback = services
        .spotify_playback()
        .await
        .map_err(|e| e.to_string())?;
    let seeker = playback.clone();
    let duration = (track.duration > 0).then(|| track.duration as f64 / 1000.0);
    let (producer, handle) = mosaic_audio::push_pair(seek_to, duration, move |secs| {
        seeker.seek((secs.max(0.0) * 1000.0) as u32);
    });
    let (writer, finisher, failer) = (producer.clone(), producer.clone(), producer);
    let from = uri.clone();
    let target = Target {
        write: Arc::new(move |samples| writer.write(samples)),
        finish: Arc::new(move || finisher.finish()),
        fail: Arc::new(move |message| failer.fail(message)),
        moved: Arc::new(move |to| {
            let _ = crate::external::sender().send(crate::external::External::SpotifyMoved {
                from: from.clone(),
                to,
            });
        }),
    };
    playback
        .load(&uri, (seek_to.max(0.0) * 1000.0) as u32, target)
        .map_err(|e| e.to_string())?;
    Ok(Stream {
        source: SourceSpec::Push(handle),
        silence: None,
    })
}
