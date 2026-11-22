//! The live Spotify account: home shelves, playlists, liked songs, and the calls that change the
//! library (like, save, follow, add to playlist). Each read degrades to empty when a scope or
//! endpoint is unavailable, so one missing shelf never blanks the page.

use anyhow::Result;
use mosaic_core::model::Track;
use reqwest::Method;
use serde_json::{json, Value};

use super::web_api::{album_card, artist_card, image, items, text, usable, AlbumCard, ArtistCard};
use crate::Services;

#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistCard {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub owner_id: String,
    pub image: String,
    pub description: String,
    pub total: u64,
    pub collaborative: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Home {
    pub recent: Vec<Track>,
    pub top_tracks: Vec<Track>,
    pub top_artists: Vec<ArtistCard>,
    pub playlists: Vec<PlaylistCard>,
    pub albums: Vec<AlbumCard>,
    pub artists: Vec<ArtistCard>,
    pub liked: Vec<Track>,
    pub liked_total: u64,
    pub new_releases: Vec<AlbumCard>,
}

#[derive(Clone, Debug)]
pub struct PlaylistPage {
    pub playlist: PlaylistCard,
    pub tracks: Vec<Track>,
    /// The user follows (or owns) it.
    pub saved: bool,
}

/// What a save / follow toggle acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Saveable {
    Track(String),
    Album(String),
    Artist(String),
    Playlist(String),
}

pub fn playlist_card(p: &Value) -> PlaylistCard {
    PlaylistCard {
        id: text(&p["id"]),
        name: text(&p["name"]),
        owner: p["owner"]["display_name"]
            .as_str()
            .or_else(|| p["owner"]["id"].as_str())
            .unwrap_or_default()
            .to_string(),
        owner_id: text(&p["owner"]["id"]),
        image: image(&p["images"], 0),
        description: strip_tags(p["description"].as_str().unwrap_or_default()),
        total: p["tracks"]["total"]
            .as_u64()
            .or_else(|| p["items"]["total"].as_u64())
            .unwrap_or(0),
        collaborative: p["collaborative"].as_bool() == Some(true),
    }
}

/// Playlist descriptions arrive as HTML (links, entities).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&#x27;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// `sp:<id>` or a bare id to a bare id.
pub fn bare_id(id: &str) -> &str {
    id.strip_prefix("sp:").unwrap_or(id)
}

impl Services {
    /// Follows `next` links, collecting `items` until `cap`; also returns the reported total.
    async fn spotify_collect(
        &self,
        path: &str,
        query: &[(&str, String)],
        cap: usize,
    ) -> Result<(Vec<Value>, u64)> {
        let mut out = Vec::new();
        let mut page = self.spotify_api(path, query).await?;
        let mut total = 0;
        loop {
            total = page["total"].as_u64().unwrap_or(total);
            out.extend(items(&page["items"]).cloned());
            match page["next"].as_str() {
                Some(url) if out.len() < cap => page = self.spotify_api(url, &[]).await?,
                _ => break,
            }
        }
        Ok((out, total))
    }

