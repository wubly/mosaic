use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use futures::stream::{self, StreamExt};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::*;
use lofty::tag::ItemKey;
use mosaic_core::model::{Source, Track, TrackId};
use mosaic_core::storage::{read_json, write_json_atomic, LOCAL_CACHE_FILE};
use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::covers::save_cover;
use crate::Services;

pub const AUDIO_EXT: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "wav", "ogg", "opus", "wma", "webm",
];
const CACHE_VERSION: u32 = 1;
const SCAN_WORKERS: usize = 4;

static NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?)\s+-\s+(.+)$").expect("static regex"));

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXT.contains(&e.to_lowercase().as_str()))
}

#[derive(Serialize, Deserialize, Default)]
struct CacheFile {
    v: u32,
    entries: HashMap<String, CacheEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct CacheEntry {
    mtime: u64,
    track: Track,
}

/// Track built only from the file name: "Artist - Title" or just the title.
pub fn fallback_track(file: &Path, id: TrackId) -> Track {
    let base = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (artist, title) = match NAMED.captures(&base) {
        Some(m) => (m[1].to_string(), m[2].to_string()),
        None => ("Unknown artist".to_string(), base),
    };
    let folder = file
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    Track {
        id,
        source: Source::Local,
        title,
        artist,
        album: String::new(),
        artwork: String::new(),
        thumb: String::new(),
        duration: 0,
        permalink: None,
        streamable: true,
        folder,
        lrc_path: None,
        uri: None,
        artist_id: None,
        album_id: None,
        video_id: None,
        extra: Default::default(),
    }
}

fn sort_key(s: &str) -> String {
    s.nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .to_lowercase()
}

pub fn sort_tracks(tracks: &mut [Track]) {
    tracks.sort_by(|a, b| {
        sort_key(&a.artist)
            .cmp(&sort_key(&b.artist))
            .then_with(|| sort_key(&a.title).cmp(&sort_key(&b.title)))
            .then_with(|| a.artist.cmp(&b.artist))
            .then_with(|| a.title.cmp(&b.title))
    });
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty()).map(str::to_string)
}

fn read_tags(file: &Path, track: &mut Track, cover_dir: &Path) {
    let Ok(tagged) = lofty::read_from_path(file) else {
        return;
    };
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    track.duration = tagged.properties().duration().as_millis() as u64;
    if let Some(tag) = tag {
        if let Some(t) = non_empty(tag.title().as_deref()) {
            track.title = t;
        }
        if let Some(a) = non_empty(tag.artist().as_deref())
            .or_else(|| non_empty(tag.get_string(ItemKey::AlbumArtist)))
        {
            track.artist = a;
        }
        track.album = non_empty(tag.album().as_deref()).unwrap_or_default();
        if let Some(picture) = tag.pictures().first() {
            if let Ok(cover) = save_cover(cover_dir, &id_string(&track.id), picture.data()) {
                track.artwork = cover.artwork;
                track.thumb = cover.thumb;
            }
        }
    }
}

fn id_string(id: &TrackId) -> String {
    id.key()
}

fn mtime_ms(file: &Path) -> Option<u64> {
    let modified = std::fs::metadata(file).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

fn collect_files(paths: &[String]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for target in paths {
        let target = PathBuf::from(target);
        let Ok(meta) = std::fs::metadata(&target) else {
            continue;
        };
        let found: Vec<PathBuf> = if meta.is_dir() {
            walkdir::WalkDir::new(&target)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| !e.file_type().is_dir() && is_audio(e.path()))
                .map(walkdir::DirEntry::into_path)
                .collect()
        } else if is_audio(&target) {
            vec![target]
        } else {
            Vec::new()
        };
        for f in found {
            if seen.insert(f.clone()) {
                files.push(f);
            }
        }
    }
    files
}

fn sibling_lrc(file: &Path) -> Option<String> {
    let lrc = file.with_extension("lrc");
    lrc.exists().then(|| lrc.to_string_lossy().into_owned())
}

