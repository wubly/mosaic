//! Search and browsing that look the same for every service: songs, artists, albums and
//! playlists, and a page for any artist, album or playlist. Each service fills these in its own
//! `catalog.rs` (`youtube`, `soundcloud`, `monochrome`); Spotify maps its own cards.

use anyhow::{bail, Result};
use mosaic_core::model::{Source, Track};

use crate::Services;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardKind {
    Artist,
    Album,
    Playlist,
}

/// An artist, album or playlist in search results or on an artist page.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub source: Source,
    pub kind: CardKind,
    /// The service's own id, as its `*_open` takes it.
    pub id: String,
    pub title: String,
    /// Artist cards: "Artist" or a follower count; albums: "2021 · Artist"; playlists: owner.
    pub subtitle: String,
    pub image: String,
}

/// What a search found on one service.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub tracks: Vec<Track>,
    pub artists: Vec<Card>,
    pub albums: Vec<Card>,
    pub playlists: Vec<Card>,
}

/// An opened artist, album or playlist.
#[derive(Clone, Debug)]
pub struct Collection {
    pub card: Card,
    /// An album's or playlist's tracks; an artist's top songs.
    pub tracks: Vec<Track>,
    /// An artist's albums and singles (empty otherwise).
    pub albums: Vec<Card>,
    /// The artist behind an album, when the service links one.
    pub artist: Option<Card>,
}

impl Services {
    /// Songs, artists, albums and playlists for `query` on `source`.
    pub async fn search_all(&self, source: &Source, query: &str) -> Result<Found> {
        match source {
            Source::YouTube => self.yt_search_all(query).await,
            Source::SoundCloud => self.sc_search_all(query).await,
            Source::Monochrome => self.mono_search_all(query).await,
            other => bail!("{} has no catalog search", other.as_str()),
        }
    }

    /// The page behind a card.
    pub async fn open_card(&self, card: &Card) -> Result<Collection> {
        match card.source {
            Source::YouTube => self.yt_open(card).await,
            Source::SoundCloud => self.sc_open(card).await,
            Source::Monochrome => self.mono_open(card).await,
            ref other => bail!("{} has no catalog pages", other.as_str()),
        }
    }
}

/// The kinds of hits a search shows, in the order they're laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Songs,
    Artists,
    Albums,
    Playlists,
}

/// When nothing stands out, songs lead.
const DEFAULT_ORDER: [Section; 4] = [
    Section::Songs,
    Section::Artists,
    Section::Albums,
    Section::Playlists,
];
/// A section only moves up when its best hit matches the query at least this well.
const CONFIDENT: f32 = 0.5;