    pub async fn spotify_home(&self) -> Home {
        let n = |v: &str| v.to_string();
        let market = self.spotify_country();
        let q_recent = [("limit", n("50"))];
        let q_top = [("limit", n("30")), ("time_range", n("short_term"))];
        let q_top_artists = [("limit", n("20")), ("time_range", n("medium_term"))];
        let q_lists = [("limit", n("50"))];
        let q_albums = [("limit", n("50")), ("market", market.clone())];
        let q_following = [("type", n("artist")), ("limit", n("50"))];
        let q_liked = [("limit", n("30")), ("market", market)];
        let q_releases = [("limit", n("30"))];
        let (recent, top, top_artists, playlists, albums, artists, liked, releases) = futures::join!(
            self.spotify_api("/me/player/recently-played", &q_recent),
            self.spotify_api("/me/top/tracks", &q_top),
            self.spotify_api("/me/top/artists", &q_top_artists),
            self.spotify_collect("/me/playlists", &q_lists, 500),
            self.spotify_api("/me/albums", &q_albums),
            self.spotify_api("/me/following", &q_following),
            self.spotify_api("/me/tracks", &q_liked),
            self.spotify_api("/browse/new-releases", &q_releases),
        );
        let or_null = |r: Result<Value>| r.unwrap_or(Value::Null);
        let recent = or_null(recent);
        let mut seen = std::collections::HashSet::new();
        let recent = usable(items(&recent["items"]).map(|i| &i["track"]))
            .into_iter()
            .filter(|t| seen.insert(t.id.clone()))
            .collect();
        let top = or_null(top);
        let top_artists = or_null(top_artists);
        let albums = or_null(albums);
        let artists = or_null(artists);
        let liked = or_null(liked);
        let releases = or_null(releases);
        Home {
            recent,
            top_tracks: usable(items(&top["items"])),
            top_artists: items(&top_artists["items"]).map(artist_card).collect(),
            playlists: playlists
                .map(|(l, _)| {
                    l.iter()
                        .filter(|p| !p.is_null())
                        .map(playlist_card)
                        .collect()
                })
                .unwrap_or_default(),
            albums: items(&albums["items"])
                .map(|i| album_card(&i["album"]))
                .collect(),
            artists: items(&artists["artists"]["items"])
                .map(artist_card)
                .collect(),
            liked: usable(items(&liked["items"]).map(|i| &i["track"])),
            liked_total: liked["total"].as_u64().unwrap_or(0),
            new_releases: items(&releases["albums"]["items"])
                .filter(|a| !a.is_null())
                .map(album_card)
                .collect(),
        }
    }

    /// Every liked song, newest first.
    pub async fn spotify_liked(&self) -> Result<Vec<Track>> {
        let (rows, _) = self
            .spotify_collect(
                "/me/tracks",
                &[("limit", "50".into()), ("market", self.spotify_country())],
                10_000,
            )
            .await?;
        Ok(usable(rows.iter().map(|i| &i["track"])))
    }

    /// Playlists the user can add tracks to.
    pub async fn spotify_editable_playlists(&self) -> Result<Vec<PlaylistCard>> {
        let me = self.spotify_user_id();
        let (rows, _) = self
            .spotify_collect("/me/playlists", &[("limit", "50".into())], 500)
            .await?;
        Ok(rows
            .iter()
            .filter(|p| !p.is_null())
            .map(playlist_card)
            .filter(|p| p.owner_id == me || p.collaborative)
            .collect())
    }

    pub async fn spotify_playlist(&self, id: &str) -> Result<PlaylistPage> {
        let market = self.spotify_country();
        let head = self
            .spotify_api(
                &format!("/playlists/{id}"),
                &[
                    ("market", market.clone()),
                    (
                        "fields",
                        "id,name,description,collaborative,images,owner(id,display_name),tracks(total)"
                            .into(),
                    ),
                ],
            )
            .await?;
        let card = playlist_card(&head);
        let query = [("limit", "100".to_string()), ("market", market)];
        // Newer apps get `/items`; older ones only `/tracks`.
        let rows = match self
            .spotify_collect(&format!("/playlists/{id}/items"), &query, 10_000)
            .await
        {
            Ok((rows, _)) => rows,
            Err(_) => {
                self.spotify_collect(&format!("/playlists/{id}/tracks"), &query, 10_000)
                    .await?
                    .0
            }
        };
        let tracks = usable(rows.iter().map(|i| {
            if i["track"].is_object() {
                &i["track"]
            } else {
                &i["item"]
            }
        }));
        let saved = card.owner_id == self.spotify_user_id()
            || self
                .spotify_contains(&Saveable::Playlist(id.to_string()))
                .await
                .unwrap_or(false);
        Ok(PlaylistPage {
            playlist: card,
            tracks,
            saved,
        })
    }

    pub async fn spotify_search_playlists(&self, q: &str) -> Vec<PlaylistCard> {
        let json = self
            .spotify_api(
                "/search",
                &[
                    ("q", q.into()),
                    ("type", "playlist".into()),
                    ("limit", "10".into()),
                ],
            )
            .await
            .unwrap_or(Value::Null);
        items(&json["playlists"]["items"])
            .filter(|p| !p.is_null())
            .map(playlist_card)
            .collect()
    }

