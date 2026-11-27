//! YouTube Data API v3 calls for browsing: search, liked videos, playlists (port of
//! `electron/youtube/ipc.js`). Playback doesn't go through here, see `innertube`.

use std::time::Duration;

use anyhow::{bail, Result};
use mosaic_core::model::{Source, Track, TrackId};
use serde_json::Value;

use crate::Services;

#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistCard {
    pub id: String,
    pub name: String,
    pub count: u64,
    pub image: String,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub liked: Vec<Track>,
    pub playlists: Vec<PlaylistCard>,
}

/// `PT3M33S` / `P1DT2H` to seconds.
pub fn iso_seconds(iso: &str) -> u64 {
    let Some(rest) = iso.strip_prefix('P') else {
        return 0;
    };
    let (mut total, mut number) = (0u64, 0u64);
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number = number * 10 + u64::from(c as u8 - b'0'),
            _ => {
                total += number
                    * match (c, in_time) {
                        ('D', _) => 86_400,
                        ('H', true) => 3600,
                        ('M', true) => 60,
                        ('S', true) => 1,
                        _ => 0,
                    };
                number = 0;
            }
        }
    }
    total
}

fn decode_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// A `videos` resource as a track; `None` for ones that can't be played (live, no length).
pub fn slim_video(v: &Value) -> Option<Track> {
    let id = v["id"].as_str()?;
    let snippet = &v["snippet"];
    if snippet["liveBroadcastContent"].as_str() == Some("live") {
        return None;
    }
    let duration = iso_seconds(v["contentDetails"]["duration"].as_str().unwrap_or("")) * 1000;
    if duration == 0 {
        return None;
    }
    let thumbs = &snippet["thumbnails"];
    let url = |k: &str| thumbs[k]["url"].as_str().filter(|s| !s.is_empty());
    let best = url("maxres")
        .or_else(|| url("standard"))
        .or_else(|| url("high"))
        .or_else(|| url("medium"))
        .or_else(|| url("default"))
        .unwrap_or_default();
    let channel = decode_entities(snippet["channelTitle"].as_str().unwrap_or(""));
    Some(youtube_track(
        id,
        decode_entities(snippet["title"].as_str().unwrap_or("")),
        &channel,
        duration,
        best,
        url("medium").unwrap_or(best),
    ))
}

/// A YouTube video as a track. A "- Topic" suffix on the channel (auto-generated music
/// channels) is dropped so the artist reads cleanly.
pub fn youtube_track(
    id: &str,
    title: String,
    channel: &str,
    duration_ms: u64,
    artwork: &str,
    thumb: &str,
) -> Track {
    let artist = channel.strip_suffix(" - Topic").unwrap_or(channel);
    Track {
        id: TrackId::Str(format!("yt:{id}")),
        source: Source::YouTube,
        title,
        artist: if artist.is_empty() {
            "YouTube".into()
        } else {
            artist.to_string()
        },
        album: String::new(),
        artwork: artwork.to_string(),
        thumb: thumb.to_string(),
        duration: duration_ms,
        permalink: Some(format!("https://www.youtube.com/watch?v={id}")),
        streamable: true,
        folder: None,
        lrc_path: None,
        uri: None,
        artist_id: None,
        album_id: None,
        video_id: Some(id.to_string()),
        extra: Default::default(),
    }
}

