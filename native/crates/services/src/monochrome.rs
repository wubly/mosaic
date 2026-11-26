//! Monochrome (<https://github.com/monochrome-music/monochrome>): a lossless catalog served
//! by its public "tracks" API. Search is plain JSON and every track streams as one FLAC file
//! over HTTP (with range requests), so playback needs nothing beyond the HTTP source.

use anyhow::{bail, Result};
use mosaic_core::model::{Source, Track, TrackId};
use serde_json::Value;

use crate::Services;

mod radio;
pub use radio::{best_match, core_title};

pub const API: &str = "https://tracks.monochrome.st";
const ID_PREFIX: &str = "mono:";
const SEARCH_LIMIT: usize = 30;

/// The Monochrome id inside a track id (`mono:<id>`), or the id itself.
pub fn raw_id(id: &TrackId) -> String {
    let key = id.key();
    key.strip_prefix(ID_PREFIX).unwrap_or(&key).to_string()
}

/// Where a track's FLAC file is served from.
pub fn stream_url(id: &TrackId) -> String {
    format!("{API}/track/{}", raw_id(id))
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// One track from a search or release response; `None` for ones that can't be played.
pub fn slim_track(item: &Value) -> Option<Track> {
    if item["playable"].as_bool() == Some(false) {
        return None;
    }
    let id = str_of(&item["trackId"]).or_else(|| str_of(&item["id"]))?;
    let artists: Vec<String> = match item["artistNames"].as_array() {
        Some(names) => names
            .iter()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect(),
        None => item["artists"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                a["displayName"]
                    .as_str()
                    .or_else(|| a["name"].as_str())
                    .map(str::to_string)
            })
            .collect(),
    };
    let artist_id = item["artistIds"]
        .get(0)
        .and_then(str_of)
        .or_else(|| item["artists"][0]["artistId"].as_str().map(str::to_string));
    // Milliseconds, though the web client also accepts seconds for small values.
    let duration = match item["duration"].as_u64().unwrap_or(0) {
        d if d > 1000 => d,
        d => d * 1000,
    };
    let artwork = item["artwork"]
        .as_str()
        .or_else(|| item["cover"].as_str())
        .unwrap_or_default()
        .to_string();
    let album = item["albumTitle"]
        .as_str()
        .or_else(|| item["releaseTitle"].as_str())
        .unwrap_or_default()
        .to_string();
    let mut extra = serde_json::Map::new();
    if let Some(isrc) = item["isrc"].as_str().filter(|s| !s.is_empty()) {
        extra.insert("isrc".into(), Value::String(isrc.into()));
    }
    Some(Track {
        id: TrackId::Str(format!("{ID_PREFIX}{id}")),
        source: Source::Monochrome,
        title: item["title"]
            .as_str()
            .unwrap_or("Unknown title")
            .to_string(),
        artist: if artists.is_empty() {
            "Unknown artist".into()
        } else {
            artists.join(", ")
        },
        album,
        thumb: artwork.clone(),
        artwork,
        duration,
        // The web player links by TIDAL id, not this catalog's, so there is no page to open.
        permalink: None,
        streamable: true,
        folder: None,
        lrc_path: None,
        uri: None,
        artist_id,
        album_id: str_of(&item["releaseId"]),
        video_id: None,
        extra,
    })
}

fn slim_all(list: &Value) -> Vec<Track> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(slim_track)
        .collect()
}

impl Services {
    async fn mono_get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let res = self
            .http()
            .get(format!("{API}{path}"))
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            bail!("Monochrome didn't answer ({status})");
        }
        Ok(res.json().await?)
    }

    /// Songs matching `query`. The catalog's own search is loose (it pads results with
    /// popular songs that only share a word), so a wider page goes through the ranker.
    pub async fn mono_search(&self, query: &str) -> Result<Vec<Track>> {
        let json = self
            .mono_get(
                "/search/tracks",
                &[
                    ("q", query.to_string()),
                    ("limit", (SEARCH_LIMIT * 2).to_string()),
                ],
            )
            .await?;
        let tracks = slim_all(&json["tracks"]);
        Ok(crate::ranking::refine(
            query,
            tracks,
            &[],
            &[],
            2.0,
            SEARCH_LIMIT,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_hits_become_tracks() {
        let hit = json!({
            "id": "155102871061270528", "trackId": "155102871061270528",
            "title": "One More Time", "artistIds": ["153542153123926016"],
            "artistNames": ["Daft Punk"], "releaseId": "155102851176075264",
            "artwork": "https://tracks.monochrome.st/proxy/mi/155102851176075264-1A01.jpg",
            "explicit": false, "playable": true, "duration": 320357,
            "isrc": "GBDUW0000053", "recordingId": "156636455916015616"
        });
        let t = slim_track(&hit).unwrap();
        assert_eq!(t.id.key(), "mono:155102871061270528");
        assert_eq!(t.source, Source::Monochrome);
        assert_eq!(t.artist, "Daft Punk");
        assert_eq!(t.duration, 320357);
        assert_eq!(t.album_id.as_deref(), Some("155102851176075264"));
        assert_eq!(
            stream_url(&t.id),
            "https://tracks.monochrome.st/track/155102871061270528"
        );
    }

    #[test]
    fn release_tracks_use_artist_objects_and_skip_unplayable() {
        let list = json!([
            {"trackId": "1", "title": "A", "duration": 200000, "playable": true,
             "artists": [{"artistId": "9", "name": "daftpunk", "displayName": "Daft Punk"}]},
            {"trackId": "2", "title": "B", "duration": 200000, "playable": false}
        ]);
        let tracks = slim_all(&list);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].artist, "Daft Punk");
        assert_eq!(tracks[0].artist_id.as_deref(), Some("9"));
    }
}
