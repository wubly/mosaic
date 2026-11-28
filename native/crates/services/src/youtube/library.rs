//! The signed-in user's YouTube Music library: liked songs, playlists and followed artists.
//!
//! Requests are WEB_REMIX InnerTube calls authenticated like the web player does it (cookies plus
//! a `SAPISIDHASH` header, see `auth`). Every list is read to its end through continuations.

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use futures::{stream, StreamExt};
use mosaic_core::model::Track;
use serde_json::{json, Value};

use super::api::youtube_track;
use super::innertube::valid_video_id;
use crate::Services;

const HOST: &str = "https://music.youtube.com/youtubei/v1";
const REMIX_VERSION: &str = "1.20260707.12.00";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
/// A library is read page by page; this only stops a response that never ends.
const MAX_PAGES: usize = 500;
/// Playlists fetched at the same time by [`Services::yt_import_everything`].
const PLAYLIST_PARALLEL: usize = 3;

pub(super) const TRACK_SHELVES: &[&str] = &[
    "musicPlaylistShelfRenderer",
    "musicPlaylistShelfContinuation",
    "musicShelfRenderer",
    "musicShelfContinuation",
    "appendContinuationItemsAction",
];
const GRID: &[&str] = &[
    "gridRenderer",
    "gridContinuation",
    "appendContinuationItemsAction",
];

/// Everything in the account's library.
#[derive(Clone, Debug, Default)]
pub struct YtLibrary {
    pub liked: Vec<Track>,
    /// (name, tracks) for each playlist in the library.
    pub playlists: Vec<(String, Vec<Track>)>,
    /// (name, browse id) of each followed artist.
    pub artists: Vec<(String, String)>,
    /// Names of playlists that could not be read (the rest of the import still stands).
    pub failed: Vec<String>,
}

fn find<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    match node {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find(v, key))),
        Value::Array(list) => list.iter().find_map(|v| find(v, key)),
        _ => None,
    }
}

fn text(node: &Value) -> String {
    node["runs"]
        .as_array()
        .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
        .or_else(|| node["simpleText"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn flex(row: &Value, i: usize) -> &Value {
    &row["flexColumns"][i]["musicResponsiveListItemFlexColumnRenderer"]["text"]
}

fn clock_ms(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut parts = 0;
    for part in text.split(':') {
        total = total * 60 + part.trim().parse::<u64>().ok()?;
        parts += 1;
    }
    (parts > 0 && parts <= 3).then_some(total * 1000)
}

/// Google-hosted album art comes sized in the URL (`=w120-h120-…`); ask for the size wanted.
fn sized(url: &str, px: u32) -> String {
    match url.rsplit_once("=w") {
        Some((base, size)) if size.starts_with(|c: char| c.is_ascii_digit()) => {
            format!("{base}=w{px}-h{px}-l90-rj")
        }
        _ => url.to_string(),
    }
}

/// One page of a list: its rows, and the token of the next page if there is one.
struct Page<'a> {
    rows: Vec<&'a Value>,
    next: Option<String>,
}

/// The rows (`row_key` items) of the first of `containers` found in `response`, plus the
/// continuation token. Anything outside that container (suggestions, headers) is ignored.
fn page<'a>(response: &'a Value, containers: &[&str], row_key: &str) -> Page<'a> {
    let Some(container) = containers.iter().find_map(|k| find(response, k)) else {
        return Page {
            rows: Vec::new(),
            next: None,
        };
    };
    let items = ["contents", "items", "continuationItems"]
        .iter()
        .find_map(|k| container[k].as_array())
        .map_or(&[][..], Vec::as_slice);
    let rows = items.iter().filter_map(|i| i.get(row_key)).collect();
    let next = items
        .iter()
        .find_map(|i| {
            i["continuationItemRenderer"]["continuationEndpoint"]["continuationCommand"]["token"]
                .as_str()
        })
        .or_else(|| container["continuations"][0]["nextContinuationData"]["continuation"].as_str())
        .map(str::to_string);
    Page { rows, next }
}