impl Services {
    /// Scans folders and files for audio. Results are cached by modification time.
    pub async fn scan_paths(&self, paths: &[String]) -> Result<Vec<Track>> {
        let cache_path = self.inner.dirs.file(LOCAL_CACHE_FILE);
        let cover_dir = self.inner.dirs.covers();

        let paths = paths.to_vec();
        let files = tokio::task::spawn_blocking(move || collect_files(&paths)).await?;

        let mut cache: CacheFile = read_json(&cache_path);
        if cache.v != CACHE_VERSION {
            cache = CacheFile {
                v: CACHE_VERSION,
                entries: HashMap::new(),
            };
        }
        let cache = std::sync::Arc::new(cache);

        let results: Vec<Option<(String, CacheEntry, Track)>> = stream::iter(files)
            .map(|file| {
                let cache = std::sync::Arc::clone(&cache);
                let cover_dir = cover_dir.clone();
                tokio::task::spawn_blocking(move || read_local_track(&file, &cache, &cover_dir))
            })
            .buffer_unordered(SCAN_WORKERS)
            .map(|r| r.ok().flatten())
            .collect()
            .await;

        let mut next = CacheFile {
            v: CACHE_VERSION,
            entries: HashMap::new(),
        };
        let mut tracks = Vec::new();
        {
            let mut local = self
                .inner
                .local_paths
                .lock()
                .map_err(|_| anyhow::anyhow!("poisoned"))?;
            for (file, entry, track) in results.into_iter().flatten() {
                local.insert(track.id.clone(), PathBuf::from(&file));
                next.entries.insert(file, entry);
                tracks.push(track);
            }
        }
        let _ = write_json_atomic(&cache_path, &next);

        sort_tracks(&mut tracks);
        Ok(tracks)
    }
}

fn read_local_track(
    file: &Path,
    cache: &CacheFile,
    cover_dir: &Path,
) -> Option<(String, CacheEntry, Track)> {
    let key = file.to_string_lossy().into_owned();
    let mtime = mtime_ms(file)?;
    let lrc_path = sibling_lrc(file);

    if let Some(hit) = cache.entries.get(&key).filter(|h| h.mtime == mtime) {
        let mut track = hit.track.clone();
        track.lrc_path = lrc_path;
        return Some((key, hit.clone(), track));
    }

    let id = TrackId::for_local_path(&key);
    let mut track = fallback_track(file, id);
    read_tags(file, &mut track, cover_dir);
    let entry = CacheEntry {
        mtime,
        track: track.clone(),
    };
    track.lrc_path = lrc_path;
    Some((key, entry, track))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> TrackId {
        TrackId::Str("local:x".into())
    }

    #[test]
    fn filename_parsing() {
        let t = fallback_track(Path::new("/m/Rock/Artist Name - Song Title.mp3"), id());
        assert_eq!(
            (t.artist.as_str(), t.title.as_str(), t.folder.as_deref()),
            ("Artist Name", "Song Title", Some("Rock"))
        );

        let t = fallback_track(Path::new("/m/just a title.flac"), id());
        assert_eq!(
            (t.artist.as_str(), t.title.as_str()),
            ("Unknown artist", "just a title")
        );

        let t = fallback_track(Path::new("/m/A - B - C.ogg"), id());
        assert_eq!((t.artist.as_str(), t.title.as_str()), ("A", "B - C"));

        let t = fallback_track(Path::new("/m/no-dash-spaces.mp3"), id());
        assert_eq!(t.title, "no-dash-spaces");
        assert!(t.streamable && t.source == Source::Local);
    }

    #[test]
    fn audio_extension_filter() {
        assert!(is_audio(Path::new("a.MP3")));
        assert!(is_audio(Path::new("a.opus")));
        assert!(!is_audio(Path::new("a.txt")));
        assert!(!is_audio(Path::new("mp3")));
    }

    #[test]
    fn sorting_ignores_case_and_accents() {
        let mut v = vec![
            fallback_track(Path::new("/m/b - z.mp3"), id()),
            fallback_track(Path::new("/m/Ágata - y.mp3"), id()),
            fallback_track(Path::new("/m/a - a.mp3"), id()),
        ];
        sort_tracks(&mut v);
        let artists: Vec<_> = v.iter().map(|t| t.artist.as_str()).collect();
        assert_eq!(artists, ["a", "Ágata", "b"]);
    }

    #[tokio::test]
    async fn scans_directory_with_cache_and_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        std::fs::create_dir_all(music.join("sub")).unwrap();
        std::fs::write(music.join("A - One.mp3"), b"not really audio").unwrap();
        std::fs::write(music.join("A - One.lrc"), b"[00:01.00]hi").unwrap();
        std::fs::write(music.join("sub").join("Two.flac"), b"x").unwrap();
        std::fs::write(music.join("notes.txt"), b"x").unwrap();

        let dirs = mosaic_core::storage::Dirs::with_root(dir.path().join("data"));
        let services = Services::new(dirs, Default::default()).unwrap();
        let tracks = services
            .scan_paths(&[music.to_string_lossy().into_owned()])
            .await
            .unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "One");
        assert!(tracks[0]
            .lrc_path
            .as_deref()
            .is_some_and(|p| p.ends_with("A - One.lrc")));
        assert!(tracks[1].lrc_path.is_none());
        assert!(services.local_path(&tracks[0].id).is_some());

        let again = services
            .scan_paths(&[music.to_string_lossy().into_owned()])
            .await
            .unwrap();
        assert_eq!(again.len(), 2);
    }
}