    /// Whether the user has saved / follows `what`.
    pub async fn spotify_contains(&self, what: &Saveable) -> Result<bool> {
        let (path, query): (String, Vec<(&str, String)>) = match what {
            Saveable::Track(id) => (
                "/me/tracks/contains".into(),
                vec![("ids", bare_id(id).into())],
            ),
            Saveable::Album(id) => ("/me/albums/contains".into(), vec![("ids", id.clone())]),
            Saveable::Artist(id) => (
                "/me/following/contains".into(),
                vec![("type", "artist".into()), ("ids", id.clone())],
            ),
            Saveable::Playlist(id) => (
                format!("/playlists/{id}/followers/contains"),
                vec![("ids", self.spotify_user_id())],
            ),
        };
        let json = self.spotify_api(&path, &query).await?;
        Ok(json[0].as_bool() == Some(true))
    }

    /// Saves (or removes) a track or album, follows (or unfollows) an artist or playlist.
    pub async fn spotify_set_saved(&self, what: &Saveable, on: bool) -> Result<()> {
        let method = if on { Method::PUT } else { Method::DELETE };
        match what {
            Saveable::Track(id) => {
                self.spotify_send(method, "/me/tracks", &[("ids", bare_id(id).into())], None)
                    .await?;
            }
            Saveable::Album(id) => {
                self.spotify_send(method, "/me/albums", &[("ids", id.clone())], None)
                    .await?;
            }
            Saveable::Artist(id) => {
                self.spotify_send(
                    method,
                    "/me/following",
                    &[("type", "artist".into()), ("ids", id.clone())],
                    None,
                )
                .await?;
            }
            Saveable::Playlist(id) => {
                self.spotify_send(method, &format!("/playlists/{id}/followers"), &[], None)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn spotify_add_to_playlist(&self, playlist: &str, uris: &[String]) -> Result<()> {
        for chunk in uris.chunks(100) {
            let body = Some(json!({ "uris": chunk }));
            let items = self
                .spotify_send(
                    Method::POST,
                    &format!("/playlists/{playlist}/items"),
                    &[],
                    body.clone(),
                )
                .await;
            if items.is_err() {
                self.spotify_send(
                    Method::POST,
                    &format!("/playlists/{playlist}/tracks"),
                    &[],
                    body,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn spotify_remove_from_playlist(&self, playlist: &str, uri: &str) -> Result<()> {
        let body = Some(json!({ "tracks": [{ "uri": uri }], "items": [{ "uri": uri }] }));
        let items = self
            .spotify_send(
                Method::DELETE,
                &format!("/playlists/{playlist}/items"),
                &[],
                body.clone(),
            )
            .await;
        if items.is_err() {
            self.spotify_send(
                Method::DELETE,
                &format!("/playlists/{playlist}/tracks"),
                &[],
                body,
            )
            .await?;
        }
        Ok(())
    }

    /// Creates a private playlist, optionally filled with `uris`; returns it.
    pub async fn spotify_create_playlist(
        &self,
        name: &str,
        uris: &[String],
    ) -> Result<PlaylistCard> {
        let body = Some(json!({ "name": name, "public": false }));
        let created = match self
            .spotify_send(Method::POST, "/me/playlists", &[], body.clone())
            .await
        {
            Ok(v) => v,
            Err(_) => {
                let me = self.spotify_user_id();
                self.spotify_send(Method::POST, &format!("/users/{me}/playlists"), &[], body)
                    .await?
            }
        };
        let card = playlist_card(&created);
        if !uris.is_empty() {
            self.spotify_add_to_playlist(&card.id, uris).await?;
        }
        Ok(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_cards_map_owner_and_strip_html() {
        let c = playlist_card(&json!({
            "id": "p", "name": "Mix", "collaborative": false,
            "owner": {"id": "u1", "display_name": "Me"},
            "images": [{"url": "big"}],
            "description": "Fresh <a href=\"x\">picks</a> &amp; more",
            "tracks": {"total": 42}
        }));
        assert_eq!((c.owner.as_str(), c.owner_id.as_str()), ("Me", "u1"));
        assert_eq!(c.description, "Fresh picks & more");
        assert_eq!((c.total, c.image.as_str()), (42, "big"));
        let newer = playlist_card(&json!({"id": "p", "owner": {"id": "u"}, "items": {"total": 3}}));
        assert_eq!((newer.total, newer.owner.as_str()), (3, "u"));
    }

    #[test]
    fn track_ids_lose_the_mosaic_prefix() {
        assert_eq!(bare_id("sp:abc"), "abc");
        assert_eq!(bare_id("abc"), "abc");
    }
}
