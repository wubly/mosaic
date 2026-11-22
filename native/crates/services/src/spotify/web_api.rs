//! Non-playback parts of `electron/spotify/sdk.js`. The mappers are copied from the JS.

use anyhow::Result;
use mosaic_core::model::Track;
use serde_json::{json, Value};

use crate::Services;

#[derive(Clone, Debug, PartialEq)]
pub struct AlbumCard {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub image: String,
    pub year: String,
    pub kind: String,
    pub total: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtistCard {
    pub id: String,
    pub name: String,
    pub image: String,
    pub followers: Option<u64>,
    pub genres: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub artists: Vec<ArtistCard>,
    pub albums: Vec<AlbumCard>,
    pub playlists: Vec<super::library::PlaylistCard>,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub saved: Vec<Track>,
    pub albums: Vec<AlbumCard>,
    pub artists: Vec<ArtistCard>,
}

/// Everything an account holds, for a one-shot import into Mosaic.
#[derive(Clone, Debug, Default)]
pub struct Everything {
    pub liked: Vec<Track>,
    pub playlists: Vec<(String, Vec<Track>)>,
    pub top: Vec<Track>,
    /// Newest first, with the epoch-millisecond play time.
    pub recent: Vec<(Track, i64)>,
    pub mix: Vec<Track>,
}

#[derive(Clone, Debug)]
pub struct ArtistPage {
    pub artist: ArtistCard,
    pub top: Vec<Track>,
    pub albums: Vec<AlbumCard>,
}

#[derive(Clone, Debug)]
pub struct AlbumPage {
    pub album: AlbumCard,
    pub tracks: Vec<Track>,
}

pub(crate) fn image(list: &Value, i: usize) -> String {
    let Some(list) = list.as_array().filter(|l| !l.is_empty()) else {
        return String::new();
    };
    list[i.min(list.len() - 1)]["url"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

pub(crate) fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

pub fn slim(t: &Value) -> Track {
    let artists: Vec<&str> = t["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect();
    let mut v = json!({
        "id": format!("sp:{}", text(&t["id"])),
        "source": "spotify",
        "uri": t["uri"],
        "title": text(&t["name"]),
        "artist": if artists.is_empty() { "Unknown".to_string() } else { artists.join(", ") },
        "album": text(&t["album"]["name"]),
        "artwork": image(&t["album"]["images"], 0),
        "thumb": image(&t["album"]["images"], 1),
        "duration": t["duration_ms"].as_u64().unwrap_or(0),
        "streamable": t["is_playable"].as_bool() != Some(false),
    });
    if let Some(id) = t["artists"][0]["id"].as_str() {
        v["artistId"] = json!(id);
    }
    if let Some(id) = t["album"]["id"].as_str() {
        v["albumId"] = json!(id);
    }
    if let Some(url) = t["external_urls"]["spotify"].as_str() {
        v["permalink"] = json!(url);
    }
    serde_json::from_value(v).unwrap_or_else(|_| unreachable!("slim builds a valid track"))
}

pub fn album_card(a: &Value) -> AlbumCard {
    let artists: Vec<&str> = a["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x["name"].as_str())
        .collect();
    AlbumCard {
        id: text(&a["id"]),
        name: text(&a["name"]),
        artist: artists.join(", "),
        artist_id: a["artists"][0]["id"].as_str().map(str::to_string),
        image: image(&a["images"], 1),
        year: text(&a["release_date"]).chars().take(4).collect(),
        kind: text(&a["album_type"]),
        total: None,
    }
}

pub fn artist_card(a: &Value) -> ArtistCard {
    ArtistCard {
        id: text(&a["id"]),
        name: text(&a["name"]),
        image: image(&a["images"], 1),
        followers: a["followers"]["total"].as_u64(),
        genres: a["genres"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_str().map(str::to_string))
            .collect(),
    }
}

/// Playable tracks only (`usable` in the JS).
pub fn usable<'a>(list: impl IntoIterator<Item = &'a Value>) -> Vec<Track> {
    list.into_iter()
        .filter(|t| t["id"].as_str().is_some_and(|s| !s.is_empty()) && t["uri"].as_str().is_some())
        .map(slim)
        .filter(|t| t.streamable)
        .collect()
}

pub(crate) fn items(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

impl Services {
    pub async fn spotify_search(&self, q: &str, limit: u32) -> Result<SearchResults> {
        let json = self
            .spotify_api(
                "/search",
                &[
                    ("q", q.into()),
                    ("type", "track,artist,album,playlist".into()),
                    ("limit", limit.to_string()),
                ],
            )
            .await?;
        Ok(SearchResults {
            tracks: usable(items(&json["tracks"]["items"])),
            artists: items(&json["artists"]["items"])
                .filter(|a| !a.is_null())
                .take(10)
                .map(artist_card)
                .collect(),
            albums: items(&json["albums"]["items"])
                .filter(|a| !a.is_null())
                .take(10)
                .map(album_card)
                .collect(),
            playlists: items(&json["playlists"]["items"])
                .filter(|p| !p.is_null())
                .take(10)
                .map(super::library::playlist_card)
                .collect(),
        })
    }

    pub async fn spotify_library(&self) -> Library {
        let limit = [("limit", "30".to_string())];
        let following = [("type", "artist".to_string()), ("limit", "30".to_string())];
        let (saved, albums, artists) = futures::join!(
            self.spotify_api("/me/tracks", &limit),
            self.spotify_api("/me/albums", &limit),
            self.spotify_api("/me/following", &following),
        );
        let saved = saved.unwrap_or(Value::Null);
        let albums = albums.unwrap_or(Value::Null);
        let artists = artists.unwrap_or(Value::Null);
        Library {
            saved: usable(items(&saved["items"]).map(|i| &i["track"])),
            albums: items(&albums["items"])
                .map(|i| album_card(&i["album"]))
                .collect(),
            artists: items(&artists["artists"]["items"])
                .map(artist_card)
                .collect(),
        }
    }

    /// Follows `next` links from `first`, collecting each page's `items`.
    async fn spotify_pages(&self, path: &str, query: &[(&str, String)], cap: usize) -> Vec<Value> {
        let mut out = Vec::new();
        let mut page = self.spotify_api(path, query).await.unwrap_or(Value::Null);
        loop {
            out.extend(items(&page["items"]).cloned());
            let next = page["next"].as_str().map(str::to_string);
            match next {
                Some(url) if out.len() < cap => {
                    page = self.spotify_api(&url, &[]).await.unwrap_or(Value::Null);
                }
                _ => break,
            }
        }
        out
    }

    /// Liked songs, playlists, top tracks, listening history and a mix built from top artists.
    /// Each part degrades to empty when the token lacks its scope.
    pub async fn spotify_everything(&self) -> Everything {
        let market = self.spotify_country();
        let liked = self
            .spotify_pages("/me/tracks", &[("limit", "50".into())], 5000)
            .await;
        let liked = usable(liked.iter().map(|i| &i["track"]));
        let lists = self
            .spotify_pages("/me/playlists", &[("limit", "50".into())], 200)
            .await;
        let mut playlists = Vec::new();
        for l in lists.iter().take(60) {
            let (Some(id), Some(name)) = (l["id"].as_str(), l["name"].as_str()) else {
                continue;
            };
            let rows = self
                .spotify_pages(
                    &format!("/playlists/{id}/tracks"),
                    &[("limit", "100".into()), ("market", market.clone())],
                    3000,
                )
                .await;
            let tracks = usable(rows.iter().map(|i| &i["track"]));
            if !tracks.is_empty() {
                playlists.push((name.to_string(), tracks));
            }
        }
        let top = self
            .spotify_api(
                "/me/top/tracks",
                &[("limit", "50".into()), ("time_range", "medium_term".into())],
            )
            .await
            .unwrap_or(Value::Null);
        let top = usable(items(&top["items"]));
        let recent = self
            .spotify_api("/me/player/recently-played", &[("limit", "50".into())])
            .await
            .unwrap_or(Value::Null);
        let recent = items(&recent["items"])
            .filter(|i| i["track"]["name"].as_str().is_some())
            .map(|i| {
                let at =
                    chrono::DateTime::parse_from_rfc3339(i["played_at"].as_str().unwrap_or(""))
                        .map_or(0, |d| d.timestamp_millis());
                (slim(&i["track"]), at)
            })
            .collect();
        let artists = self
            .spotify_api(
                "/me/top/artists",
                &[("limit", "10".into()), ("time_range", "medium_term".into())],
            )
            .await
            .unwrap_or(Value::Null);
        let mut mix = Vec::new();
        let mut seen: std::collections::HashSet<_> = top.iter().map(|t| t.id.clone()).collect();
        for a in items(&artists["items"]) {
            if let Some(id) = a["id"].as_str() {
                if let Ok(page) = self.spotify_artist(id).await {
                    for t in page.top.into_iter().take(4) {
                        if seen.insert(t.id.clone()) {
                            mix.push(t);
                        }
                    }
                }
            }
        }
        Everything {
            liked,
            playlists,
            top,
            recent,
            mix,
        }
    }

    async fn spotify_artist_albums(&self, id: &str) -> Vec<AlbumCard> {
        let json = self
            .spotify_api(
                &format!("/artists/{id}/albums"),
                &[
                    ("include_groups", "album,single".into()),
                    ("limit", "50".into()),
                    ("market", self.spotify_country()),
                ],
            )
            .await
            .unwrap_or(Value::Null);
        let mut seen = std::collections::HashSet::new();
        items(&json["items"])
            .filter(|a| seen.insert(text(&a["name"]).to_lowercase()))
            .map(album_card)
            .collect()
    }

    /// Full tracks for `spotify:track:` URIs, in order; ones that fail to load are skipped.
    pub async fn spotify_tracks(&self, uris: &[String]) -> Vec<Track> {
        let market = self.spotify_country();
        let fetches = uris.iter().map(|uri| {
            let id = uri.rsplit(':').next().unwrap_or_default().to_string();
            let market = market.clone();
            async move {
                self.spotify_api(&format!("/tracks/{id}"), &[("market", market)])
                    .await
                    .ok()
            }
        });
        futures::future::join_all(fetches)
            .await
            .into_iter()
            .flatten()
            .filter(|t| t["id"].is_string())
            .map(|t| slim(&t))
            .collect()
    }

    pub async fn spotify_artist(&self, id: &str) -> Result<ArtistPage> {
        let market = [("market", self.spotify_country())];
        let (artist_path, top_path) = (
            format!("/artists/{id}"),
            format!("/artists/{id}/top-tracks"),
        );
        let (artist, top, albums) = futures::join!(
            self.spotify_api(&artist_path, &[]),
            self.spotify_api(&top_path, &market),
            self.spotify_artist_albums(id),
        );
        let top = top.unwrap_or(Value::Null);
        Ok(ArtistPage {
            artist: artist_card(&artist?),
            top: usable(items(&top["tracks"])),
            albums,
        })
    }

    pub async fn spotify_album(&self, id: &str) -> Result<AlbumPage> {
        let album = self
            .spotify_api(
                &format!("/albums/{id}"),
                &[("market", self.spotify_country())],
            )
            .await?;
        let tracks: Vec<Value> = items(&album["tracks"]["items"])
            .map(|t| {
                let mut t = t.clone();
                t["album"] = album.clone();
                t
            })
            .collect();
        let mut card = album_card(&album);
        card.image = image(&album["images"], 0);
        card.total = album["total_tracks"].as_u64();
        Ok(AlbumPage {
            album: card,
            tracks: usable(&tracks),
        })
    }

    /// Full track list of a playlist, following pagination.
    pub async fn spotify_playlist_tracks(&self, id: &str) -> Result<super::embed::Imported> {
        let head = self
            .spotify_api(
                &format!("/playlists/{id}"),
                &[("fields", "name,owner(display_name),images".into())],
            )
            .await?;
        let mut tracks = Vec::new();
        let mut offset = 0;
        loop {
            let page = self
                .spotify_api(
                    &format!("/playlists/{id}/tracks"),
                    &[
                        ("limit", "100".into()),
                        ("offset", offset.to_string()),
                        ("market", self.spotify_country()),
                    ],
                )
                .await?;
            let batch: Vec<_> = items(&page["items"])
                .map(|i| &i["track"])
                .filter(|t| t["name"].as_str().is_some())
                .collect();
            tracks.extend(batch.iter().map(|t| remote(t)));
            if page["next"].is_null() || page["items"].as_array().is_none_or(Vec::is_empty) {
                break;
            }
            offset += 100;
        }
        Ok(super::embed::Imported {
            via: "api",
            name: head["name"]
                .as_str()
                .unwrap_or("Spotify playlist")
                .to_string(),
            owner: text(&head["owner"]["display_name"]),
            cover: image(&head["images"], 0),
            tracks,
        })
    }

    pub async fn spotify_album_tracks(&self, id: &str) -> Result<super::embed::Imported> {
        let album = self
            .spotify_api(
                &format!("/albums/{id}"),
                &[("market", self.spotify_country())],
            )
            .await?;
        let mut tracks: Vec<_> = items(&album["tracks"]["items"]).map(remote).collect();
        let mut next = album["tracks"]["next"].as_str().map(str::to_string);
        while let Some(url) = next {
            let page = self.spotify_api(&url, &[]).await?;
            tracks.extend(items(&page["items"]).map(remote));
            next = page["next"].as_str().map(str::to_string);
        }
        Ok(super::embed::Imported {
            via: "api",
            name: text(&album["name"]),
            owner: album_card(&album).artist,
            cover: image(&album["images"], 0),
            tracks,
        })
    }
}

fn remote(t: &Value) -> super::embed::RemoteTrack {
    let artists: Vec<&str> = t["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect();
    super::embed::RemoteTrack {
        title: text(&t["name"]),
        artist: artists.join(", "),
        duration: t["duration_ms"].as_u64().unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_json() -> Value {
        json!({
            "id": "4uLU6hMCjMI75M1A2tKUQC", "uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC", "name": "Never Gonna Give You Up",
            "artists": [{"id": "a1", "name": "Rick Astley"}, {"id": "a2", "name": "Guest"}],
            "album": {"id": "al1", "name": "Whenever You Need Somebody",
                "images": [{"url": "big"}, {"url": "mid"}, {"url": "small"}]},
            "duration_ms": 213573, "external_urls": {"spotify": "https://open.spotify.com/track/x"}
        })
    }

    #[test]
    fn slim_maps_the_js_fields() {
        let t = slim(&track_json());
        assert_eq!(t.id.key(), "sp:4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(t.artist, "Rick Astley, Guest");
        assert_eq!((t.artwork.as_str(), t.thumb.as_str()), ("big", "mid"));
        assert_eq!(t.duration, 213_573);
        assert_eq!(t.artist_id.as_deref(), Some("a1"));
        assert_eq!(t.album_id.as_deref(), Some("al1"));
        assert_eq!(
            t.uri.as_deref(),
            Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC")
        );
        assert!(t.streamable);
    }

    #[test]
    fn usable_drops_unplayable_and_incomplete_tracks() {
        let mut blocked = track_json();
        blocked["is_playable"] = json!(false);
        let no_uri = json!({"id": "x", "name": "n"});
        let list = [track_json(), blocked, no_uri, Value::Null];
        assert_eq!(usable(&list).len(), 1);
    }

    #[test]
    fn cards_use_the_second_image_and_year_prefix() {
        let album = json!({"id": "a", "name": "N", "artists": [{"id": "x", "name": "A"}], "images": [{"url": "1"}, {"url": "2"}], "release_date": "1987-11-12", "album_type": "album"});
        let c = album_card(&album);
        assert_eq!(
            (c.image.as_str(), c.year.as_str(), c.kind.as_str()),
            ("2", "1987", "album")
        );
        let a = artist_card(
            &json!({"id": "x", "name": "N", "images": [{"url": "only"}], "followers": {"total": 5}, "genres": ["pop"]}),
        );
        assert_eq!(
            (a.image.as_str(), a.followers, a.genres.len()),
            ("only", Some(5), 1)
        );
        assert_eq!(artist_card(&json!({"id": "x", "name": "N"})).image, "");
    }
}
