pub mod genius;
pub mod lrclib;
pub mod parse;
pub mod service;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

use mosaic_core::lyrics_timing::LyricLine;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LyricsKind {
    Instrumental,
    Word,
    Synced,
    Plain,
    None,
}

impl LyricsKind {
    pub fn label(self) -> &'static str {
        match self {
            LyricsKind::Instrumental => "Instrumental",
            LyricsKind::Word => "Word synced",
            LyricsKind::Synced => "Synced",
            LyricsKind::Plain => "Plain",
            LyricsKind::None => "None",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LyricsMeta {
    pub origin: String,
    pub id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub kind: LyricsKind,
}

#[derive(Clone, Debug)]
pub struct Lyrics {
    pub synced: Vec<LyricLine>,
    pub plain: String,
    pub meta: LyricsMeta,
}

/// An lrclib record, also the shape stored in `lyrics-choices.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub id: Option<i64>,
    pub track_name: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    pub duration: Option<f64>,
    pub instrumental: Option<bool>,
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
}

impl Record {
    pub fn has_synced(&self) -> bool {
        self.synced_lyrics
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }

    pub fn has_lyrics(&self) -> bool {
        self.synced_lyrics.as_deref().is_some_and(|s| !s.is_empty())
            || self.plain_lyrics.as_deref().is_some_and(|s| !s.is_empty())
    }
}

pub struct State {
    pub(crate) auto_cache: Mutex<HashMap<String, Lyrics>>,
    pub(crate) candidates: Mutex<HashMap<i64, Record>>,
    pub(crate) permits: Semaphore,
    pub(crate) failures: AtomicU64,
}

impl Default for State {
    fn default() -> Self {
        State {
            auto_cache: Mutex::default(),
            candidates: Mutex::default(),
            permits: Semaphore::new(lrclib::MAX_IN_FLIGHT),
            failures: AtomicU64::new(0),
        }
    }
}
