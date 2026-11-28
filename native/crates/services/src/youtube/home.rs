//! What the YouTube tab shows for a signed-in account, read straight from YouTube Music the
//! way its web player does it (cookies only, no developer client): recently played, the
//! home shelves, liked songs, playlists and followed artists. Each part is one request, all
//! sent at once, so the tab fills in right after sign-in.

use anyhow::{anyhow, Result};
use mosaic_core::model::Track;
use serde_json::{json, Value};

use super::api::youtube_track;
use super::catalog::{collect, find, is_count, row_card, song_row, tile_card};
use super::innertube::valid_video_id;
use super::music::{art_of, segments, text_of};
use crate::catalog::{Card, CardKind};
use crate::Services;

/// One home shelf ("Listen again", "Quick picks", "Mixed for you", …).
#[derive(Clone, Debug, Default)]
pub struct Shelf {
    pub title: String,
    pub tracks: Vec<Track>,
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug, Default)]
pub struct YtHome {
    /// Newest first, from YouTube Music's own history.
    pub recent: Vec<Track>,
    /// The first page of liked songs (the import reads all of them).
    pub liked: Vec<Track>,
    pub playlists: Vec<Card>,
    pub artists: Vec<Card>,
    pub shelves: Vec<Shelf>,
}

/// A two-row tile that plays a song rather than opening a page ("Listen again").
fn tile_song(item: &Value) -> Option<Track> {
    let id = item["navigationEndpoint"]["watchEndpoint"]["videoId"]
        .as_str()
        .filter(|id| valid_video_id(id))?;
    let title = text_of(&item["title"]);
    if title.is_empty() {
        return None;
    }
    let artist = segments(&item["subtitle"]["runs"])
        .into_iter()
        .find(|p| !matches!(p.as_str(), "Song" | "Video") && !is_count(p))
        .unwrap_or_default();
    let (art, thumb) =
        art_of(&item["thumbnailRenderer"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]);
    Some(youtube_track(id, title, &artist, 0, &art, &thumb))
}

/// Every song row of a browse response, in order, each once.
fn song_rows(response: &Value) -> Vec<Track> {
    let mut rows = Vec::new();
    collect(response, "musicResponsiveListItemRenderer", &mut rows);
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter_map(|r| song_row(r, None))
        .filter(|t| seen.insert(t.id.clone()))
        .collect()
}

/// The cards of a library grid or list, of `kind` only. The "New playlist" button, "Your
/// Likes" (`VLLM`) and "Episodes for later" (`VLSE`) aren't playlists to show.
fn library_cards(response: &Value, kind: CardKind) -> Vec<Card> {
    let mut tiles = Vec::new();
    collect(response, "musicTwoRowItemRenderer", &mut tiles);
    let mut rows = Vec::new();
    collect(response, "musicResponsiveListItemRenderer", &mut rows);
    let mut out: Vec<Card> = Vec::new();
    let cards = tiles
        .into_iter()
        .filter_map(|t| tile_card(t, ""))
        .chain(rows.into_iter().filter_map(row_card));
    for card in cards {
        if card.kind == kind
            && !matches!(card.id.as_str(), "VLLM" | "VLSE")
            && !out.iter().any(|c| c.id == card.id)
        {
            out.push(card);
        }
    }
    out
}

