//! Reads the public embed page of a playlist or album (at most 100 tracks, no login).

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use super::url::SpotifyRef;
use crate::Services;

static NEXT_DATA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<script id="__NEXT_DATA__" type="application/json">(.*?)</script>"#)
        .expect("static regex")
});

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteTrack {
    pub title: String,
    pub artist: String,
    pub duration: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub via: &'static str,
    pub name: String,
    pub owner: String,
    pub cover: String,
    pub tracks: Vec<RemoteTrack>,
}

pub fn parse_embed(html: &str) -> Option<Imported> {
    let data = NEXT_DATA.captures(html)?;
    let json: Value = serde_json::from_str(&data[1]).ok()?;
    let entity = json.pointer("/props/pageProps/state/data/entity")?;
    let list = entity
        .get("trackList")?
        .as_array()
        .filter(|l| !l.is_empty())?;
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    Some(Imported {
        via: "embed",
        name: [text(entity, "name"), text(entity, "title")]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| "Spotify playlist".into()),
        owner: text(entity, "subtitle"),
        cover: entity
            .pointer("/coverArt/sources/0/url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        tracks: list
            .iter()
            .map(|t| RemoteTrack {
                title: text(t, "title"),
                artist: text(t, "subtitle"),
                duration: t.get("duration").and_then(Value::as_u64).unwrap_or(0),
            })
            .filter(|t| !t.title.is_empty())
            .collect(),
    })
}

impl Services {
    pub async fn spotify_embed(&self, r: &SpotifyRef) -> Option<Imported> {
        let res = self
            .http()
            .get(format!(
                "https://open.spotify.com/embed/{}/{}",
                r.kind.as_str(),
                r.id
            ))
            .header("Accept-Language", "en")
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        parse_embed(&res.text().await.ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(entity: &str) -> String {
        format!(
            r#"<html><script id="__NEXT_DATA__" type="application/json">{{"props":{{"pageProps":{{"state":{{"data":{{"entity":{entity}}}}}}}}}}}</script></html>"#
        )
    }

    #[test]
    fn parses_a_playlist_entity() {
        let html = page(
            r#"{"name":"Road trip","subtitle":"alice","coverArt":{"sources":[{"url":"https://i.scdn.co/x"}]},
            "trackList":[{"title":"Song A","subtitle":"Artist A","duration":200000},{"title":"","subtitle":"x"},{"title":"Song B","subtitle":"B1, B2"}]}"#,
        );
        let got = parse_embed(&html).unwrap();
        assert_eq!(got.name, "Road trip");
        assert_eq!(got.owner, "alice");
        assert_eq!(got.cover, "https://i.scdn.co/x");
        assert_eq!(got.tracks.len(), 2);
        assert_eq!(
            got.tracks[1],
            RemoteTrack {
                title: "Song B".into(),
                artist: "B1, B2".into(),
                duration: 0
            }
        );
    }

    #[test]
    fn rejects_empty_or_missing_data() {
        assert!(parse_embed("<html></html>").is_none());
        assert!(parse_embed(&page(r#"{"name":"x","trackList":[]}"#)).is_none());
        assert!(parse_embed(&page(r#"{"name":"x"}"#)).is_none());
    }

    #[test]
    fn falls_back_to_title_and_a_default_name() {
        assert_eq!(
            parse_embed(&page(r#"{"title":"T","trackList":[{"title":"a"}]}"#))
                .unwrap()
                .name,
            "T"
        );
        assert_eq!(
            parse_embed(&page(r#"{"trackList":[{"title":"a"}]}"#))
                .unwrap()
                .name,
            "Spotify playlist"
        );
    }
}
