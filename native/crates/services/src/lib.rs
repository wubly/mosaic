pub mod autoplay;
pub mod discord;
pub mod http;
pub mod images;
pub mod local;
pub mod lyrics;
pub mod monochrome;
pub mod oauth;
pub mod ranking;
pub mod soundcloud;
pub mod spotify;
pub mod update;
pub mod youtube;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mosaic_core::model::{Config, TrackId};
use mosaic_core::storage::{save_config, Dirs};

pub(crate) struct Inner {
    pub dirs: Dirs,
    pub http: reqwest::Client,
    pub config: Arc<Mutex<Config>>,
    pub sc_client_id: Mutex<Option<String>>,
    pub local_paths: Mutex<HashMap<TrackId, PathBuf>>,
    pub lyrics: lyrics::State,
    pub images: images::State,
    pub spotify: spotify::auth::State,
    pub youtube: youtube::auth::State,
    /// Whether Spotify decides what plays after the current Spotify track (nothing of Mosaic's
    /// own follows it). See `Services::set_spotify_leads`.
    pub spotify_leads: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct Services {
    pub(crate) inner: Arc<Inner>,
}

impl Services {
    pub fn new(dirs: Dirs, config: Arc<Mutex<Config>>) -> anyhow::Result<Services> {
        let sc_client_id = config
            .lock()
            .map_err(|_| anyhow::anyhow!("config poisoned"))?
            .client_id
            .clone();
        Ok(Services {
            inner: Arc::new(Inner {
                http: http::build_client()?,
                spotify: spotify::auth::State::load(&dirs),
                youtube: youtube::auth::State::load(&dirs),
                dirs,
                config,
                sc_client_id: Mutex::new(sc_client_id.filter(|s| !s.is_empty())),
                local_paths: Mutex::default(),
                lyrics: lyrics::State::default(),
                images: images::State::new(),
                spotify_leads: Default::default(),
            }),
        })
    }

    /// Set by the app whenever the queue changes: true while nothing of Mosaic's own follows
    /// the playing Spotify track, so Spotify's queue and autoplay carry on from it.
    pub fn set_spotify_leads(&self, on: bool) {
        self.inner
            .spotify_leads
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn spotify_leads(&self) -> bool {
        self.inner
            .spotify_leads
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn dirs(&self) -> &Dirs {
        &self.inner.dirs
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    pub fn config(&self) -> Config {
        self.inner
            .config
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        if let Ok(mut config) = self.inner.config.lock() {
            f(&mut config);
            let _ = save_config(&self.inner.dirs, &config);
        }
    }

    pub fn local_path(&self, id: &TrackId) -> Option<PathBuf> {
        self.inner.local_paths.lock().ok()?.get(id).cloned()
    }
}
