//! Spotify playback through librespot (feature `spotify`). A custom sink hands the decoded audio to
//! whoever called `load`, which in the app is the engine's push source.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_core::spotify_uri::SpotifyUri;
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::config::{Bitrate, PlayerConfig};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent};
use tokio::sync::OnceCell;

use crate::Services;

const REDIRECT: &str = "http://127.0.0.1:8898/login";
const SCOPES: [&str; 8] = [
    "streaming",
    "user-read-private",
    "user-read-email",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-library-read",
    "playlist-read-private",
    "user-top-read",
];
const AUDIO_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

pub const UNAVAILABLE: &str = "This track isn't available on Spotify in your region";
pub const NEEDS_PREMIUM: &str = "Spotify playback needs Premium";

/// Where decoded audio goes: interleaved stereo f32 at 44.1 kHz.
#[derive(Clone)]
pub struct Target {
    pub write: Arc<dyn Fn(Vec<f32>) -> bool + Send + Sync>,
    pub finish: Arc<dyn Fn() + Send + Sync>,
    pub fail: Arc<dyn Fn(String) + Send + Sync>,
}

type Shared = Arc<Mutex<Option<Target>>>;

struct EngineSink {
    target: Shared,
}

impl Sink for EngineSink {
    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(samples) = packet else {
            return Ok(());
        };
        let target = self
            .target
            .lock()
            .map_err(|_| SinkError::OnWrite("sink poisoned".into()))?
            .clone();
        let Some(target) = target else { return Ok(()) };
        // A dropped chunk (full queue after the timeout) means the deck is paused or gone; the
        // player is paused with it, so losing it is harmless.
        (target.write)(converter.f64_to_f32(&samples));
        Ok(())
    }
}

#[derive(Clone)]
pub struct SpotifyPlayback {
    player: Arc<Player>,
    target: Shared,
}

pub fn parse_uri(uri: &str) -> Option<SpotifyUri> {
    SpotifyUri::from_uri(uri).ok()
}

impl SpotifyPlayback {
    /// Starts `uri` at `position_ms`; audio goes to `target`.
    pub fn load(&self, uri: &str, position_ms: u32, target: Target) -> Result<()> {
        let id = parse_uri(uri).ok_or_else(|| anyhow!("Not a Spotify track"))?;
        if let Ok(mut slot) = self.target.lock() {
            *slot = Some(target);
        }
        self.player.load(id, true, position_ms);
        Ok(())
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.player.play();
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    pub fn stop(&self) {
        self.player.stop();
    }
}

async fn login(cache: &Cache) -> Result<Credentials> {
    if let Some(saved) = cache.credentials() {
        return Ok(saved);
    }
    let client_id = SessionConfig::default().client_id;
    let token = tokio::task::spawn_blocking(move || {
        OAuthClientBuilder::new(&client_id, REDIRECT, SCOPES.to_vec())
            .open_in_browser()
            .build()
            .and_then(|c| c.get_access_token())
    })
    .await?
    .map_err(|e| anyhow!("Spotify sign-in failed: {e}"))?;
    Ok(Credentials::with_access_token(token.access_token))
}

impl Services {
    /// Creates (once) the librespot session and player.
    pub async fn spotify_playback(&self) -> Result<SpotifyPlayback> {
        static CELL: OnceCell<SpotifyPlayback> = OnceCell::const_new();
        CELL.get_or_try_init(|| self.create_playback())
            .await
            .cloned()
    }

    async fn create_playback(&self) -> Result<SpotifyPlayback> {
        let cache = Cache::new(
            Some(self.inner.dirs.file("librespot")),
            None,
            Some(self.inner.dirs.images().with_file_name("librespot-audio")),
            Some(AUDIO_CACHE_BYTES),
        )
        .map_err(|e| anyhow!("Could not open the Spotify cache: {e}"))?;
        let credentials = login(&cache).await?;
        let session = Session::new(SessionConfig::default(), Some(cache));
        session.connect(credentials, true).await.map_err(|e| {
            let msg = e.to_string();
            if msg.to_lowercase().contains("premium") {
                anyhow!(NEEDS_PREMIUM)
            } else {
                anyhow!("Could not connect to Spotify: {msg}")
            }
        })?;

        let target: Shared = Arc::default();
        let sink_target = Arc::clone(&target);
        let player = Player::new(
            PlayerConfig {
                bitrate: Bitrate::Bitrate320,
                gapless: true,
                normalisation: false,
                ..PlayerConfig::default()
            },
            session,
            Box::new(NoOpVolume),
            move || {
                Box::new(EngineSink {
                    target: sink_target,
                })
            },
        );

        let mut events = player.get_player_event_channel();
        let event_target = Arc::clone(&target);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let current = event_target.lock().ok().and_then(|t| t.clone());
                let Some(t) = current else { continue };
                match event {
                    PlayerEvent::EndOfTrack { .. } => (t.finish)(),
                    PlayerEvent::Unavailable { .. } => (t.fail)(UNAVAILABLE.into()),
                    _ => {}
                }
            }
        });
        Ok(SpotifyPlayback { player, target })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_uris_parse_and_garbage_does_not() {
        assert!(parse_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").is_some());
        assert!(parse_uri("not a uri").is_none());
        assert!(parse_uri("").is_none());
    }
}
