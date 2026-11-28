//! SoundCloud's side of `crate::catalog`: users stand in for artists, and sets are albums or
//! playlists. A set lists only its first few tracks in full; the rest come back as bare ids
//! and are looked up in batches.

use std::collections::HashMap;

use anyhow::Result;
use mosaic_core::model::Source;
use serde_json::Value;

use super::tracks::{playable, slim_track};
use crate::catalog::{Card, CardKind, Collection, Found};
use crate::Services;

const API: &str = "https://api-v2.soundcloud.com";
const CARDS_PER_KIND: u32 = 12;
/// `/tracks?ids=` takes up to 50 ids.
const IDS_PER_LOOKUP: usize = 50;

/// SoundCloud art comes as `-large` (100 px); ask for a size that fills a card.
fn card_art(url: &str) -> String {
    url.replacen("-large", "-t300x300", 1)
}

pub fn user_card(u: &Value) -> Option<Card> {
    let id = u["id"].as_u64()?;
    let name = u["username"].as_str().filter(|s| !s.is_empty())?;
    let followers = u["followers_count"].as_u64();
    Some(Card {
        source: Source::SoundCloud,
        kind: CardKind::Artist,
        id: id.to_string(),
        title: name.to_string(),
        subtitle: match followers {
            Some(n) if n > 0 => format!("{} followers", compact(n)),
            _ => "Artist".into(),
        },
        image: card_art(u["avatar_url"].as_str().unwrap_or_default()),
    })
}

pub fn set_card(s: &Value) -> Option<Card> {
    let id = s["id"].as_u64()?;
    let title = s["title"].as_str().filter(|s| !s.is_empty())?;
    let owner = s["user"]["username"].as_str().unwrap_or_default();
    let album = s["is_album"].as_bool() == Some(true);
    let subtitle = if album {
        let year: String = s["release_date"]
            .as_str()
            .or_else(|| s["published_at"].as_str())
            .or_else(|| s["created_at"].as_str())
            .unwrap_or_default()
            .chars()
            .take(4)
            .collect();
        let kind = match s["set_type"].as_str() {
            Some("ep") => "EP",
            Some("single") => "Single",
            _ => owner,
        };
        [year.as_str(), kind]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        owner.to_string()
    };
    // A set without art of its own shows its first track's.
    let art = s["artwork_url"]
        .as_str()
        .or_else(|| s["tracks"][0]["artwork_url"].as_str())
        .or_else(|| s["user"]["avatar_url"].as_str())
        .unwrap_or_default();
    Some(Card {
        source: Source::SoundCloud,
        kind: if album {
            CardKind::Album
        } else {
            CardKind::Playlist
        },
        id: id.to_string(),
        title: title.to_string(),
        subtitle,
        image: card_art(art),
    })
}

fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{}K", (n as f64 / 1e3).round())
    } else {
        n.to_string()
    }
}

