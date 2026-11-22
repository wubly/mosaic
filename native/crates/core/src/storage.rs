use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::model::{Config, Store};

pub const STORE_FILE: &str = "mosaic-data.json";
pub const CONFIG_FILE: &str = "mosaic-config.json";
pub const LOCAL_CACHE_FILE: &str = "local-cache.json";
pub const LYRICS_CHOICES_FILE: &str = "lyrics-choices.json";
pub const SPOTIFY_AUTH_FILE: &str = "spotify-auth.json";
const MIGRATED_MARKER: &str = ".migrated-from";
const DEBOUNCE: Duration = Duration::from_millis(600);

#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("no home directory could be determined")]
    NoHome,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug)]
pub struct Dirs {
    pub data: PathBuf,
    pub cache: PathBuf,
    legacy: Vec<PathBuf>,
}

impl Dirs {
    pub fn resolve() -> Result<Dirs, StorageError> {
        let base = directories::BaseDirs::new().ok_or(StorageError::NoHome)?;
        let (data, cache, legacy_root) = if cfg!(target_os = "windows") {
            (
                base.data_dir().join("mosaic").join("data"),
                base.cache_dir().join("mosaic").join("cache"),
                base.data_dir().to_path_buf(),
            )
        } else if cfg!(target_os = "macos") {
            (
                base.data_dir().join("app.mosaic.player"),
                base.cache_dir().join("app.mosaic.player"),
                base.data_dir().to_path_buf(),
            )
        } else {
            (
                base.data_dir().join("mosaic"),
                base.cache_dir().join("mosaic"),
                base.config_dir().to_path_buf(),
            )
        };
        Ok(Dirs {
            data,
            cache,
            legacy: vec![legacy_root.join("Mosaic"), legacy_root.join("mosaic")],
        })
    }

    /// Everything under one root; used by `--data-dir` and tests. Migration is disabled.
    pub fn with_root(root: impl Into<PathBuf>) -> Dirs {
        let root = root.into();
        Dirs {
            cache: root.join("cache"),
            data: root,
            legacy: Vec::new(),
        }
    }

    pub fn with_legacy(mut self, legacy: Vec<PathBuf>) -> Dirs {
        self.legacy = legacy;
        self
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.data.join(name)
    }

    pub fn covers(&self) -> PathBuf {
        self.cache.join("covers")
    }

    pub fn images(&self) -> PathBuf {
        self.cache.join("images")
    }

    pub fn legacy_candidates(&self) -> &[PathBuf] {
        &self.legacy
    }
}

/// Reads JSON, returning the default for a missing file. A corrupt file is kept as `<file>.corrupt`.
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(bytes) = fs::read(path) else {
        return T::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".corrupt");
            let _ = fs::copy(path, backup);
            T::default()
        }
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    write_atomic(path, &serde_json::to_vec(value)?)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_store(dirs: &Dirs) -> Store {
    read_json(&dirs.file(STORE_FILE))
}

pub fn load_config(dirs: &Dirs) -> Config {
    read_json(&dirs.file(CONFIG_FILE))
}

pub fn save_config(dirs: &Dirs, config: &Config) -> Result<(), StorageError> {
    write_json_atomic(&dirs.file(CONFIG_FILE), config)
}

/// Copies an existing Electron install's data on first run. Returns the legacy dir used.
pub fn migrate_legacy(dirs: &Dirs) -> Result<Option<PathBuf>, StorageError> {
    if dirs.file(STORE_FILE).exists() {
        return Ok(None);
    }
    let Some(legacy) = dirs
        .legacy_candidates()
        .iter()
        .find(|d| d.join(STORE_FILE).is_file())
    else {
        return Ok(None);
    };

    fs::create_dir_all(&dirs.data)?;
    for name in [
        STORE_FILE,
        CONFIG_FILE,
        LYRICS_CHOICES_FILE,
        SPOTIFY_AUTH_FILE,
    ] {
        let from = legacy.join(name);
        if from.is_file() {
            fs::copy(&from, dirs.file(name))?;
        }
    }

    let covers = legacy.join("covers");
    if covers.is_dir() {
        fs::create_dir_all(dirs.covers())?;
        for entry in fs::read_dir(&covers)?.flatten() {
            if entry.path().is_file() {
                fs::copy(entry.path(), dirs.covers().join(entry.file_name()))?;
            }
        }
    }

    for name in [STORE_FILE, LYRICS_CHOICES_FILE] {
        let path = dirs.file(name);
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) {
                rewrite_media_urls(&mut value, &dirs.covers());
                write_json_atomic(&path, &value)?;
            }
        }
    }

    fs::write(
        dirs.file(MIGRATED_MARKER),
        legacy.to_string_lossy().as_bytes(),
    )?;
    Ok(Some(legacy.clone()))
}

