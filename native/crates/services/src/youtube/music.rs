//! YouTube Music through InnerTube's WEB_REMIX client (the web player at music.youtube.com):
//! song search with real artist and album names, and radio ("similar tracks") from a video.
//! Playing a track still goes through `innertube::yt_stream`.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use mosaic_core::model::Track;
use serde_json::{json, Value};

use super::api::youtube_track;
use super::innertube::valid_video_id;
use crate::Services;

const HOST: &str = "https://music.youtube.com/youtubei/v1";
/// Any recent web-remix version is accepted; this is only bumped if music search starts failing.
const REMIX_VERSION: &str = "1.20260707.12.00";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
/// The "songs only" result filter.
const SONGS_ONLY: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
/// The "videos only" result filter.
const VIDEOS_ONLY: &str = "EgWKAQIQAWoKEAkQBRAKEAMQBA==";
/// How far a music video's length may be from the song's and still be taken for it.
const VIDEO_LENGTH_SLACK_MS: u64 = 20_000;
/// Radio from a video: the `RDAMVM` playlist is YouTube Music's "start radio".
const RADIO_PARAMS: &str = "wAEB";
const SEPARATOR: &str = " • ";

fn text_of(node: &Value) -> String {
    node["runs"]
        .as_array()
        .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
        .or_else(|| node["simpleText"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The runs of a subtitle (`Artist • Album • 3:45`) joined into its `•`-separated parts.
fn segments(runs: &Value) -> Vec<String> {
    let mut parts = vec![String::new()];
    for run in runs.as_array().into_iter().flatten() {
        let text = run["text"].as_str().unwrap_or_default();
        if text == SEPARATOR {
            parts.push(String::new());
        } else if let Some(last) = parts.last_mut() {
            last.push_str(text);
        }
    }
    parts.retain(|p| !p.is_empty());
    parts
}

fn clock_ms(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut parts = 0;
    for part in text.split(':') {
        total = total * 60 + part.trim().parse::<u64>().ok()?;
        parts += 1;
    }
    (parts > 0 && parts <= 3 && total > 0).then_some(total * 1000)
}

/// Google-hosted album art comes sized in the URL (`=w120-h120-…`); ask for the size wanted.
fn sized(url: &str, px: u32) -> String {
    match url.rsplit_once("=w") {
        Some((base, _)) => format!("{base}=w{px}-h{px}-l90-rj"),
        None => url.to_string(),
    }
}

fn art_of(thumbnails: &Value) -> (String, String) {
    let url = thumbnails
        .as_array()
        .and_then(|t| t.last())
        .and_then(|t| t["url"].as_str())
        .unwrap_or_default();
    (sized(url, 544), sized(url, 226))
}

fn collect<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
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

/// `Artist • Album • …` parts as (artist, album). Video-style subtitles carry a view count
/// where the album would be, so that isn't taken for one.
fn artist_and_album(parts: &[String]) -> (String, String) {
    let parts: Vec<&String> = parts
        .iter()
        .skip_while(|p| matches!(p.as_str(), "Song" | "Video"))
        .collect();
    let artist = parts.first().map(|p| p.to_string()).unwrap_or_default();
    let album = parts
        .get(1)
        .filter(|_| parts.len() >= 3)
        .filter(|p| !p.ends_with(" views") && clock_ms(p).is_none())
        .map(|p| p.to_string())
        .unwrap_or_default();
    (artist, album)
}

fn finish(id: &str, title: String, parts: &[String], ms: u64, thumbnails: &Value) -> Track {
    let (artist, album) = artist_and_album(parts);
    let (art, thumb) = art_of(thumbnails);
    let mut track = youtube_track(id, title, &artist, ms, &art, &thumb);
    track.album = album;
    track
}

/// Song hits out of a music `/search` response, in order, without duplicates.
pub fn parse_search(response: &Value) -> Vec<Track> {
    let mut items = Vec::new();
    collect(response, "musicResponsiveListItemRenderer", &mut items);
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter_map(|item| {
            let id = item["playlistItemData"]["videoId"]
                .as_str()
                .filter(|id| valid_video_id(id))?;
            let columns = item["flexColumns"].as_array()?;
            let column = |i: usize| {
                columns
                    .get(i)
                    .map(|c| &c["musicResponsiveListItemFlexColumnRenderer"])
            };
            let title = text_of(&column(0)?["text"]);
            let parts = segments(&column(1)?["text"]["runs"]);
            let ms = parts.last().and_then(|p| clock_ms(p))?;
            if title.is_empty() || !seen.insert(id.to_string()) {
                return None;
            }
            Some(finish(
                id,
                title,
                &parts,
                ms,
                &item["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"],
            ))
        })
        .collect()
}

/// The tracks of a `/next` response's queue (a radio), first the seed, then what follows.
pub fn parse_queue(response: &Value) -> Vec<Track> {
    let mut items = Vec::new();
    collect(response, "playlistPanelVideoRenderer", &mut items);
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter_map(|item| {
            let id = item["videoId"].as_str().filter(|id| valid_video_id(id))?;
            let ms = clock_ms(&text_of(&item["lengthText"]))?;
            if !seen.insert(id.to_string()) {
                return None;
            }
            Some(finish(
                id,
                text_of(&item["title"]),
                &segments(&item["longBylineText"]["runs"]),
                ms,
                &item["thumbnail"]["thumbnails"],
            ))
        })
        .collect()
}

/// Lowercase words only, so titles compare without punctuation, case or spacing getting in the way.
fn clean(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first of `A, B & C`: the artist a music video is filed under.
fn lead_artist(artists: &str) -> &str {
    artists.split([',', '&']).next().map_or(artists, str::trim)
}

fn has_words(haystack: &str, needle: &str) -> bool {
    !needle.is_empty() && format!(" {haystack} ").contains(&format!(" {needle} "))
}

/// Letters and digits only, for comparing channel names like "RickAstleyVEVO" with "Rick Astley".
fn squashed(s: &str) -> String {
    clean(s).replace(' ', "")
}

/// The video among `candidates` that is the same song as `track`, uploaded by the artist (a
/// fan upload is usually the album audio over a still picture). Its title must contain the
/// song's and the length must be close; failing that, one titled exactly like the song, or
/// called "official", counts at any length (a radio edit is shorter than the album cut).
/// Picking nothing beats picking a lyric video, a live take or a cover.
pub fn pick_video<'a>(track: &Track, candidates: &'a [Track]) -> Option<&'a Track> {
    let title = clean(&track.title);
    let artist = squashed(lead_artist(&track.artist));
    let by_artist = |c: &&Track| {
        c.video_id != track.video_id
            && !artist.is_empty()
            && squashed(&c.artist).contains(&artist)
            && has_words(&clean(&c.title), &title)
    };
    candidates
        .iter()
        .filter(by_artist)
        .find(|c| c.duration.abs_diff(track.duration) <= VIDEO_LENGTH_SLACK_MS)
        .or_else(|| {
            candidates
                .iter()
                .filter(by_artist)
                .find(|c| clean(&c.title) == title || has_words(&clean(&c.title), "official"))
        })
}

/// `MUSIC_VIDEO_TYPE_*` of the first (the requested) item in a `/next` response.
fn video_type(response: &Value) -> Option<&str> {
    match response {
        Value::Object(map) => map
            .get("musicVideoType")
            .and_then(Value::as_str)
            .or_else(|| map.values().find_map(video_type)),
        Value::Array(list) => list.iter().find_map(video_type),
        _ => None,
    }
}

impl Services {
    async fn music_post(&self, endpoint: &str, mut body: Value) -> Result<Value> {
        body["context"] = json!({"client": {
            "clientName": "WEB_REMIX",
            "clientVersion": REMIX_VERSION,
            "hl": "en",
            "gl": "US",
        }});
        let res = self
            .http()
            .post(format!("{HOST}/{endpoint}?prettyPrint=false"))
            .header("Origin", "https://music.youtube.com")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| anyhow!("Could not reach YouTube Music: {e}"))?;
        let status = res.status();
        if !status.is_success() {
            bail!("YouTube Music said {}", status.as_u16());
        }
        Ok(res.json().await?)
    }

    /// Songs matching `query` on YouTube Music.
    pub async fn yt_search_music(&self, query: &str) -> Result<Vec<Track>> {
        let json = self
            .music_post("search", json!({"query": query, "params": SONGS_ONLY}))
            .await?;
        Ok(parse_search(&json))
    }

    /// The id of a video to watch for `track`, if there is one. A track that is itself a video
    /// is its own; an audio-only YouTube Music song has none of its own, so its official video
    /// is looked for among YouTube Music's videos.
    pub async fn yt_video_for(&self, track: &Track) -> Result<Option<String>> {
        let id = track
            .video_id
            .as_deref()
            .filter(|id| valid_video_id(id))
            .ok_or_else(|| anyhow!("YouTube: this isn't a valid video"))?;
        let response = self.music_post("next", json!({"videoId": id})).await?;
        if video_type(&response) != Some("MUSIC_VIDEO_TYPE_ATV") {
            return Ok(Some(id.to_string()));
        }
        let query = format!("{} {}", lead_artist(&track.artist), track.title);
        let found = self
            .music_post("search", json!({"query": query, "params": VIDEOS_ONLY}))
            .await?;
        Ok(pick_video(track, &parse_search(&found)).and_then(|v| v.video_id.clone()))
    }

    /// Tracks YouTube Music's radio plays after `video_id`, not including it.
    pub async fn yt_related(&self, video_id: &str) -> Result<Vec<Track>> {
        if !valid_video_id(video_id) {
            bail!("YouTube: this isn't a valid video");
        }
        let json = self
            .music_post(
                "next",
                json!({
                    "videoId": video_id,
                    "playlistId": format!("RDAMVM{video_id}"),
                    "isAudioOnly": true,
                    "params": RADIO_PARAMS,
                }),
            )
            .await?;
        Ok(parse_queue(&json)
            .into_iter()
            .filter(|t| t.video_id.as_deref() != Some(video_id))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(parts: &[&str]) -> Value {
        json!(parts.iter().map(|t| json!({"text": t})).collect::<Vec<_>>())
    }

    fn item(id: &str, title: &str, sub: &[&str]) -> Value {
        json!({"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": id},
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs(sub)}}},
            ],
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "https://lh3.googleusercontent.com/abc=w60-h60-l90-rj"},
                {"url": "https://lh3.googleusercontent.com/abc=w120-h120-l90-rj"},
            ]}}},
        }})
    }

    #[test]
    fn music_search_hits_become_tracks_with_artist_and_album() {
        let response = json!({"contents": [
            item("sWcLccMuCA8", "Xtal", &["Aphex Twin", " • ", "Selected Ambient Works 85-92", " • ", "4:54"]),
            item("q6sQeUkyWPc", "Evil at Play", &["Steven Jeffries", ", ", "Mary Carewe", " & ", "Donald Greig", " • ", "Glistens", " • ", "1:09"]),
            item("sWcLccMuCA8", "Xtal again", &["Aphex Twin", " • ", "4:54"]),
            item("noLengthxxx", "Nope", &["Someone", " • ", "Album"]),
            item("singleXXXXX", "Single", &["Solo", " • ", "2:00"]),
        ]});
        let tracks = parse_search(&response);
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].id.key(), "yt:sWcLccMuCA8");
        assert_eq!(tracks[0].artist, "Aphex Twin");
        assert_eq!(tracks[0].album, "Selected Ambient Works 85-92");
        assert_eq!(tracks[0].duration, 294_000);
        assert_eq!(
            tracks[0].artwork,
            "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
        assert_eq!(
            tracks[1].artist,
            "Steven Jeffries, Mary Carewe & Donald Greig"
        );
        assert_eq!(tracks[1].album, "Glistens");
        assert_eq!(tracks[2].album, "", "a two-part subtitle has no album");
    }

    fn panel(id: &str, sub: &[&str], length: &str) -> Value {
        json!({"playlistPanelVideoRenderer": {
            "videoId": id,
            "title": {"runs": [{"text": "Flim"}]},
            "longBylineText": {"runs": runs(sub)},
            "lengthText": {"runs": [{"text": length}]},
            "thumbnail": {"thumbnails": [{"url": "https://lh3.googleusercontent.com/x=w544-h544-l90-rj"}]},
        }})
    }

    #[test]
    fn radio_queue_parses_and_does_not_mistake_views_for_an_album() {
        let response = json!({"tabs": [{"content": {"contents": [
            panel("-yNHlKAzyVA", &["Aphex Twin", " • ", "Come To Daddy", " • ", "1997"], "2:58"),
            panel("viewsvideo1", &["Some Channel", " • ", "5.5M views", " • ", "2010"], "3:00"),
            panel("-yNHlKAzyVA", &["dup", " • ", "x", " • ", "1"], "1:00"),
            panel("nolengthxxx", &["A", " • ", "B", " • ", "C"], "LIVE"),
        ]}}]});
        let tracks = parse_queue(&response);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].album, "Come To Daddy");
        assert_eq!(tracks[0].duration, 178_000);
        assert_eq!(tracks[1].artist, "Some Channel");
        assert_eq!(tracks[1].album, "");
    }

    fn song(id: &str, title: &str, artist: &str, secs: u64) -> Track {
        let mut t = youtube_track(id, title.into(), artist, secs * 1000, "", "");
        t.video_id = Some(id.into());
        t
    }

    #[test]
    fn a_song_gets_the_artists_own_video_of_about_the_same_length() {
        let track = song("atvAtvAtvAt", "Around the World", "Daft Punk", 429);
        let wrong_length = song("shortEdit00", "Around the World (Live)", "Daft Punk", 238);
        let lyric = song(
            "lyricVideo1",
            "Daft Punk - Around the World (Lyrics)",
            "Some Fan",
            429,
        );
        let cover = song("cover000000", "Around the World (cover)", "Other Band", 430);
        let exact = song("exactMatch1", "Around the World", "Daft Punk", 424);
        assert_eq!(
            pick_video(&track, &[wrong_length, lyric, cover, exact])
                .unwrap()
                .video_id
                .as_deref(),
            Some("exactMatch1"),
            "only the artist's own upload of about the same length qualifies"
        );
    }

    #[test]
    fn an_exactly_titled_upload_by_the_artist_counts_at_any_length() {
        let track = song("atvAtvAtvAt", "Around the World", "Daft Punk", 429);
        let video = song("K0HSD_i2DvA", "Around the World", "Daft Punk", 242);
        let remix = song(
            "remix000000",
            "Around the World (Westend Extended Remix)",
            "Daft Punk",
            304,
        );
        assert_eq!(
            pick_video(&track, &[remix.clone(), video])
                .unwrap()
                .video_id
                .as_deref(),
            Some("K0HSD_i2DvA")
        );
        assert!(pick_video(&track, &[remix]).is_none());
    }

    #[test]
    fn an_official_video_counts_even_at_another_length() {
        let track = song("atvAtvAtvAt", "Around the World", "Daft Punk", 429);
        let radio_edit = song(
            "officialVid",
            "Around the World (Official Music Video)",
            "Daft Punk",
            238,
        );
        let live = song("live0000000", "Around the World (Live)", "Daft Punk", 238);
        assert_eq!(
            pick_video(&track, &[live.clone(), radio_edit])
                .unwrap()
                .video_id
                .as_deref(),
            Some("officialVid")
        );
        assert!(
            pick_video(&track, &[live]).is_none(),
            "a live take isn't it"
        );
    }

    #[test]
    fn vevo_style_channels_match_and_fan_uploads_and_the_song_itself_do_not() {
        let track = song("atvAtvAtvAt", "Xtal", "Aphex Twin", 294);
        let fan = song("fanUpload000", "Aphex Twin - Xtal (HQ)", "pigsfood27", 295);
        let itself = song("atvAtvAtvAt", "Xtal", "Aphex Twin", 294);
        assert!(pick_video(&track, &[fan, itself]).is_none());
        let rick = song("rickAtvAtvA", "Never Gonna Give You Up", "Rick Astley", 213);
        let vevo = song(
            "dQw4w9WgXcQ",
            "Never Gonna Give You Up",
            "RickAstleyVEVO",
            213,
        );
        assert_eq!(
            pick_video(&rick, &[vevo]).unwrap().video_id.as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert!(pick_video(&track, &[]).is_none());
    }

    #[test]
    fn titles_match_on_whole_words_and_the_lead_artist_leads() {
        assert!(has_words("daft punk around the world", "around the world"));
        assert!(!has_words("a party tonight", "art"));
        assert!(!has_words("anything", ""));
        assert_eq!(
            lead_artist("Steven Jeffries, Mary Carewe & Donald Greig"),
            "Steven Jeffries"
        );
        assert_eq!(lead_artist("Aphex Twin"), "Aphex Twin");
    }

    #[test]
    fn the_requested_items_video_type_is_found() {
        let atv = json!({"contents": {"x": [{"musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}, {"musicVideoType": "MUSIC_VIDEO_TYPE_OMV"}]}});
        assert_eq!(video_type(&atv), Some("MUSIC_VIDEO_TYPE_ATV"));
        assert_eq!(video_type(&json!({"a": 1})), None);
    }

    #[test]
    fn art_urls_are_resized_and_other_urls_left_alone() {
        assert_eq!(
            sized("https://g/x=w60-h60-l90-rj", 544),
            "https://g/x=w544-h544-l90-rj"
        );
        assert_eq!(
            sized("https://i.ytimg.com/vi/id/hq.jpg", 544),
            "https://i.ytimg.com/vi/id/hq.jpg"
        );
    }
}
