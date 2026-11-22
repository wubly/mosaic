pub mod discord;
pub mod http;
pub mod images;
pub mod local;
pub mod lyrics;
pub mod oauth;
pub mod soundcloud;
pub mod spotify;
pub mod update;

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
                dirs,
                config,
                sc_client_id: Mutex::new(sc_client_id.filter(|s| !s.is_empty())),
                local_paths: Mutex::default(),
                lyrics: lyrics::State::default(),
                images: images::State::new(),
            }),
        })
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