fn rewrite_media_urls(value: &mut Value, covers: &Path) {
    match value {
        Value::String(s) => {
            if let Some(file) = s.strip_prefix("media://art/") {
                *s = covers.join(file).to_string_lossy().into_owned();
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| rewrite_media_urls(v, covers)),
        Value::Object(map) => map.values_mut().for_each(|v| rewrite_media_urls(v, covers)),
        _ => {}
    }
}

struct SaverState {
    pending: Option<Store>,
    last_change: std::time::Instant,
    closing: bool,
}

/// Writes the store 600 ms after the last change, and on `flush`/drop.
pub struct Saver {
    path: PathBuf,
    shared: Arc<(Mutex<SaverState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Saver {
    pub fn new(dirs: &Dirs) -> Saver {
        let path = dirs.file(STORE_FILE);
        let shared = Arc::new((
            Mutex::new(SaverState {
                pending: None,
                last_change: std::time::Instant::now(),
                closing: false,
            }),
            Condvar::new(),
        ));
        let thread = {
            let shared = Arc::clone(&shared);
            let path = path.clone();
            std::thread::spawn(move || Saver::run(&shared, &path))
        };
        Saver {
            path,
            shared,
            thread: Some(thread),
        }
    }

    fn run(shared: &(Mutex<SaverState>, Condvar), path: &Path) {
        let (lock, cv) = shared;
        let Ok(mut state) = lock.lock() else { return };
        loop {
            if state.closing {
                return;
            }
            match state.pending.is_some() {
                false => {
                    state = match cv.wait(state) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                }
                true => {
                    let quiet = state.last_change.elapsed();
                    if quiet >= DEBOUNCE {
                        if let Some(store) = state.pending.take() {
                            drop(state);
                            let _ = write_json_atomic(path, &store);
                            state = match lock.lock() {
                                Ok(s) => s,
                                Err(_) => return,
                            };
                        }
                    } else {
                        state = match cv.wait_timeout(state, DEBOUNCE - quiet) {
                            Ok((s, _)) => s,
                            Err(_) => return,
                        };
                    }
                }
            }
        }
    }

    pub fn schedule(&self, store: &Store) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut state) = lock.lock() {
            state.pending = Some(store.clone());
            state.last_change = std::time::Instant::now();
            cv.notify_all();
        }
    }

    /// Writes `store` immediately and discards anything pending.
    pub fn flush(&self, store: &Store) -> Result<(), StorageError> {
        if let Ok(mut state) = self.shared.0.lock() {
            state.pending = None;
        }
        write_json_atomic(&self.path, store)
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        let pending = lock.lock().ok().and_then(|mut s| {
            s.closing = true;
            cv.notify_all();
            s.pending.take()
        });
        if let Some(store) = pending {
            let _ = write_json_atomic(&self.path, &store);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackId;
    use serde_json::json;

    fn fixture() -> Value {
        let sc = json!({"id": 123456789u64, "source": "soundcloud", "title": "Song", "artist": "Artist",
            "album": "", "artwork": "https://i1.sndcdn.com/a-t500x500.jpg", "thumb": "https://i1.sndcdn.com/a-t200x200.jpg",
            "duration": 215000, "permalink": "https://soundcloud.com/a/b", "streamable": true, "customField": {"x": [1, 2.5]}});
        let local = json!({"id": "local:0123456789abcdef", "source": "local", "title": "Local", "artist": "L",
            "album": "Alb", "artwork": "media://art/0123456789abcdef.jpg", "thumb": "media://art/0123456789abcdef-s.jpg",
            "duration": 180000, "streamable": true, "folder": "/m", "lrcPath": "/m/a.lrc"});
        let spotify = json!({"id": "sp:abc", "source": "spotify", "title": "Sp", "artist": "S", "album": "",
            "artwork": "", "thumb": "", "duration": 1000, "streamable": true,
            "uri": "spotify:track:abc", "artistId": "ar", "albumId": "al"});
        let youtube = json!({"id": "yt:xyz", "source": "youtube", "title": "Yt", "artist": "Y", "album": "",
            "artwork": "", "thumb": "", "duration": 1000, "streamable": true, "videoId": "xyz"});
        json!({
            "likes": [sc, local],
            "playlists": [{"id": "k3j2h1x9", "name": "Mix", "tracks": [spotify, youtube]}],
            "history": [{"track": sc, "at": 1732000000000i64}],
            "plays": {"123456789": {"track": sc, "count": 3, "seconds": 615}, "local:0123456789abcdef": {"track": local, "count": 1, "seconds": 12.5}},
            "days": {"2026-11-22": 1830, "2026-11-21": 60.5},
            "sources": ["/home/u/Music"],
            "settings": {"autoplay": true, "crossfade": 6, "fadePause": true, "skipSilence": true, "ytVideo": false,
                "leveling": false, "eqOn": true, "eqGains": [0, 1.5, -2, 0, 3], "eqPreset": "Flat", "speed": 1, "volume": 0.8,
                "shuffle": false, "repeat": "all", "lyricsOpen": true, "presence": true, "barHeight": 100, "sideWidth": 360,
                "futureSetting": "kept"},
            "session": {"queue": [sc], "index": 0, "position": 42.3},
            "topLevelUnknown": 7
        })
    }

    #[test]
    fn store_round_trips_losslessly() {
        let original = fixture();
        let store: Store = serde_json::from_value(original.clone()).unwrap();
        let out = serde_json::to_value(&store).unwrap();
        assert_eq!(out, original);
        let again: Store = serde_json::from_value(out.clone()).unwrap();
        assert_eq!(serde_json::to_value(&again).unwrap(), out);
    }

    #[test]
    fn numeric_and_string_ids_stay_distinct() {
        let store: Store = serde_json::from_value(fixture()).unwrap();
        assert_eq!(store.likes[0].id, TrackId::Num(123456789));
        assert_ne!(TrackId::Num(5), TrackId::Str("5".into()));
        assert_eq!(store.likes[1].id.key(), "local:0123456789abcdef");
    }

    #[test]
    fn empty_store_gets_defaults() {
        let store: Store = serde_json::from_value(json!({"settings": {"volume": 0.3}})).unwrap();
        assert_eq!(store.settings.volume, 0.3);
        assert!(store.settings.autoplay);
        assert_eq!(store.settings.side_width, 360.0);
        assert!(store.likes.is_empty());
    }

    #[test]
    fn local_id_matches_electron() {
        // node -e "console.log('local:'+require('crypto').createHash('sha1').update('/home/u/Music/a.mp3'.toLowerCase()).digest('hex').slice(0,16))"
        assert_eq!(
            TrackId::for_local_path("/home/u/Music/A.mp3"),
            TrackId::Str("local:7122d348d2875f64".into())
        );
    }

    #[test]
    fn config_reset_keeps_only_whitelisted_keys() {
        let config: Config = serde_json::from_value(json!({
            "name": "N", "onboarded": true, "clientId": "cid", "closeToTray": false, "genius": true,
            "beta": false, "spotifyClientId": "sp", "lowMemory": true
        }))
        .unwrap();
        let reset = serde_json::to_value(config.reset()).unwrap();
        assert_eq!(
            reset,
            json!({"clientId": "cid", "closeToTray": false, "genius": true, "spotifyClientId": "sp"})
        );
        assert!(Config::default().close_to_tray());
    }

    #[test]
    fn atomic_write_and_corrupt_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("f.json");
        write_json_atomic(&path, &json!({"a": 1})).unwrap();
        assert!(!dir.path().join("sub").join("f.json.tmp").exists());
        assert_eq!(read_json::<Value>(&path), json!({"a": 1}));
        fs::write(&path, b"{nope").unwrap();
        assert_eq!(read_json::<Config>(&path).name, None);
        assert!(dir.path().join("sub").join("f.json.corrupt").exists());
        assert_eq!(
            read_json::<Value>(&dir.path().join("missing.json")),
            Value::Null
        );
    }

    #[test]
    fn saver_debounces_and_flushes_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::with_root(dir.path());
        let saver = Saver::new(&dirs);
        let mut store = Store::default();
        store.sources.push("/a".into());
        saver.schedule(&store);
        assert!(!dirs.file(STORE_FILE).exists());
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(load_store(&dirs).sources, ["/a"]);
        store.sources.push("/b".into());
        saver.schedule(&store);
        drop(saver);
        assert_eq!(load_store(&dirs).sources, ["/a", "/b"]);
    }

    #[test]
    fn migrates_legacy_install() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        fs::create_dir_all(legacy.join("covers")).unwrap();
        fs::write(legacy.join("covers").join("ab.jpg"), b"jpg").unwrap();
        fs::write(
            legacy.join(STORE_FILE),
            serde_json::to_vec(&fixture()).unwrap(),
        )
        .unwrap();
        fs::write(legacy.join(CONFIG_FILE), br#"{"name":"N"}"#).unwrap();
        fs::write(legacy.join(LOCAL_CACHE_FILE), b"{}").unwrap();

        let dirs = Dirs::with_root(root.path().join("new"))
            .with_legacy(vec![root.path().join("none"), legacy.clone()]);
        assert_eq!(migrate_legacy(&dirs).unwrap(), Some(legacy.clone()));

        assert!(dirs.covers().join("ab.jpg").exists());
        assert!(!dirs.file(LOCAL_CACHE_FILE).exists());
        assert_eq!(load_config(&dirs).name.as_deref(), Some("N"));
        assert_eq!(
            fs::read_to_string(dirs.file(MIGRATED_MARKER)).unwrap(),
            legacy.to_string_lossy()
        );

        let store = load_store(&dirs);
        let expected = dirs
            .covers()
            .join("0123456789abcdef.jpg")
            .to_string_lossy()
            .into_owned();
        assert_eq!(store.likes[1].artwork, expected);
        assert!(store.likes[1].thumb.ends_with("0123456789abcdef-s.jpg"));
        assert!(store.plays["local:0123456789abcdef"]
            .track
            .artwork
            .starts_with(dirs.covers().to_str().unwrap()));

        assert_eq!(migrate_legacy(&dirs).unwrap(), None);
    }
}