/// Lowercase words, without punctuation or accents, for comparing names.
fn norm(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// How well `name` matches the whole query: 1 when it's exactly what was typed, a little less
/// when what was typed is the start of it (still typing), else 0.
fn closeness(query: &str, name: &str) -> f32 {
    let name = norm(name);
    if name.is_empty() {
        return 0.0;
    }
    if name == query {
        1.0
    } else if query.chars().count() >= 3 && name.starts_with(query) {
        0.55 + 0.3 * query.len() as f32 / name.len() as f32
    } else {
        0.0
    }
}

/// `query` names both `a` and `b` ("daft punk one more time", either way round).
fn names_both(query: &str, a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (query == format!("{a} {b}") || query == format!("{b} {a}"))
}

/// The first artist of "A, B & C" or "A feat. B".
fn lead_artist(artists: &str) -> &str {
    artists
        .split([',', '&'])
        .next()
        .and_then(|a| a.split(" feat").next())
        .map_or(artists, str::trim)
}

fn score(query: &str, section: Section, found: &Found) -> f32 {
    const TOP: usize = 4;
    let best = |scores: &mut dyn Iterator<Item = f32>| scores.fold(0.0f32, f32::max);
    match section {
        Section::Artists => best(&mut found.artists.iter().take(TOP).map(|a| {
            // An exact artist name beats a song of the same name: that's who they're after.
            let c = closeness(query, &a.title);
            if c >= 1.0 {
                1.05
            } else {
                c
            }
        })),
        Section::Songs => best(&mut found.tracks.iter().take(TOP + 2).map(|t| {
            let title = crate::ranking::tidy_title(&t.title);
            if names_both(query, lead_artist(&t.artist), &title)
                || names_both(query, &t.artist, &title)
            {
                1.1
            } else {
                0.9 * closeness(query, &title)
            }
        })),
        Section::Albums => best(&mut found.albums.iter().take(TOP).map(|a| {
            // "2001 · Daft Punk": the part after the year names the artist, unless it's a kind.
            let artist = a.subtitle.rsplit(" · ").next().unwrap_or_default();
            let single = matches!(artist, "Single" | "EP");
            if !single && names_both(query, artist, &a.title) {
                1.05
            } else {
                // A single is usually its song; the song should come first then.
                closeness(query, &a.title) * if single { 0.85 } else { 0.95 }
            }
        })),
        Section::Playlists => {
            let asked = query
                .split(' ')
                .any(|w| matches!(w, "playlist" | "playlists"));
            if asked && !found.playlists.is_empty() {
                1.2
            } else {
                // Playlists borrow names freely ("ODESZA - A Moment Apart Remixes"), so only an
                // exact name moves them up.
                best(
                    &mut found
                        .playlists
                        .iter()
                        .take(TOP)
                        .map(|p| 0.5 * closeness(query, &p.title)),
                )
            }
        }
    }
}

/// The order to show a search's sections in: what the query most likely asks for first
/// (an artist's name puts artists first, "artist song" the songs, an album's title the albums),
/// the rest by how well they match, songs first when nothing stands out.
pub fn section_order(query: &str, found: &Found) -> [Section; 4] {
    let query = norm(query);
    let mut scored: Vec<(Section, f32)> = DEFAULT_ORDER
        .iter()
        .map(|s| (*s, score(&query, *s, found)))
        .map(|(s, v)| (s, if v >= CONFIDENT { v } else { 0.0 }))
        .collect();
    // Stable: ties keep the default order.
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out = DEFAULT_ORDER;
    for (slot, (section, _)) in out.iter_mut().zip(scored) {
        *slot = section;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mosaic_core::model::TrackId;

    fn card(kind: CardKind, title: &str, subtitle: &str) -> Card {
        Card {
            source: Source::YouTube,
            kind,
            id: title.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            image: String::new(),
        }
    }

    fn song(title: &str, artist: &str) -> Track {
        let mut t: Track = serde_json::from_value(serde_json::json!({
            "id": "x", "source": "youtube", "title": title, "artist": artist,
            "duration": 1000
        }))
        .unwrap();
        t.id = TrackId::Str(format!("yt:{title}"));
        t
    }

    fn daft_punk() -> Found {
        Found {
            tracks: vec![
                song("One More Time", "Daft Punk"),
                song(
                    "Get Lucky (feat. Pharrell Williams)",
                    "Daft Punk, Pharrell Williams",
                ),
                song("Blinding Lights", "The Weeknd"),
            ],
            artists: vec![
                card(CardKind::Artist, "Daft Punk", "Artist"),
                card(CardKind::Artist, "Thomas Bangalter", "Artist"),
            ],
            albums: vec![
                card(CardKind::Album, "Discovery", "2001 · Daft Punk"),
                card(CardKind::Album, "Blinding Lights", "2019 · Single"),
            ],
            playlists: vec![card(
                CardKind::Playlist,
                "Presenting Daft Punk",
                "YouTube Music",
            )],
        }
    }

    fn first(query: &str) -> Section {
        section_order(query, &daft_punk())[0]
    }

    #[test]
    fn the_section_a_query_names_comes_first() {
        assert_eq!(first("Daft Punk"), Section::Artists);
        assert_eq!(first("daft punk"), Section::Artists, "any case");
        assert_eq!(first("one more time"), Section::Songs);
        assert_eq!(first("Daft Punk - One More Time"), Section::Songs);
        assert_eq!(first("one more time daft punk"), Section::Songs);
        assert_eq!(
            first("get lucky"),
            Section::Songs,
            "a feat. tail doesn't matter"
        );
        assert_eq!(first("discovery"), Section::Albums);
        assert_eq!(first("daft punk discovery"), Section::Albums);
        assert_eq!(first("daft punk playlist"), Section::Playlists);
        assert_eq!(
            first("blinding lights"),
            Section::Songs,
            "a single doesn't beat its song"
        );
        let mut remixes = daft_punk();
        remixes.playlists = vec![card(CardKind::Playlist, "Daft Punk Remixes and More", "x")];
        assert_ne!(
            section_order("daft punk remixes", &remixes)[0],
            Section::Playlists,
            "a playlist named after the query's start isn't what was asked for"
        );
    }

    #[test]
    fn half_typed_names_count_and_nothing_much_keeps_songs_first() {
        assert_eq!(first("daft pu"), Section::Artists);
        assert_eq!(section_order("zzz", &daft_punk()), DEFAULT_ORDER);
        assert_eq!(section_order("daft punk", &Found::default()), DEFAULT_ORDER);
        // Every section once, whatever the order.
        let mut order = section_order("discovery", &daft_punk()).to_vec();
        order.sort_by_key(|s| *s as u8);
        assert_eq!(order, DEFAULT_ORDER);
    }

    #[test]
    fn names_compare_without_accents_or_punctuation() {
        assert_eq!(norm("Beyoncé — Halo!"), "beyonce halo");
        assert_eq!(lead_artist("Daft Punk, Pharrell Williams"), "Daft Punk");
        assert_eq!(lead_artist("Kanye West feat. Jay-Z"), "Kanye West");
    }
}