fn cards(json: &Value, to_card: fn(&Value) -> Option<Card>) -> Vec<Card> {
    json["collection"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(to_card)
        .collect()
}

/// The ids of a set's tracks that came back without their details, in order.
pub fn missing_ids(set: &Value) -> Vec<u64> {
    set["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["title"].is_null())
        .filter_map(|t| t["id"].as_u64())
        .collect()
}

impl Services {
    pub async fn sc_search_all(&self, query: &str) -> Result<Found> {
        let params = [
            ("q", query.to_string()),
            ("limit", CARDS_PER_KIND.to_string()),
        ];
        let url = |what: &str| format!("{API}/search/{what}");
        let (users_url, albums_url, lists_url) =
            (url("users"), url("albums"), url("playlists_without_albums"));
        let (tracks, users, albums, playlists) = tokio::join!(
            self.sc_search(query, 30),
            self.sc_get(&users_url, &params),
            self.sc_get(&albums_url, &params),
            self.sc_get(&lists_url, &params),
        );
        let or_empty = |r: Result<Value>, f: fn(&Value) -> Option<Card>| {
            r.map(|v| cards(&v, f))
                .inspect_err(|e| tracing::warn!("SoundCloud catalog search: {e}"))
                .unwrap_or_default()
        };
        Ok(Found {
            tracks: tracks?,
            artists: or_empty(users, user_card),
            albums: or_empty(albums, set_card),
            playlists: or_empty(playlists, set_card),
        })
    }

    /// The tracks of a set in its own order, with the ones it left bare looked up.
    async fn sc_set_tracks(&self, set: &Value) -> Result<Vec<mosaic_core::model::Track>> {
        let mut full: HashMap<u64, Value> = set["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| !t["title"].is_null())
            .filter_map(|t| Some((t["id"].as_u64()?, t.clone())))
            .collect();
        for chunk in missing_ids(set).chunks(IDS_PER_LOOKUP) {
            let ids = chunk
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let found = self
                .sc_get(&format!("{API}/tracks"), &[("ids", ids)])
                .await?;
            for t in found.as_array().into_iter().flatten() {
                if let Some(id) = t["id"].as_u64() {
                    full.insert(id, t.clone());
                }
            }
        }
        Ok(set["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| full.get(&t["id"].as_u64()?))
            .filter_map(slim_track)
            .filter(|t| t.streamable)
            .collect())
    }

    pub async fn sc_open(&self, card: &Card) -> Result<Collection> {
        match card.kind {
            CardKind::Artist => {
                let limit = [("limit", "20".to_string())];
                let user_url = format!("{API}/users/{}", card.id);
                let (top_url, albums_url) = (
                    format!("{user_url}/toptracks"),
                    format!("{user_url}/albums"),
                );
                let (top, albums, user) = tokio::join!(
                    self.sc_get(&top_url, &limit),
                    self.sc_get(&albums_url, &limit),
                    self.sc_get(&user_url, &[]),
                );
                let top = top?;
                let mut tracks = playable(top.get("collection"));
                if tracks.is_empty() {
                    // Some accounts have no "popular" list; their uploads stand in.
                    let uploads = self
                        .sc_get(&format!("{API}/users/{}/tracks", card.id), &limit)
                        .await?;
                    tracks = playable(uploads.get("collection"));
                }
                let header = user.ok().as_ref().and_then(user_card);
                Ok(Collection {
                    card: header.unwrap_or_else(|| card.clone()),
                    tracks,
                    albums: albums.map(|a| cards(&a, set_card)).unwrap_or_default(),
                    artist: None,
                })
            }
            CardKind::Album | CardKind::Playlist => {
                let set = self
                    .sc_get(&format!("{API}/playlists/{}", card.id), &[])
                    .await?;
                let tracks = self.sc_set_tracks(&set).await?;
                let artist = (card.kind == CardKind::Album)
                    .then(|| user_card(&set["user"]))
                    .flatten();
                Ok(Collection {
                    card: set_card(&set).unwrap_or_else(|| card.clone()),
                    tracks,
                    albums: Vec::new(),
                    artist,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn users_become_artist_cards() {
        let u = json!({"id": 7, "username": "Daft Punk", "followers_count": 1_260_000,
                       "avatar_url": "https://i1.sndcdn.com/avatars-x-large.jpg"});
        let c = user_card(&u).unwrap();
        assert_eq!((c.kind, c.id.as_str()), (CardKind::Artist, "7"));
        assert_eq!(c.subtitle, "1.3M followers");
        assert_eq!(c.image, "https://i1.sndcdn.com/avatars-x-t300x300.jpg");
        assert!(user_card(&json!({"id": 1})).is_none());
    }

    #[test]
    fn sets_are_albums_or_playlists() {
        let album = json!({"id": 1, "title": "Homework", "is_album": true, "set_type": "album",
                           "release_date": "1997-01-20T00:00:00Z",
                           "user": {"username": "Daft Punk"}, "artwork_url": null,
                           "tracks": [{"id": 5, "artwork_url": "https://i1.sndcdn.com/a-large.jpg"}]});
        let c = set_card(&album).unwrap();
        assert_eq!(c.kind, CardKind::Album);
        assert_eq!(c.subtitle, "1997 · Daft Punk");
        assert_eq!(c.image, "https://i1.sndcdn.com/a-t300x300.jpg");
        let ep = json!({"id": 2, "title": "E", "is_album": true, "set_type": "ep",
                        "published_at": "2020-05-01", "user": {"username": "X"}});
        assert_eq!(set_card(&ep).unwrap().subtitle, "2020 · EP");
        let list = json!({"id": 3, "title": "Mix", "is_album": false, "user": {"username": "dj"}});
        let c = set_card(&list).unwrap();
        assert_eq!((c.kind, c.subtitle.as_str()), (CardKind::Playlist, "dj"));
    }

    #[test]
    fn only_bare_set_tracks_are_looked_up() {
        let set = json!({"tracks": [
            {"id": 1, "title": "Full"}, {"id": 2, "kind": "track"}, {"id": 3}
        ]});
        assert_eq!(missing_ids(&set), [2, 3]);
    }
}
