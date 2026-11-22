//! Spotify playback (feature `spotify`). librespot decodes in-process and a custom sink hands the
//! audio to whoever called `load` (the engine's push source). Accounts Spotify refuses librespot
//! keys for can use Spotify Soloist instead (see `soloist`), picked when a Soloist key is saved.

use std::sync::{Arc, Mutex};

use super::soloist::{Soloist, NOT_PAIRED};
use crate::Services;
use anyhow::{anyhow, Result};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_core::spotify_uri::SpotifyUri;
use librespot_metadata::{Metadata, Track};
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::config::{Bitrate, PlayerConfig};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent};

static BACKEND: tokio::sync::Mutex<Option<SpotifyPlayback>> = tokio::sync::Mutex::const_new(None);

fn shut_down(backend: &SpotifyPlayback) {
    match backend {
        SpotifyPlayback::Librespot(l) => l.stop(),
        SpotifyPlayback::Soloist(s) => {
            s.stop();
            s.shutdown();
        }
    }
}

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
/// Spotify only hands librespot decryption keys to accounts that have recently used a legacy
/// (eSDK v1) device; newer accounts are refused for every track (librespot issue #1649).
pub const KEY_REFUSED: &str = "Spotify refused this account a playback key (newer accounts can't play through librespot). Set up Spotify Soloist in Settings \u{2192} Beta to play Spotify tracks";

/// Where decoded audio goes: interleaved stereo f32 at 44.1 kHz.
#[derive(Clone)]
pub struct Target {
    pub write: Arc<dyn Fn(Vec<f32>) -> bool + Send + Sync>,
    pub finish: Arc<dyn Fn() + Send + Sync>,
    pub fail: Arc<dyn Fn(String) + Send + Sync>,
    /// Soloist moved on to another track by itself (the URI); called just before `finish`.
    pub moved: Arc<dyn Fn(String) + Send + Sync>,
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
pub struct Librespot {
    player: Arc<Player>,
    target: Shared,
}

/// Whichever backend is playing Spotify tracks.
#[derive(Clone)]
pub enum SpotifyPlayback {
    Librespot(Librespot),
    Soloist(Soloist),
}

impl SpotifyPlayback {
    /// Starts `uri` at `position_ms`; audio (or, for Soloist, a silent clock) goes to `target`.
    pub fn load(&self, uri: &str, position_ms: u32, target: Target) -> Result<()> {
        match self {
            Self::Librespot(l) => l.load(uri, position_ms, target),
            Self::Soloist(s) => s.load(uri, position_ms, target),
        }
    }

    pub fn pause(&self) {
        match self {
            Self::Librespot(l) => l.pause(),
            Self::Soloist(s) => s.pause(),
        }
    }

    pub fn resume(&self) {
        match self {
            Self::Librespot(l) => l.resume(),
            Self::Soloist(s) => s.resume(),
        }
    }

    pub fn seek(&self, position_ms: u32) {
        match self {
            Self::Librespot(l) => l.seek(position_ms),
            Self::Soloist(s) => s.seek(position_ms),
        }
    }

    pub fn stop(&self) {
        match self {
            Self::Librespot(l) => l.stop(),
            Self::Soloist(s) => s.stop(),
        }
    }

    /// librespot's audio runs through the deck (which applies the volume); Soloist's doesn't.
    pub fn set_volume(&self, v: f64) {
        if let Self::Soloist(s) = self {
            s.set_volume(v);
        }
    }

    /// Soloist's own up-next URIs (asks for a fresh list as a side effect); empty for librespot.
    pub fn queue(&self) -> Vec<String> {
        match self {
            Self::Librespot(_) => Vec::new(),
            Self::Soloist(s) => {
                s.request_queue();
                s.queue()
            }
        }
    }

    /// A one-line status for Settings once the backend is up.
    pub fn describe(&self) -> String {
        match self {
            Self::Librespot(_) => "Spotify playback is ready".into(),
            Self::Soloist(s) if s.paired() => "Spotify Soloist is running and paired".into(),
            Self::Soloist(_) => NOT_PAIRED.into(),
        }
    }
}

pub fn parse_uri(uri: &str) -> Option<SpotifyUri> {
    SpotifyUri::from_uri(uri).ok()
}

impl Librespot {
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
    /// The playback backend, created on first use: Soloist when a Soloist key is saved, otherwise
    /// librespot. A Soloist that died is restarted.
    pub async fn spotify_playback(&self) -> Result<SpotifyPlayback> {
        let mut slot = BACKEND.lock().await;
        let soloist = self.soloist_settings();
        if let Some(current) = slot.as_ref() {
            let keep = match current {
                SpotifyPlayback::Soloist(s) => soloist.configured() && s.alive(),
                SpotifyPlayback::Librespot(_) => !soloist.configured(),
            };
            if keep {
                return Ok(current.clone());
            }
            shut_down(current);
            *slot = None;
        }
        let backend = if soloist.configured() {
            SpotifyPlayback::Soloist(self.start_soloist(&soloist).await?)
        } else {
            SpotifyPlayback::Librespot(self.create_playback().await?)
        };
        *slot = Some(backend.clone());
        Ok(backend)
    }

    /// The backend if one is already running; never starts one.
    pub async fn spotify_playback_current(&self) -> Option<SpotifyPlayback> {
        BACKEND.lock().await.clone()
    }

    /// Drops the current backend (after Soloist settings change) so the next use starts afresh.
    pub async fn spotify_playback_reset(&self) {
        if let Some(current) = BACKEND.lock().await.take() {
            shut_down(&current);
        }
    }

    async fn create_playback(&self) -> Result<Librespot> {
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
        let probe_session = session.clone();
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
                    PlayerEvent::Unavailable { track_id, .. } => {
                        // librespot reports a refused key as a generic load failure, so ask for
                        // the key ourselves to tell the two apart.
                        let session = probe_session.clone();
                        tokio::spawn(async move {
                            let refused = audio_key_refused(&session, &track_id).await;
                            (t.fail)(if refused { KEY_REFUSED } else { UNAVAILABLE }.into());
                        });
                    }
                    _ => {}
                }
            }
        });
        Ok(Librespot { player, target })
    }
}

/// True when Spotify serves the track's metadata but refuses its decryption key. Any other
/// failure (no metadata, no files) is left to the generic "unavailable" message.
async fn audio_key_refused(session: &Session, uri: &SpotifyUri) -> bool {
    let SpotifyUri::Track { id } = uri else {
        return false;
    };
    let Ok(track) = Track::get(session, uri).await else {
        return false;
    };
    let Some(&file) = track.files.values().next() else {
        return false;
    };
    session.audio_key().request(*id, file).await.is_err()
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
