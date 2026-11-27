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