/// A playlist/library row as a track; `None` for rows that can't be played (deleted or private
/// videos are greyed out and have no usable id).
fn parse_track(row: &Value) -> Option<Track> {
    if row["musicItemRendererDisplayPolicy"]
        .as_str()
        .is_some_and(|p| p.ends_with("GREY_OUT"))
    {
        return None;
    }
    let id = row["playlistItemData"]["videoId"]
        .as_str()
        .or_else(|| {
            flex(row, 0)["runs"][0]["navigationEndpoint"]["watchEndpoint"]["videoId"].as_str()
        })
        .filter(|id| valid_video_id(id))?;
    let title = text(flex(row, 0));
    if title.is_empty() {
        return None;
    }
    let artist_text = text(flex(row, 1));
    let artist = artist_text.split(" • ").next().unwrap_or_default();
    let ms = row["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"]
        [0]["text"]
        .as_str()
        .and_then(clock_ms)
        .unwrap_or(0);
    let url = row["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|t| t.last())
        .and_then(|t| t["url"].as_str())
        .unwrap_or_default();
    let mut track = youtube_track(id, title, artist, ms, &sized(url, 544), &sized(url, 226));
    track.album = text(flex(row, 2));
    Some(track)
}

fn parse_artist(row: &Value) -> Option<(String, String)> {
    let id = row["navigationEndpoint"]["browseEndpoint"]["browseId"]
        .as_str()
        .or_else(|| {
            flex(row, 0)["runs"][0]["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()
        })?;
    let name = text(flex(row, 0));
    (!name.is_empty()).then(|| (name, id.to_string()))
}

/// A playlist tile as (name, playlist id). Library tiles that aren't playlists (the "New
/// playlist" button) have no browse id; "Your Likes" (`LM`) and "Episodes for later" (`SE`) are
/// not playlists to import.
fn parse_playlist(item: &Value) -> Option<(String, String)> {
    let browse = item["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()?;
    let id = browse.strip_prefix("VL").unwrap_or(browse);
    if id.is_empty() || id == "LM" || id == "SE" {
        return None;
    }
    let name = text(&item["title"]);
    (!name.is_empty()).then(|| (name, id.to_string()))
}

/// Reads a list to its end. `fetch` gets each request body (the first names `browse_id`, the
/// rest carry a continuation token) and returns the response.
pub(super) async fn crawl<T, F, Fut>(
    mut fetch: F,
    browse_id: &str,
    containers: &[&str],
    row_key: &str,
    parse: impl Fn(&Value) -> Option<T>,
) -> Result<Vec<T>>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let mut out = Vec::new();
    let mut body = json!({"browseId": browse_id});
    let mut seen = HashSet::new();
    for _ in 0..MAX_PAGES {
        let response = fetch(body).await?;
        let page = page(&response, containers, row_key);
        out.extend(page.rows.into_iter().filter_map(&parse));
        match page.next {
            Some(token) if seen.insert(token.clone()) => body = json!({"continuation": token}),
            _ => break,
        }
    }
    Ok(out)
}

impl Services {
    fn yt_session_cookies(&self) -> Result<String> {
        self.yt_cookies()
            .ok_or_else(|| anyhow!("Sign in to YouTube Music first"))
    }

    /// A signed-in WEB_REMIX InnerTube POST using `cookies`.
    pub(crate) async fn yt_music_post(
        &self,
        endpoint: &str,
        mut body: Value,
        cookies: &str,
    ) -> Result<Value> {
        let (visitor, account) = self.yt_session_ids();
        body["context"] = json!({
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": REMIX_VERSION,
                "hl": "en",
                "gl": "US",
            },
            "user": {"lockedSafetyMode": false},
        });
        if let Some(v) = &visitor {
            body["context"]["client"]["visitorData"] = json!(v);
        }
        if let Some(a) = account {
            body["context"]["user"]["onBehalfOfUser"] = json!(a);
        }
        let mut req = self
            .http()
            .post(format!("{HOST}/{endpoint}?prettyPrint=false"))
            .header("Origin", "https://music.youtube.com")
            .header("Referer", "https://music.youtube.com/")
            .header("User-Agent", USER_AGENT)
            .header("X-Youtube-Client-Name", "67")
            .header("X-Youtube-Client-Version", REMIX_VERSION)
            .json(&body)
            .timeout(Duration::from_secs(20));
        for (name, value) in Self::yt_auth_headers(cookies)? {
            req = req.header(name, value);
        }
        if let Some(v) = visitor {
            req = req.header("X-Goog-Visitor-Id", v);
        }
        let res = req
            .send()
            .await
            .map_err(|e| anyhow!("Could not reach YouTube Music: {e}"))?;
        let status = res.status();
        if matches!(status.as_u16(), 401 | 403) {
            bail!("YouTube sign-in expired. Sign in again.");
        }
        if !status.is_success() {
            bail!("YouTube Music said {}", status.as_u16());
        }
        Ok(res.json().await?)
    }

    async fn yt_crawl<T>(
        &self,
        browse_id: &str,
        containers: &[&str],
        row_key: &str,
        parse: impl Fn(&Value) -> Option<T>,
    ) -> Result<Vec<T>> {
        let cookies = self.yt_session_cookies()?;
        crawl(
            |body| {
                let cookies = cookies.clone();
                async move { self.yt_music_post("browse", body, &cookies).await }
            },
            browse_id,
            containers,
            row_key,
            parse,
        )
        .await
    }

    /// Every liked song, newest first.
    pub async fn yt_liked_songs(&self) -> Result<Vec<Track>> {
        match self
            .yt_crawl(
                "VLLM",
                TRACK_SHELVES,
                "musicResponsiveListItemRenderer",
                parse_track,
            )
            .await
        {
            Ok(tracks) => Ok(tracks),
            Err(_) => {
                self.yt_crawl(
                    "FEmusic_liked_videos",
                    TRACK_SHELVES,
                    "musicResponsiveListItemRenderer",
                    parse_track,
                )
                .await
            }
        }
    }

    /// The playlists in the library as (name, playlist id), without "Your Likes".
    pub async fn yt_library_playlists(&self) -> Result<Vec<(String, String)>> {
        self.yt_crawl(
            "FEmusic_liked_playlists",
            GRID,
            "musicTwoRowItemRenderer",
            parse_playlist,
        )
        .await
    }

    /// Every playable track of a playlist.
    pub async fn yt_playlist_tracks(&self, id: &str) -> Result<Vec<Track>> {
        let browse = if id.starts_with("VL") {
            id.to_string()
        } else {
            format!("VL{id}")
        };
        self.yt_crawl(
            &browse,
            TRACK_SHELVES,
            "musicResponsiveListItemRenderer",
            parse_track,
        )
        .await
    }

    /// The artists the account follows, as (name, browse id).
    pub async fn yt_library_artists(&self) -> Result<Vec<(String, String)>> {
        self.yt_crawl(
            "FEmusic_library_corpus_artists",
            TRACK_SHELVES,
            "musicResponsiveListItemRenderer",
            parse_artist,
        )
        .await
    }

    /// Liked songs, playlists with their tracks, and followed artists. A playlist that can't be
    /// read is listed in `failed` instead of failing the import.
    pub async fn yt_import_everything(&self) -> Result<YtLibrary> {
        let liked = self.yt_liked_songs().await?;
        let lists = self.yt_library_playlists().await?;
        let artists = self.yt_library_artists().await.unwrap_or_default();
        let results: Vec<_> = stream::iter(lists)
            .map(|(name, id)| async move {
                let tracks = self.yt_playlist_tracks(&id).await;
                (name, tracks)
            })
            .buffered(PLAYLIST_PARALLEL)
            .collect()
            .await;
        let mut library = YtLibrary {
            liked,
            artists,
            ..Default::default()
        };
        for (name, tracks) in results {
            match tracks {
                Ok(tracks) => library.playlists.push((name, tracks)),
                Err(e) => {
                    tracing::warn!("YouTube playlist {name}: {e}");
                    library.failed.push(name);
                }
            }
        }
        Ok(library)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Serves fixtures by request: the first request returns `first`, a continuation returns the
    /// fixture its token names.
    fn server(
        first: &'static str,
        next: &'static [(&'static str, &'static str)],
    ) -> impl FnMut(Value) -> std::future::Ready<Result<Value>> {
        move |body: Value| {
            let name = match body["continuation"].as_str() {
                None => first,
                Some(t) => next.iter().find(|(k, _)| *k == t).expect("known token").1,
            };
            std::future::ready(Ok(fixture(name)))
        }
    }

    #[tokio::test]
    async fn liked_songs_follow_continuations_and_skip_unplayable_rows() {
        let fetch = server(
            "yt_library_liked_page1.json",
            &[("TOKEN_PAGE_2", "yt_library_liked_page2.json")],
        );
        let tracks = crawl(
            fetch,
            "VLLM",
            TRACK_SHELVES,
            "musicResponsiveListItemRenderer",
            parse_track,
        )
        .await
        .unwrap();
        let ids: Vec<_> = tracks.iter().map(|t| t.id.key()).collect();
        assert_eq!(ids, ["yt:sWcLccMuCA8", "yt:q6sQeUkyWPc", "yt:-yNHlKAzyVA"]);
        let xtal = &tracks[0];
        assert_eq!(xtal.source, mosaic_core::model::Source::YouTube);
        assert_eq!(xtal.video_id.as_deref(), Some("sWcLccMuCA8"));
        assert_eq!(xtal.title, "Xtal");
        assert_eq!(xtal.artist, "Aphex Twin");
        assert_eq!(xtal.album, "Selected Ambient Works 85-92");
        assert_eq!(xtal.duration, 294_000);
        assert_eq!(
            xtal.artwork,
            "https://lh3.googleusercontent.com/a=w544-h544-l90-rj"
        );
        assert_eq!(tracks[1].artist, "Steven Jeffries & Mary Carewe");
        assert_eq!(tracks[1].duration, 4_145_000);
    }

    #[tokio::test]
    async fn playlist_reads_only_its_own_shelf_and_the_append_continuation() {
        let fetch = server(
            "yt_library_playlist_page1.json",
            &[("PL_TOKEN_2", "yt_library_playlist_page2.json")],
        );
        let tracks = crawl(
            fetch,
            "VLPLchill",
            TRACK_SHELVES,
            "musicResponsiveListItemRenderer",
            parse_track,
        )
        .await
        .unwrap();
        let ids: Vec<_> = tracks.iter().map(|t| t.id.key()).collect();
        assert_eq!(ids, ["yt:dQw4w9WgXcQ", "yt:sWcLccMuCA8"], "no suggestions");
        assert_eq!(
            tracks[0].artist, "Rick Astley",
            "the Topic suffix is dropped"
        );
        assert_eq!(tracks[1].album, "SAW 85-92");
    }

    #[tokio::test]
    async fn library_playlists_skip_the_new_button_and_likes() {
        let fetch = server(
            "yt_library_playlists.json",
            &[("GRID_TOKEN_2", "yt_library_playlists_page2.json")],
        );
        let lists = crawl(
            fetch,
            "FEmusic_liked_playlists",
            GRID,
            "musicTwoRowItemRenderer",
            parse_playlist,
        )
        .await
        .unwrap();
        assert_eq!(
            lists,
            [
                ("Chill".to_string(), "PLchill".to_string()),
                ("Gym".to_string(), "PLgym".to_string()),
                ("Focus".to_string(), "PLfocus".to_string()),
            ]
        );
    }

    #[test]
    fn artists_come_with_their_browse_ids() {
        let response = fixture("yt_library_artists.json");
        let p = page(&response, TRACK_SHELVES, "musicResponsiveListItemRenderer");
        assert!(p.next.is_none());
        let artists: Vec<_> = p.rows.into_iter().filter_map(parse_artist).collect();
        assert_eq!(
            artists,
            [
                (
                    "Aphex Twin".to_string(),
                    "UCqzXGgtNXPUG0nR3tjfQmTg".to_string()
                ),
                ("Boards of Canada".to_string(), "UCkey".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn a_repeated_token_ends_the_crawl() {
        let looping = |_body: Value| {
            std::future::ready(Ok(json!({"musicShelfContinuation": {
                "contents": [],
                "continuations": [{"nextContinuationData": {"continuation": "SAME"}}],
            }})))
        };
        let out = crawl(
            looping,
            "x",
            TRACK_SHELVES,
            "musicResponsiveListItemRenderer",
            parse_track,
        )
        .await
        .unwrap();
        assert!(out.is_empty());
    }
}
