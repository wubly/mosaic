use std::collections::HashMap;

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use sha1::{Digest, Sha1};

type Extra = Map<String, Value>;

/// Serializes whole numbers as JSON integers, matching `JSON.stringify` in the Electron app.
pub fn ser_num<S: Serializer>(value: &f64, s: S) -> Result<S::Ok, S::Error> {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9e15 {
        s.serialize_i64(*value as i64)
    } else {
        s.serialize_f64(*value)
    }
}

fn ser_opt_num<S: Serializer>(value: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(v) => ser_num(v, s),
        None => s.serialize_none(),
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(untagged)]
pub enum TrackId {
    Num(u64),
    Str(String),
}

impl TrackId {
    pub fn key(&self) -> String {
        match self {
            TrackId::Num(n) => n.to_string(),
            TrackId::Str(s) => s.clone(),
        }
    }

    /// Id of a local file; must match `idForPath` in the Electron scanner.
    pub fn for_local_path(path: &str) -> TrackId {
        let digest = Sha1::digest(path.to_lowercase().as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        TrackId::Str(format!("local:{}", &hex[..16]))
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    SoundCloud,
    Local,
    Spotify,
    YouTube,
    Monochrome,
    Other(String),
}

impl Source {
    pub fn as_str(&self) -> &str {
        match self {
            Source::SoundCloud => "soundcloud",
            Source::Local => "local",
            Source::Spotify => "spotify",
            Source::YouTube => "youtube",
            Source::Monochrome => "monochrome",
            Source::Other(s) => s,
        }
    }
}

impl Serialize for Source {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "soundcloud" => Source::SoundCloud,
            "local" => Source::Local,
            "spotify" => Source::Spotify,
            "youtube" => Source::YouTube,
            "monochrome" => Source::Monochrome,
            _ => Source::Other(s),
        })
    }
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: TrackId,
    pub source: Source,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub artwork: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub duration: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(default = "yes")]
    pub streamable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lrc_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Track {
    /// The form written to disk: `lrcPath` only survives on local tracks.
    pub fn strip(&self) -> Track {
        let mut t = self.clone();
        if t.source != Source::Local {
            t.lrc_path = None;
        }
        t
    }

    pub fn art(&self) -> &str {
        if self.thumb.is_empty() {
            &self.artwork
        } else {
            &self.thumb
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    pub track: Track,
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayStat {
    pub track: Track,
    #[serde(default)]
    pub count: u64,
    #[serde(default, serialize_with = "ser_num")]
    pub seconds: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub autoplay: bool,
    #[serde(serialize_with = "ser_num")]
    pub crossfade: f64,
    pub fade_pause: bool,
    pub skip_silence: bool,
    pub yt_video: bool,
    pub leveling: bool,
    pub eq_on: bool,
    #[serde(serialize_with = "ser_gains")]
    pub eq_gains: [f64; 5],
    pub eq_preset: String,
    #[serde(serialize_with = "ser_num")]
    pub speed: f64,
    #[serde(serialize_with = "ser_num")]
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub lyrics_open: bool,
    pub presence: bool,
    #[serde(serialize_with = "ser_num")]
    pub bar_height: f64,
    #[serde(serialize_with = "ser_num")]
    pub side_width: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            autoplay: true,
            crossfade: 0.0,
            fade_pause: true,
            skip_silence: true,
            yt_video: false,
            leveling: false,
            eq_on: false,
            eq_gains: [0.0; 5],
            eq_preset: "Flat".into(),
            speed: 1.0,
            volume: 0.8,
            shuffle: false,
            repeat: Repeat::Off,
            lyrics_open: true,
            presence: true,
            bar_height: 100.0,
            side_width: 360.0,
            extra: Extra::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Session {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<Vec<Track>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_opt_num"
    )]
    pub position: Option<f64>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Session {
    pub fn merge(&mut self, patch: Session) {
        if patch.queue.is_some() {
            self.queue = patch.queue;
        }
        if patch.index.is_some() {
            self.index = patch.index;
        }
        if patch.position.is_some() {
            self.position = patch.position;
        }
        self.extra.extend(patch.extra);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Store {
    pub likes: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub history: Vec<HistoryEntry>,
    pub plays: IndexMap<String, PlayStat>,
    #[serde(serialize_with = "ser_days")]
    pub days: HashMap<String, f64>,
    pub sources: Vec<String>,
    pub settings: Settings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
    #[serde(flatten)]
    pub extra: Extra,
}

fn ser_days<S: Serializer>(days: &HashMap<String, f64>, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut keys: Vec<_> = days.keys().collect();
    keys.sort();
    let mut map = s.serialize_map(Some(days.len()))?;
    for k in keys {
        map.serialize_entry(k, &Num(days[k]))?;
    }
    map.end()
}

fn ser_gains<S: Serializer>(gains: &[f64; 5], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(gains.iter().map(|g| Num(*g)))
}

struct Num(f64);

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ser_num(&self.0, s)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onboarded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_to_tray: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genius: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beta: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spotify_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub youtube_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub youtube_client_secret: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Config {
    pub fn close_to_tray(&self) -> bool {
        self.close_to_tray != Some(false)
    }

    /// Keeps only the keys `config:reset` preserves in the Electron app.
    pub fn reset(&self) -> Config {
        Config {
            client_id: self.client_id.clone().filter(|s| !s.is_empty()),
            close_to_tray: (self.close_to_tray == Some(false)).then_some(false),
            genius: (self.genius == Some(true)).then_some(true),
            beta: (self.beta == Some(true)).then_some(true),
            spotify_client_id: self.spotify_client_id.clone().filter(|s| !s.is_empty()),
            youtube_client_id: self.youtube_client_id.clone().filter(|s| !s.is_empty()),
            youtube_client_secret: self.youtube_client_secret.clone().filter(|s| !s.is_empty()),
            ..Config::default()
        }
    }
}
