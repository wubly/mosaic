//! Port of `importPlaylist` in `electron/spotify/ipc.js`, minus the browser scrape.

use anyhow::Result;

use super::embed::Imported;
use super::url::{parse_spotify_url, Kind};
use crate::Services;

pub const EMBED_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Imported(Imported),
    BadUrl,
    Unavailable,
}

/// Picks between the embed result (≤100 tracks) and the full API list.
pub fn choose(embed: Option<Imported>, api: Option<Result<Imported, ()>>) -> Option<Imported> {
    match (embed, api) {
        (Some(e), _) if e.tracks.len() < EMBED_LIMIT => Some(e),
        (Some(e), Some(Ok(a))) if a.tracks.len() > e.tracks.len() => Some(Imported {
            cover: if e.cover.is_empty() { a.cover } else { e.cover },
            owner: if e.owner.is_empty() { a.owner } else { e.owner },
            ..a
        }),
        (Some(e), _) => Some(e),
        (None, Some(Ok(a))) if !a.tracks.is_empty() => Some(a),
        _ => None,
    }
}

impl Services {
    pub async fn spotify_import(&self, input: &str) -> Outcome {
        let Some(r) = parse_spotify_url(input) else {
            return Outcome::BadUrl;
        };
        let embed = self.spotify_embed(&r).await;
        let need_api = embed.as_ref().is_none_or(|e| e.tracks.len() >= EMBED_LIMIT);
        let api = if need_api && self.spotify_status().await.connected {
            Some(
                match r.kind {
                    Kind::Playlist => self.spotify_playlist_tracks(&r.id).await,
                    Kind::Album => self.spotify_album_tracks(&r.id).await,
                }
                .map_err(|_| ()),
            )
        } else {
            None
        };
        match choose(embed, api) {
            Some(i) => Outcome::Imported(i),
            None => Outcome::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::embed::RemoteTrack;
    use super::*;

    fn list(n: usize, via: &'static str, cover: &str) -> Imported {
        Imported {
            via,
            name: "n".into(),
            owner: String::new(),
            cover: cover.into(),
            tracks: (0..n)
                .map(|i| RemoteTrack {
                    title: format!("t{i}"),
                    artist: "a".into(),
                    duration: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn short_embed_wins() {
        assert_eq!(
            choose(Some(list(40, "embed", "")), None)
                .unwrap()
                .tracks
                .len(),
            40
        );
        assert_eq!(
            choose(Some(list(40, "embed", "")), Some(Ok(list(300, "api", ""))))
                .unwrap()
                .via,
            "embed"
        );
    }

    #[test]
    fn full_embed_defers_to_a_longer_api_list_and_keeps_the_embed_cover() {
        let got = choose(
            Some(list(100, "embed", "cover-e")),
            Some(Ok(list(250, "api", "cover-a"))),
        )
        .unwrap();
        assert_eq!(
            (got.tracks.len(), got.via, got.cover.as_str()),
            (250, "api", "cover-e")
        );
    }

    #[test]
    fn full_embed_stays_when_the_api_is_missing_or_failed() {
        assert_eq!(
            choose(Some(list(100, "embed", "")), None)
                .unwrap()
                .tracks
                .len(),
            100
        );
        assert_eq!(
            choose(Some(list(100, "embed", "")), Some(Err(())))
                .unwrap()
                .via,
            "embed"
        );
    }

    #[test]
    fn api_is_used_when_the_embed_fails_and_nothing_otherwise() {
        assert_eq!(
            choose(None, Some(Ok(list(7, "api", "")))).unwrap().via,
            "api"
        );
        assert!(choose(None, None).is_none());
        assert!(choose(None, Some(Err(()))).is_none());
        assert!(choose(None, Some(Ok(list(0, "api", "")))).is_none());
    }
}