/// The carousels of `FEmusic_home`, each with its songs and its cards.
pub fn parse_home(response: &Value) -> Vec<Shelf> {
    let mut carousels = Vec::new();
    collect(response, "musicCarouselShelfRenderer", &mut carousels);
    carousels
        .into_iter()
        .filter_map(|c| {
            let title = text_of(&c["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]);
            let mut shelf = Shelf {
                title,
                ..Shelf::default()
            };
            for item in c["contents"].as_array().into_iter().flatten() {
                if let Some(row) = item.get("musicResponsiveListItemRenderer") {
                    shelf.tracks.extend(song_row(row, None));
                } else if let Some(tile) = item.get("musicTwoRowItemRenderer") {
                    match tile_song(tile) {
                        Some(t) => shelf.tracks.push(t),
                        None => shelf.cards.extend(tile_card(tile, "")),
                    }
                }
            }
            (!shelf.title.is_empty() && (!shelf.tracks.is_empty() || !shelf.cards.is_empty()))
                .then_some(shelf)
        })
        .collect()
}

pub fn parse_history(response: &Value) -> Vec<Track> {
    song_rows(find(response, "sectionListRenderer").unwrap_or(response))
}

impl Services {
    /// Everything the YouTube tab shows for the signed-in account. Parts that fail are left
    /// empty (and logged); only a sign-in YouTube no longer accepts fails the whole of it.
    pub async fn yt_home(&self) -> Result<YtHome> {
        let cookies = self
            .yt_cookies()
            .ok_or_else(|| anyhow!("Sign in to YouTube Music first"))?;
        let browse = |id: &str| {
            let cookies = cookies.clone();
            let body = json!({ "browseId": id });
            async move { self.yt_music_post("browse", body, &cookies).await }
        };
        let (history, home, liked, playlists, artists) = tokio::join!(
            browse("FEmusic_history"),
            browse("FEmusic_home"),
            browse("VLLM"),
            browse("FEmusic_liked_playlists"),
            browse("FEmusic_library_corpus_artists"),
        );
        let parts = [&history, &home, &liked, &playlists, &artists];
        if let Some(Err(e)) = parts
            .iter()
            .find(|r| r.as_ref().is_err_and(|e| e.to_string().contains("expired")))
        {
            return Err(anyhow!("{e}"));
        }
        if parts.iter().all(|r| r.is_err()) {
            return Err(history
                .err()
                .unwrap_or_else(|| anyhow!("YouTube Music didn't answer")));
        }
        fn ok<T: Default>(r: Result<Value>, what: &str, f: impl Fn(&Value) -> T) -> T {
            match r {
                Ok(v) => f(&v),
                Err(e) => {
                    tracing::warn!("YouTube Music {what}: {e}");
                    T::default()
                }
            }
        }
        Ok(YtHome {
            recent: ok(history, "history", parse_history),
            shelves: ok(home, "home", parse_home),
            liked: ok(liked, "liked songs", song_rows),
            playlists: ok(playlists, "playlists", |v| {
                library_cards(v, CardKind::Playlist)
            }),
            artists: ok(artists, "artists", |v| library_cards(v, CardKind::Artist)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn home_shelves_keep_their_songs_and_cards() {
        let shelves = parse_home(&fixture("ytm_home"));
        assert!(shelves.len() >= 2);
        assert!(shelves.iter().any(|s| !s.cards.is_empty()));
        assert!(shelves.iter().any(|s| !s.tracks.is_empty()));
        for s in &shelves {
            assert!(!s.title.is_empty());
            assert!(s
                .cards
                .iter()
                .all(|c| !c.id.is_empty() && !c.title.is_empty()));
        }
    }

    #[test]
    fn library_playlists_drop_likes_and_buttons() {
        let lists = library_cards(&fixture("yt_library_playlists"), CardKind::Playlist);
        let ids: Vec<_> = lists.iter().map(|c| c.id.as_str()).collect();
        assert!(!ids.contains(&"VLLM"));
        assert!(ids.iter().all(|id| id.starts_with("VL")), "{ids:?}");
        assert!(!lists.is_empty());
    }

    #[test]
    fn listen_again_tiles_play_songs() {
        let tile = json!({
            "title": {"runs": [{"text": "Xtal"}]},
            "subtitle": {"runs": [{"text": "Song"}, {"text": " • "}, {"text": "Aphex Twin"}]},
            "navigationEndpoint": {"watchEndpoint": {"videoId": "sWcLccMuCA8"}},
            "thumbnailRenderer": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "https://lh3.googleusercontent.com/x=w120-h120-l90-rj"}]}}}
        });
        let t = tile_song(&tile).unwrap();
        assert_eq!(
            (t.title.as_str(), t.artist.as_str()),
            ("Xtal", "Aphex Twin")
        );
        assert!(tile_song(&json!({"navigationEndpoint": {"browseEndpoint": {}}})).is_none());
    }

    #[test]
    fn history_rows_are_songs_once_each() {
        let history = json!({"contents": {"sectionListRenderer": {"contents": [
            {"musicShelfRenderer": {"title": {"runs": [{"text": "Today"}]}, "contents": [
                {"musicResponsiveListItemRenderer": {
                    "playlistItemData": {"videoId": "sWcLccMuCA8"},
                    "flexColumns": [
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Xtal"}]}}},
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Aphex Twin"}]}}},
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Selected Ambient Works 85-92"}]}}}
                    ],
                    "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": "4:54"}]}}}]
                }}]}},
            {"musicShelfRenderer": {"title": {"runs": [{"text": "Yesterday"}]}, "contents": [
                {"musicResponsiveListItemRenderer": {
                    "playlistItemData": {"videoId": "sWcLccMuCA8"},
                    "flexColumns": [
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Xtal"}]}}}
                    ]}}]}}
        ]}}});
        let recent = parse_history(&history);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].album, "Selected Ambient Works 85-92");
        assert_eq!(recent[0].duration, 294_000);
    }
}
