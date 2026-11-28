//! YouTube Music's side of `crate::catalog`: artists, albums and playlists from the filtered
//! WEB_REMIX searches, and their pages from `browse`. None of it needs an account; a public
//! playlist is read through the signed-in session when there is one so private ones open too.

use anyhow::{bail, Result};
use mosaic_core::model::{Source, Track};
use serde_json::{json, Value};

use super::api::youtube_track;
use super::innertube::valid_video_id;
use super::library::{crawl, TRACK_SHELVES};
use super::music::{art_of, clock_ms, segments, text_of};
use crate::catalog::{Card, CardKind, Collection, Found};
use crate::Services;

const ARTISTS_ONLY: &str = "EgWKAQIgAWoKEAkQBRAKEAMQBA==";
const ALBUMS_ONLY: &str = "EgWKAQIYAWoKEAkQBRAKEAMQBA==";
/// "Featured playlists" (YouTube Music's own); community playlists are mostly noise.
const PLAYLISTS_ONLY: &str = "EgeKAQQoADgBagwQDhAKEAMQBRAJEAQ=";
const CARDS_PER_KIND: usize = 12;

pub(super) fn find<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    match node {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find(v, key))),
        Value::Array(list) => list.iter().find_map(|v| find(v, key)),
        _ => None,
    }
}

pub(super) fn collect<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                out.push(found);
            }
            map.values().for_each(|v| collect(v, key, out));
        }
        Value::Array(list) => list.iter().for_each(|v| collect(v, key, out)),
        _ => {}
    }
}

fn page_type(endpoint: &Value) -> &str {
    endpoint["browseEndpoint"]["browseEndpointContextSupportedConfigs"]
        ["browseEndpointContextMusicConfig"]["pageType"]
        .as_str()
        .unwrap_or_default()
}

/// What a browse endpoint opens: its page type, or for an endpoint without one (some library
/// tiles), what its id looks like.
fn endpoint_kind(endpoint: &Value) -> Option<CardKind> {
    kind_of(page_type(endpoint)).or_else(|| {
        let id = endpoint["browseEndpoint"]["browseId"].as_str()?;
        if id.starts_with("VL") {
            Some(CardKind::Playlist)
        } else if id.starts_with("MPRE") {
            Some(CardKind::Album)
        } else if id.starts_with("UC") {
            Some(CardKind::Artist)
        } else {
            None
        }
    })
}

fn kind_of(page_type: &str) -> Option<CardKind> {
    match page_type {
        "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => Some(CardKind::Artist),
        "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => Some(CardKind::Album),
        "MUSIC_PAGE_TYPE_PLAYLIST" => Some(CardKind::Playlist),
        _ => None,
    }
}

pub(super) fn is_count(part: &str) -> bool {
    ["views", "plays", "audience", "subscribers", "songs", "song"]
        .iter()
        .any(|w| part.ends_with(w))
}

fn is_year(part: &str) -> bool {
    part.len() == 4 && part.chars().all(|c| c.is_ascii_digit())
}