fn slim_all(json: &Value) -> Vec<Track> {
    json["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(slim_video)
        .collect()
}

const SEARCH_LIMIT: usize = 30;

impl Services {
    /// GET against the Data API with the signed-in account.
    pub(crate) async fn yt_api(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let res = self
            .http()
            .get(format!("https://www.googleapis.com/youtube/v3{path}"))
            .bearer_auth(self.yt_token().await?)
            .query(query)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let status = res.status();
        let json: Value = res.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            if json["error"]["errors"][0]["reason"].as_str() == Some("quotaExceeded") {
                bail!("YouTube’s daily quota is used up. Try again tomorrow.");
            }
            bail!(json["error"]["message"].as_str().map_or_else(
                || format!("YouTube said {}", status.as_u16()),
                str::to_string
            ));
        }
        Ok(json)
    }

    /// Playable details for `ids`, in the order given.
    async fn yt_details(&self, ids: Vec<String>) -> Result<Vec<Track>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let json = self
            .yt_api(
                "/videos",
                &[
                    ("part", "snippet,contentDetails,status".into()),
                    ("id", ids.join(",")),
                    ("maxResults", "50".into()),
                ],
            )
            .await?;
        let mut by_id: std::collections::HashMap<String, Track> = slim_all(&json)
            .into_iter()
            .filter_map(|t| Some((t.video_id.clone()?, t)))
            .collect();
        Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
    }

    /// Searches YouTube Music for songs, falling back to the website's video search if the
    /// music endpoint fails. Signed in or not, it doesn't touch the Data API quota.
    pub async fn yt_search(&self, query: &str) -> Result<Vec<Track>> {
        use crate::ranking::refine;
        match self.yt_search_music_counted(query).await {
            // YouTube Music's own order is good; this tidies it, de-duplicates it and lets
            // play counts put the original above its covers.
            Ok(counted) => {
                let (tracks, plays): (Vec<_>, Vec<_>) = counted.into_iter().unzip();
                Ok(refine(query, tracks, &plays, &[], 3.0, SEARCH_LIMIT))
            }
            Err(e) => {
                tracing::warn!("YouTube Music search failed, using website search: {e}");
                let tracks = self.yt_search_public(query).await?;
                Ok(refine(query, tracks, &[], &[], 1.0, SEARCH_LIMIT))
            }
        }
    }

    pub async fn yt_library(&self) -> Library {
        let liked_query = [
            ("part", "snippet,contentDetails,status".to_string()),
            ("myRating", "like".into()),
            ("maxResults", "30".into()),
        ];
        let lists_query = [
            ("part", "snippet,contentDetails".to_string()),
            ("mine", "true".into()),
            ("maxResults", "25".into()),
        ];
        let (liked, lists) = tokio::join!(
            self.yt_api("/videos", &liked_query),
            self.yt_api("/playlists", &lists_query),
        );
        Library {
            liked: liked.map(|j| slim_all(&j)).unwrap_or_default(),
            playlists: lists
                .map(|j| {
                    j["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|p| PlaylistCard {
                            id: p["id"].as_str().unwrap_or_default().to_string(),
                            name: p["snippet"]["title"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            count: p["contentDetails"]["itemCount"].as_u64().unwrap_or(0),
                            image: p["snippet"]["thumbnails"]["medium"]["url"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub async fn yt_playlist(&self, id: &str) -> Result<Vec<Track>> {
        let json = self
            .yt_api(
                "/playlistItems",
                &[
                    ("part", "contentDetails".into()),
                    ("playlistId", id.to_string()),
                    ("maxResults", "50".into()),
                ],
            )
            .await?;
        let ids = json["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["contentDetails"]["videoId"].as_str().map(str::to_string))
            .collect();
        self.yt_details(ids).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso_durations_become_seconds() {
        assert_eq!(iso_seconds("PT3M33S"), 213);
        assert_eq!(iso_seconds("PT1H2M3S"), 3723);
        assert_eq!(iso_seconds("P1DT1S"), 86_401);
        assert_eq!(iso_seconds("PT45S"), 45);
        assert_eq!(iso_seconds("P0D"), 0);
        assert_eq!(iso_seconds(""), 0);
    }

    fn video() -> Value {
        json!({
            "id": "dQw4w9WgXcQ",
            "snippet": {
                "title": "Rick &amp; Roll &#39;87",
                "channelTitle": "Rick Astley - Topic",
                "liveBroadcastContent": "none",
                "thumbnails": {"medium": {"url": "https://i/m.jpg"}, "high": {"url": "https://i/h.jpg"}}
            },
            "contentDetails": {"duration": "PT3M33S"}
        })
    }

    #[test]
    fn videos_become_youtube_tracks() {
        let t = slim_video(&video()).expect("track");
        assert_eq!(t.id.key(), "yt:dQw4w9WgXcQ");
        assert_eq!(t.source, Source::YouTube);
        assert_eq!(t.title, "Rick & Roll '87");
        assert_eq!(t.artist, "Rick Astley");
        assert_eq!(t.duration, 213_000);
        assert_eq!(t.artwork, "https://i/h.jpg");
        assert_eq!(t.thumb, "https://i/m.jpg");
        assert_eq!(t.video_id.as_deref(), Some("dQw4w9WgXcQ"));
    }

    #[test]
    fn live_and_zero_length_videos_are_dropped() {
        let mut live = video();
        live["snippet"]["liveBroadcastContent"] = json!("live");
        assert!(slim_video(&live).is_none());
        let mut empty = video();
        empty["contentDetails"]["duration"] = json!("P0D");
        assert!(slim_video(&empty).is_none());
    }
}
