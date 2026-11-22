use mosaic_core::model::{Source, Track, TrackId};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    Mp3,
    Aac,
    Opus,
}

#[derive(Clone, Debug)]
pub struct Transcoding {
    pub url: String,
    pub hls: bool,
    pub codec: Codec,
}

fn codec_of(mime: &str) -> Codec {
    if mime.contains("opus") || mime.contains("ogg") {
        Codec::Opus
    } else if mime.contains("mp4") || mime.contains("aac") {
        Codec::Aac
    } else {
        Codec::Mp3
    }
}

fn rank(t: &Transcoding) -> u8 {
    match (t.hls, t.codec) {
        (false, _) => 0,
        (true, Codec::Mp3) => 1,
        (true, Codec::Aac) => 2,
        (true, Codec::Opus) => 3,
    }
}

/// Full (non-snipped) streams, best first: progressive, then HLS mp3, aac, opus.
pub fn full_streams(transcodings: Option<&Value>) -> Vec<Transcoding> {
    let mut out: Vec<Transcoding> = transcodings
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| !t["snipped"].as_bool().unwrap_or(false))
        .filter_map(|t| {
            let protocol = t["format"]["protocol"].as_str()?;
            if protocol != "hls" && protocol != "progressive" {
                return None;
            }
            Some(Transcoding {
                url: t["url"].as_str()?.to_string(),
                hls: protocol == "hls",
                codec: codec_of(t["format"]["mime_type"].as_str().unwrap_or("")),
            })
        })
        .collect();
    out.sort_by_key(rank);
    out
}

pub fn slim_track(t: &Value) -> Option<Track> {
    let id = t["id"].as_u64()?;
    let art = t["artwork_url"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| t["user"]["avatar_url"].as_str())
        .unwrap_or("");
    let streamable = t["streamable"] != Value::Bool(false)
        && t["policy"].as_str() != Some("BLOCK")
        && !full_streams(t["media"].get("transcodings")).is_empty();

    Some(Track {
        id: TrackId::Num(id),
        source: Source::SoundCloud,
        title: t["title"].as_str()?.to_string(),
        artist: t["user"]["username"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string(),
        album: String::new(),
        artwork: art.replacen("-large", "-t500x500", 1),
        thumb: art.replacen("-large", "-t300x300", 1),
        duration: t["duration"].as_u64().unwrap_or(0),
        permalink: t["permalink_url"].as_str().map(str::to_string),
        streamable,
        folder: None,
        lrc_path: None,
        uri: None,
        artist_id: None,
        album_id: None,
        video_id: None,
        extra: Default::default(),
    })
}

pub fn playable(list: Option<&Value>) -> Vec<Track> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| {
            t["id"].as_u64().is_some_and(|id| id != 0)
                && t["title"].as_str().is_some_and(|s| !s.is_empty())
        })
        .filter_map(slim_track)
        .filter(|t| t.streamable)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn media() -> Value {
        json!({"transcodings": [
            {"url": "u-hls-opus", "snipped": false, "format": {"protocol": "hls", "mime_type": "audio/ogg; codecs=\"opus\""}},
            {"url": "u-hls-aac", "format": {"protocol": "hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\""}},
            {"url": "u-hls-mp3", "format": {"protocol": "hls", "mime_type": "audio/mpeg"}},
            {"url": "u-prog", "format": {"protocol": "progressive", "mime_type": "audio/mpeg"}},
            {"url": "u-snip", "snipped": true, "format": {"protocol": "progressive", "mime_type": "audio/mpeg"}},
            {"url": "u-other", "format": {"protocol": "dash", "mime_type": "audio/mpeg"}}
        ]})
    }

    #[test]
    fn stream_preference_order() {
        let urls: Vec<_> = full_streams(media().get("transcodings"))
            .into_iter()
            .map(|t| t.url)
            .collect();
        assert_eq!(urls, ["u-prog", "u-hls-mp3", "u-hls-aac", "u-hls-opus"]);
    }

    #[test]
    fn slim_track_maps_fields() {
        let raw = json!({"id": 42, "title": "Song", "user": {"username": "Art", "avatar_url": "https://a/avatar-large.jpg"},
            "artwork_url": "https://a/x-large.jpg", "duration": 123000, "permalink_url": "https://soundcloud.com/a/b", "media": media()});
        let t = slim_track(&raw).unwrap();
        assert_eq!(t.id, TrackId::Num(42));
        assert_eq!(t.artwork, "https://a/x-t500x500.jpg");
        assert_eq!(t.thumb, "https://a/x-t300x300.jpg");
        assert_eq!(t.artist, "Art");
        assert!(t.streamable);
        let no_art = json!({"id": 1, "title": "T", "user": {"avatar_url": "https://a/av-large.jpg"}, "media": media()});
        let t = slim_track(&no_art).unwrap();
        assert_eq!(
            (t.artist.as_str(), t.artwork.as_str()),
            ("Unknown", "https://a/av-t500x500.jpg")
        );
    }

    #[test]
    fn playable_filters_unplayable() {
        let list = json!([
            {"id": 1, "title": "ok", "media": media()},
            {"id": 2, "title": "blocked", "policy": "BLOCK", "media": media()},
            {"id": 3, "title": "not streamable", "streamable": false, "media": media()},
            {"id": 4, "title": "no media"},
            {"id": 5, "title": "", "media": media()},
            null,
            {"title": "no id", "media": media()}
        ]);
        let ids: Vec<_> = playable(Some(&list)).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, [TrackId::Num(1)]);
        assert!(playable(None).is_empty());
    }
}
