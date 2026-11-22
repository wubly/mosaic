use anyhow::{bail, Result};
use mosaic_core::model::{Track, TrackId};

use super::client_types::{StreamInfo, StreamKind};
use super::tracks::{full_streams, playable};
use crate::Services;

const API: &str = "https://api-v2.soundcloud.com";

impl Services {
    pub async fn sc_search(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let json = self
            .sc_get(
                &format!("{API}/search/tracks"),
                &[("q", query.into()), ("limit", limit.to_string())],
            )
            .await?;
        Ok(playable(json.get("collection")))
    }

    pub async fn sc_charts(&self, genre: &str, limit: u32) -> Result<Vec<Track>> {
        if genre == "all-music" {
            let json = self
                .sc_get(
                    &format!("{API}/charts"),
                    &[
                        ("kind", "trending".into()),
                        ("genre", "soundcloud:genres:all-music".into()),
                        ("limit", limit.to_string()),
                    ],
                )
                .await?;
            let tracks: Vec<_> = json["collection"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|entry| entry["track"].clone())
                .collect();
            return Ok(playable(Some(&serde_json::Value::Array(tracks))));
        }
        let json = self
            .sc_get(
                &format!("{API}/search/tracks"),
                &[
                    ("q", String::new()),
                    ("filter.genre_or_tag", genre.into()),
                    ("filter.created_at", "last_month".into()),
                    ("sort", "popular".into()),
                    ("limit", limit.to_string()),
                ],
            )
            .await?;
        Ok(playable(json.get("collection")))
    }

    pub async fn sc_related(&self, id: &TrackId, limit: u32) -> Result<Vec<Track>> {
        let json = self
            .sc_get(
                &format!("{API}/tracks/{}/related", id.key()),
                &[("limit", limit.to_string())],
            )
            .await?;
        Ok(playable(json.get("collection")))
    }

    pub async fn sc_stream(&self, id: &TrackId) -> Result<StreamInfo> {
        let track = self
            .sc_get(&format!("{API}/tracks/{}", id.key()), &[])
            .await?;
        let params: Vec<(&str, String)> = track["track_authorization"]
            .as_str()
            .map(|a| vec![("track_authorization", a.to_string())])
            .unwrap_or_default();

        let silence = {
            let this = self.clone();
            let track = track.clone();
            tokio::spawn(async move { this.waveform_silence(&track).await })
        };

        for transcoding in full_streams(track["media"].get("transcodings")) {
            let Ok(resolved) = self.sc_get(&transcoding.url, &params).await else {
                continue;
            };
            let Some(url) = resolved["url"].as_str() else {
                continue;
            };
            return Ok(StreamInfo {
                kind: if transcoding.hls {
                    StreamKind::Hls(transcoding.codec)
                } else {
                    StreamKind::Progressive
                },
                url: url.to_string(),
                silence: silence.await.ok().flatten(),
            });
        }
        bail!("No playable stream for this track")
    }
}