/// `Album • Daft Punk • 2013` → `2013 · Daft Punk`; `Single • 2023` → `2023 · Single`;
/// `Artist • 65.8M monthly audience` → `Artist`; `YouTube Music • 35 songs` → `YouTube Music`.
fn card_subtitle(kind: CardKind, parts: &[String]) -> String {
    let label = parts.first().filter(|p| {
        matches!(
            p.as_str(),
            "Album" | "Single" | "EP" | "Playlist" | "Artist"
        )
    });
    let rest: Vec<&String> = parts
        .iter()
        .skip(usize::from(label.is_some()))
        .filter(|p| !is_count(p))
        .collect();
    match kind {
        CardKind::Artist => "Artist".into(),
        CardKind::Album => {
            let year = rest.iter().find(|p| is_year(p)).map(|p| p.as_str());
            let who = match label.map(String::as_str) {
                Some("Single") => Some("Single"),
                Some("EP") => Some("EP"),
                _ => rest.iter().find(|p| !is_year(p)).map(|p| p.as_str()),
            };
            [year, who]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ")
        }
        CardKind::Playlist => rest
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

/// A row of a filtered search (or of a library list) that leads to a page.
pub(super) fn row_card(row: &Value) -> Option<Card> {
    let endpoint = &row["navigationEndpoint"];
    let kind = endpoint_kind(endpoint)?;
    let id = endpoint["browseEndpoint"]["browseId"].as_str()?;
    let columns = row["flexColumns"].as_array()?;
    let column = |i: usize| {
        columns.get(i).map_or(&Value::Null, |c| {
            &c["musicResponsiveListItemFlexColumnRenderer"]["text"]
        })
    };
    let title = text_of(column(0));
    if title.is_empty() {
        return None;
    }
    let parts = segments(&column(1)["runs"]);
    Some(Card {
        source: Source::YouTube,
        kind,
        id: id.to_string(),
        title,
        subtitle: card_subtitle(kind, &parts),
        image: art_of(&row["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]).1,
    })
}

/// A tile of an artist page's carousels.
pub(super) fn tile_card(item: &Value, fallback_artist: &str) -> Option<Card> {
    let endpoint = &item["navigationEndpoint"];
    let kind = endpoint_kind(endpoint)?;
    let id = endpoint["browseEndpoint"]["browseId"].as_str()?;
    let title = text_of(&item["title"]);
    if title.is_empty() {
        return None;
    }
    let mut parts = segments(&item["subtitle"]["runs"]);
    // An artist's own albums only show the year; put the artist in like search does.
    if kind == CardKind::Album && parts.iter().all(|p| is_year(p)) && !fallback_artist.is_empty() {
        parts.insert(0, fallback_artist.to_string());
    }
    Some(Card {
        source: Source::YouTube,
        kind,
        id: id.to_string(),
        title,
        subtitle: card_subtitle(kind, &parts),
        image: art_of(
            &item["thumbnailRenderer"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"],
        )
        .1,
    })
}

/// The cards of a filtered search response, of `kind` only.
pub fn parse_cards(response: &Value, kind: CardKind) -> Vec<Card> {
    let mut rows = Vec::new();
    collect(response, "musicResponsiveListItemRenderer", &mut rows);
    let mut out: Vec<Card> = Vec::new();
    for card in rows.into_iter().filter_map(row_card) {
        if card.kind == kind && !out.iter().any(|c| c.id == card.id) {
            out.push(card);
        }
    }
    out.truncate(CARDS_PER_KIND);
    out
}

/// A song row of an album, playlist or an artist's "Top songs". Columns differ: albums have
/// no artist or art per row (`album` fills them in), top songs carry plays and the album.
pub(super) fn song_row(row: &Value, album: Option<(&str, &str, &str)>) -> Option<Track> {
    if row["musicItemRendererDisplayPolicy"]
        .as_str()
        .is_some_and(|p| p.ends_with("GREY_OUT"))
    {
        return None;
    }
    let columns = row["flexColumns"].as_array()?;
    let column = |i: usize| {
        columns.get(i).map_or(&Value::Null, |c| {
            &c["musicResponsiveListItemFlexColumnRenderer"]["text"]
        })
    };
    let id = row["playlistItemData"]["videoId"]
        .as_str()
        .or_else(|| column(0)["runs"][0]["navigationEndpoint"]["watchEndpoint"]["videoId"].as_str())
        .filter(|id| valid_video_id(id))?;
    let title = text_of(column(0));
    if title.is_empty() {
        return None;
    }
    let byline = segments(&column(1)["runs"]);
    let artist = byline
        .iter()
        .find(|p| !matches!(p.as_str(), "Song" | "Video") && !is_count(p))
        .cloned()
        .unwrap_or_default();
    let ms = clock_ms(&text_of(
        &row["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"],
    ))
    .unwrap_or(0);
    let (mut art, mut thumb) =
        art_of(&row["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]);
    let album_column = (2..columns.len())
        .map(|i| text_of(column(i)))
        .find(|t| !t.is_empty() && !is_count(t))
        .unwrap_or_default();
    let (album_name, album_artist, album_art) = album.unwrap_or_default();
    if art.is_empty() {
        art = album_art.to_string();
        thumb = album_art.to_string();
    }
    let artist = if artist.is_empty() {
        album_artist
    } else {
        &artist
    };
    let mut track = super::music::linked(
        youtube_track(id, title, artist, ms, &art, &thumb),
        &row["flexColumns"],
    );
    track.album = if album_column.is_empty() {
        album_name.to_string()
    } else {
        album_column
    };
    Some(track)
}

/// The cover in a page header (not the small artist picture beside the strapline).
fn header_art(header: &Value) -> String {
    let mut thumbs = Vec::new();
    collect(&header["thumbnail"], "thumbnails", &mut thumbs);
    thumbs.first().map_or_else(String::new, |t| art_of(t).0)
}

/// An album page.
pub fn parse_album(response: &Value, card: &Card) -> Collection {
    let header = find(response, "musicResponsiveHeaderRenderer").unwrap_or(&Value::Null);
    let title = Some(text_of(&header["title"]))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| card.title.clone());
    let artist = text_of(&header["straplineTextOne"]);
    let image = Some(header_art(header))
        .filter(|i| !i.is_empty())
        .unwrap_or_else(|| card.image.clone());
    let sub = segments(&header["subtitle"]["runs"]);
    let label = sub.first().cloned().unwrap_or_default();
    let year = sub.iter().find(|p| is_year(p)).cloned().unwrap_or_default();
    let mut rows = Vec::new();
    if let Some(shelf) = find(response, "musicShelfRenderer") {
        collect(
            &shelf["contents"],
            "musicResponsiveListItemRenderer",
            &mut rows,
        );
    }
    let tracks = rows
        .into_iter()
        .filter_map(|r| song_row(r, Some((&title, &artist, &image))))
        .collect();
    let subtitle = [
        year,
        if label == "Album" {
            artist.clone()
        } else {
            label
        },
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    Collection {
        card: Card {
            title,
            subtitle: if subtitle.is_empty() {
                card.subtitle.clone()
            } else {
                subtitle
            },
            image,
            ..card.clone()
        },
        tracks,
        albums: Vec::new(),
        artist: artist_link(header),
    }
}

/// The artist an album header names, as a card to open.
fn artist_link(header: &Value) -> Option<Card> {
    let run = &header["straplineTextOne"]["runs"][0];
    let id = run["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()?;
    Some(Card {
        source: Source::YouTube,
        kind: CardKind::Artist,
        id: id.to_string(),
        title: run["text"].as_str().unwrap_or_default().to_string(),
        subtitle: "Artist".into(),
        image: art_of(
            &header["straplineThumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"],
        )
        .1,
    })
}

/// An artist page: top songs, then albums and singles.
pub fn parse_artist(response: &Value, card: &Card) -> Collection {
    let header = find(response, "musicImmersiveHeaderRenderer")
        .or_else(|| find(response, "musicVisualHeaderRenderer"))
        .unwrap_or(&Value::Null);
    let name = Some(text_of(&header["title"]))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| card.title.clone());
    let sections = find(response, "sectionListRenderer")
        .and_then(|s| s["contents"].as_array())
        .map_or(&[][..], Vec::as_slice);
    let mut tracks = Vec::new();
    let mut albums: Vec<Card> = Vec::new();
    for section in sections {
        if let Some(shelf) = section.get("musicShelfRenderer") {
            let mut rows = Vec::new();
            collect(
                &shelf["contents"],
                "musicResponsiveListItemRenderer",
                &mut rows,
            );
            tracks.extend(rows.into_iter().filter_map(|r| song_row(r, None)));
        } else if let Some(carousel) = section.get("musicCarouselShelfRenderer") {
            let title =
                text_of(&carousel["header"]["musicCarouselShelfBasicHeaderRenderer"]["title"]);
            if !matches!(title.as_str(), "Albums" | "Singles & EPs" | "Singles") {
                continue;
            }
            let mut items = Vec::new();
            collect(&carousel["contents"], "musicTwoRowItemRenderer", &mut items);
            for album in items.into_iter().filter_map(|i| tile_card(i, &name)) {
                if album.kind == CardKind::Album && !albums.iter().any(|a| a.id == album.id) {
                    albums.push(album);
                }
            }
        }
    }
    Collection {
        card: Card {
            title: name,
            subtitle: "Artist".into(),
            ..card.clone()
        },
        tracks,
        albums,
        artist: None,
    }
}

/// A playlist page's header (title, owner, art); its tracks are crawled separately.
fn playlist_header(response: &Value, card: &Card) -> Card {
    let header = find(response, "musicResponsiveHeaderRenderer")
        .or_else(|| find(response, "musicDetailHeaderRenderer"))
        .unwrap_or(&Value::Null);
    let title = Some(text_of(&header["title"]))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| card.title.clone());
    let owner = text_of(&header["straplineTextOne"]);
    let image = Some(header_art(header))
        .filter(|i| !i.is_empty())
        .unwrap_or_else(|| card.image.clone());
    Card {
        title,
        subtitle: [owner, card.subtitle.clone(), "Playlist".into()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_default(),
        image,
        ..card.clone()
    }
}

impl Services {
    /// A WEB_REMIX call as the signed-in user when there is one, else anonymous.
    async fn yt_browse(&self, body: Value) -> Result<Value> {
        match self.yt_cookies() {
            Some(cookies) => self.yt_music_post("browse", body, &cookies).await,
            None => self.music_post("browse", body).await,
        }
    }

    pub async fn yt_search_all(&self, query: &str) -> Result<Found> {
        let filtered = |params: &'static str| {
            self.music_post("search", json!({"query": query, "params": params}))
        };
        let (tracks, artists, albums, playlists) = tokio::join!(
            self.yt_search(query),
            filtered(ARTISTS_ONLY),
            filtered(ALBUMS_ONLY),
            filtered(PLAYLISTS_ONLY),
        );
        let cards = |r: Result<Value>, kind| {
            r.map(|v| parse_cards(&v, kind))
                .inspect_err(|e| tracing::warn!("YouTube Music {kind:?} search: {e}"))
                .unwrap_or_default()
        };
        Ok(Found {
            tracks: tracks?,
            artists: cards(artists, CardKind::Artist),
            albums: cards(albums, CardKind::Album),
            playlists: cards(playlists, CardKind::Playlist),
        })
    }

    pub async fn yt_open(&self, card: &Card) -> Result<Collection> {
        match card.kind {
            CardKind::Artist => {
                let json = self.yt_browse(json!({"browseId": card.id})).await?;
                Ok(parse_artist(&json, card))
            }
            CardKind::Album => {
                let json = self.yt_browse(json!({"browseId": card.id})).await?;
                Ok(parse_album(&json, card))
            }
            CardKind::Playlist => {
                let browse = if card.id.starts_with("VL") {
                    card.id.clone()
                } else {
                    format!("VL{}", card.id)
                };
                let first = self.yt_browse(json!({"browseId": browse})).await?;
                let header = playlist_header(&first, card);
                let mut served = Some(first);
                let tracks = crawl(
                    |body: Value| {
                        let ready = served.take().filter(|_| body["continuation"].is_null());
                        async move {
                            match ready {
                                Some(json) => Ok(json),
                                None => self.yt_browse(body).await,
                            }
                        }
                    },
                    &browse,
                    TRACK_SHELVES,
                    "musicResponsiveListItemRenderer",
                    |row| song_row(row, None),
                )
                .await?;
                if tracks.is_empty() {
                    bail!("This playlist is empty or private");
                }
                Ok(Collection {
                    card: header,
                    tracks,
                    albums: Vec::new(),
                    artist: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn card(kind: CardKind, id: &str) -> Card {
        Card {
            source: Source::YouTube,
            kind,
            id: id.into(),
            title: "clicked".into(),
            subtitle: String::new(),
            image: "https://clicked".into(),
        }
    }

    #[test]
    fn filtered_searches_give_artist_album_and_playlist_cards() {
        let artists = parse_cards(&fixture("ytm_catalog_search_artists"), CardKind::Artist);
        assert_eq!(artists[0].title, "Daft Punk");
        assert_eq!(artists[0].id, "UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(artists[0].subtitle, "Artist");
        assert!(artists[0].image.starts_with("https://"));

        let albums = parse_cards(&fixture("ytm_catalog_search_albums"), CardKind::Album);
        let discovery = albums.iter().find(|a| a.title == "Discovery").unwrap();
        assert_eq!(discovery.id, "MPREb_7ltM34kr0mH");
        assert_eq!(discovery.subtitle, "2001 · Daft Punk");

        let lists = parse_cards(&fixture("ytm_catalog_search_playlists"), CardKind::Playlist);
        assert_eq!(lists[0].title, "Presenting Daft Punk");
        assert!(lists[0].id.starts_with("VL"));
        assert_eq!(lists[0].subtitle, "YouTube Music");
        assert!(parse_cards(&fixture("ytm_catalog_search_playlists"), CardKind::Artist).is_empty());
    }

    #[test]
    fn album_rows_take_the_albums_artist_and_art() {
        let page = parse_album(
            &fixture("ytm_catalog_album"),
            &card(CardKind::Album, "MPREb_7ltM34kr0mH"),
        );
        assert_eq!(page.card.title, "Discovery");
        assert_eq!(page.card.subtitle, "2001 · Daft Punk");
        assert_eq!(page.tracks.len(), 14);
        let first = &page.tracks[0];
        assert_eq!(first.title, "One More Time");
        assert_eq!(first.artist, "Daft Punk");
        assert_eq!(first.album, "Discovery");
        assert_eq!(first.duration, 321_000);
        assert!(first.artwork.starts_with("https://"));
        let artist = page.artist.unwrap();
        assert_eq!(artist.id, "UCRr1xG_2WIDs18a6cIiCxeA");
        // Rows of an artist's top songs link both pages.
        let top = parse_artist(
            &fixture("ytm_catalog_artist"),
            &card(CardKind::Artist, "UCRr1xG_2WIDs18a6cIiCxeA"),
        );
        assert_eq!(
            top.tracks[0].artist_id.as_deref(),
            Some("UCRr1xG_2WIDs18a6cIiCxeA")
        );
        assert!(top.tracks[0]
            .album_id
            .as_deref()
            .is_some_and(|a| a.starts_with("MPREb_")));
        assert_eq!(artist.kind, CardKind::Artist);
    }

    #[test]
    fn artist_pages_have_top_songs_and_releases_only() {
        let page = parse_artist(
            &fixture("ytm_catalog_artist"),
            &card(CardKind::Artist, "UCRr1xG_2WIDs18a6cIiCxeA"),
        );
        assert_eq!(page.card.title, "Daft Punk");
        assert_eq!(
            page.card.image, "https://clicked",
            "the round card picture stays"
        );
        assert!(page.tracks.len() >= 5);
        let top = &page.tracks[0];
        assert_eq!(
            top.album, "Random Access Memories",
            "plays are not an album"
        );
        assert!(top.artist.starts_with("Daft Punk"));
        assert!(page.albums.iter().any(|a| a.title == "Discovery"));
        assert!(page.albums.iter().any(|a| a.subtitle.ends_with("Single")));
        assert!(page.albums.iter().all(|a| a.kind == CardKind::Album));
        assert!(
            page.albums.iter().all(|a| a.id.starts_with("MPREb_")),
            "no videos, playlists or related artists"
        );
    }

    #[tokio::test]
    async fn playlists_read_their_rows() {
        let page = fixture("ytm_catalog_playlist");
        let header = playlist_header(&page, &card(CardKind::Playlist, "VLx"));
        assert_eq!(header.title, "Presenting Daft Punk");
        let tracks = crawl(
            |_| std::future::ready(Ok(page.clone())),
            "VLx",
            TRACK_SHELVES,
            "musicResponsiveListItemRenderer",
            |row| song_row(row, None),
        )
        .await
        .unwrap();
        assert!(tracks.len() > 30);
        assert_eq!(tracks[0].title, "Technologic");
        assert_eq!(tracks[0].artist, "Daft Punk");
        assert!(tracks[0].duration > 0);
    }

    #[test]
    fn subtitles_read_like_the_other_services() {
        let s = |p: &[&str]| p.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            card_subtitle(CardKind::Album, &s(&["Single", "2023"])),
            "2023 · Single"
        );
        assert_eq!(
            card_subtitle(CardKind::Album, &s(&["EP", "Artist", "2020"])),
            "2020 · EP"
        );
        assert_eq!(
            card_subtitle(CardKind::Playlist, &s(&["Christian", "753 views"])),
            "Christian"
        );
    }
}
