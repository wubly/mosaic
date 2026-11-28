//! Monochrome's side of `crate::catalog`: artists and releases (albums, EPs, singles). The
//! public catalog API has no playlist lookup, so its playlist hits aren't offered.

use anyhow::{bail, Result};
use mosaic_core::model::Source;
use serde_json::Value;

use super::{slim_all, str_of};
use crate::catalog::{Card, CardKind, Collection, Found};
use crate::Services;

const CARDS_PER_KIND: usize = 12;

fn artist_names(item: &Value) -> String {
    match item["artistNames"].as_array() {
        Some(names) => names
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        None => item["artists"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["displayName"].as_str().or_else(|| a["name"].as_str()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub fn artist_card(a: &Value) -> Option<Card> {
    let id = str_of(&a["artistId"]).or_else(|| str_of(&a["id"]))?;
    let name = a["displayName"]
        .as_str()
        .or_else(|| a["name"].as_str())
        .filter(|s| !s.is_empty())?;
    Some(Card {
        source: Source::Monochrome,
        kind: CardKind::Artist,
        id,
        title: name.to_string(),
        subtitle: "Artist".into(),
        image: a["avatar"].as_str().unwrap_or_default().to_string(),
    })
}

pub fn release_card(r: &Value) -> Option<Card> {
    let id = str_of(&r["releaseId"]).or_else(|| str_of(&r["id"]))?;
    let title = r["title"].as_str().filter(|s| !s.is_empty())?;
    let year: String = r["releaseDate"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(4)
        .collect();
    let artists = artist_names(r);
    let kind = match r["releaseType"].as_str() {
        Some("SINGLE") => "Single",
        Some("EP") => "EP",
        _ => artists.as_str(),
    };
    Some(Card {
        source: Source::Monochrome,
        kind: CardKind::Album,
        id,
        title: title.to_string(),
        subtitle: [year.as_str(), kind]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        image: r["artwork"].as_str().unwrap_or_default().to_string(),
    })
}

fn cards(list: &Value, to_card: fn(&Value) -> Option<Card>) -> Vec<Card> {
    let mut out: Vec<Card> = Vec::new();
    for card in list.as_array().into_iter().flatten().filter_map(to_card) {
        if !out.iter().any(|c| c.id == card.id) {
            out.push(card);
        }
    }
    out.truncate(CARDS_PER_KIND);
    out
}

/// An artist page: top tracks, then albums, EPs and singles, newest first within each.
pub fn parse_artist(json: &Value, card: &Card) -> Collection {
    let header = artist_card(json).unwrap_or_else(|| card.clone());
    let mut albums: Vec<Card> = Vec::new();
    for key in ["albums", "singles", "compilations"] {
        for c in cards(&json[key], release_card) {
            if !albums.iter().any(|a| a.id == c.id) {
                albums.push(c);
            }
        }
    }
    Collection {
        card: header,
        tracks: slim_all(&json["topTracks"]),
        albums,
        artist: None,
    }
}

pub fn parse_release(json: &Value, card: &Card) -> Collection {
    let header = release_card(json).unwrap_or_else(|| card.clone());
    let title = header.title.clone();
    let art = header.image.clone();
    let tracks = slim_all(&json["tracks"])
        .into_iter()
        .map(|mut t| {
            if t.album.is_empty() {
                t.album = title.clone();
            }
            if t.artwork.is_empty() {
                t.artwork = art.clone();
                t.thumb = art.clone();
            }
            t
        })
        .collect();
    Collection {
        card: header,
        tracks,
        albums: Vec::new(),
        artist: json["artists"].get(0).and_then(artist_card),
    }
}

impl Services {
    pub async fn mono_search_all(&self, query: &str) -> Result<Found> {
        let q = [
            ("q", query.to_string()),
            ("limit", CARDS_PER_KIND.to_string()),
        ];
        let (tracks, artists, releases) = tokio::join!(
            self.mono_search(query),
            self.mono_get("/search/artists", &q),
            self.mono_get("/search/releases", &q),
        );
        let or_empty = |r: Result<Value>, key: &str, f: fn(&Value) -> Option<Card>| {
            r.map(|v| cards(&v[key], f))
                .inspect_err(|e| tracing::warn!("Monochrome catalog search: {e}"))
                .unwrap_or_default()
        };
        Ok(Found {
            tracks: tracks?,
            artists: or_empty(artists, "artists", artist_card),
            albums: or_empty(releases, "releases", release_card),
            playlists: Vec::new(),
        })
    }

    pub async fn mono_open(&self, card: &Card) -> Result<Collection> {
        match card.kind {
            CardKind::Artist => {
                let json = self.mono_get(&format!("/artists/{}", card.id), &[]).await?;
                Ok(parse_artist(&json, card))
            }
            CardKind::Album => {
                let json = self
                    .mono_get(&format!("/releases/{}", card.id), &[])
                    .await?;
                Ok(parse_release(&json, card))
            }
            CardKind::Playlist => bail!("Monochrome playlists can't be opened"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn clicked(kind: CardKind) -> Card {
        Card {
            source: Source::Monochrome,
            kind,
            id: "x".into(),
            title: "clicked".into(),
            subtitle: String::new(),
            image: String::new(),
        }
    }

    #[test]
    fn search_hits_become_cards() {
        let a = artist_card(
            &json!({"id": "1535", "artistId": "1535", "name": "Daft Punk",
            "displayName": "Daft Punk", "avatar": "https://tracks.monochrome.st/a.jpg"}),
        )
        .unwrap();
        assert_eq!((a.id.as_str(), a.title.as_str()), ("1535", "Daft Punk"));
        let r = release_card(
            &json!({"id": "155", "releaseId": "155", "title": "Discovery",
            "artistNames": ["Daft Punk"], "releaseDate": "2001-03-12T00:00:00.000Z",
            "releaseType": "ALBUM", "artwork": "https://tracks.monochrome.st/r.jpg"}),
        )
        .unwrap();
        assert_eq!(r.subtitle, "2001 · Daft Punk");
        let single = release_card(&json!({"releaseId": "9", "title": "S",
            "releaseType": "SINGLE", "releaseDate": "2023-01-01"}))
        .unwrap();
        assert_eq!(single.subtitle, "2023 · Single");
    }

    #[test]
    fn artist_pages_merge_release_lists_once() {
        let album = json!({"releaseId": "1", "title": "A", "releaseType": "ALBUM",
            "artists": [{"artistId": "9", "displayName": "D"}]});
        let page = parse_artist(
            &json!({"artistId": "9", "displayName": "D",
                "topTracks": [{"trackId": "5", "title": "T", "duration": 200000,
                               "artists": [{"artistId": "9", "displayName": "D"}]}],
                "albums": [album.clone()], "singles": [album,
                    {"releaseId": "2", "title": "B", "releaseType": "SINGLE"}]}),
            &clicked(CardKind::Artist),
        );
        assert_eq!(page.card.title, "D");
        assert_eq!(page.tracks.len(), 1);
        let ids: Vec<_> = page.albums.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["1", "2"]);
    }

    #[test]
    fn release_tracks_take_the_release_title_and_art() {
        let page = parse_release(
            &json!({"releaseId": "1", "title": "Discovery", "artwork": "https://r.jpg",
                "artists": [{"artistId": "9", "displayName": "Daft Punk"}],
                "tracks": [{"trackId": "5", "title": "One More Time", "duration": 320357,
                            "artists": [{"artistId": "9", "displayName": "Daft Punk"}]}]}),
            &clicked(CardKind::Album),
        );
        assert_eq!(page.tracks[0].album, "Discovery");
        assert_eq!(page.tracks[0].artwork, "https://r.jpg");
        assert_eq!(page.artist.unwrap().id, "9");
    }
}
