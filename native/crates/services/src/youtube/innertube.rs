//! Stream resolution through InnerTube's `/player` endpoint, as the visionOS app calls it.
//!
//! That client gets plain `url`s (no signature cipher, no player script) and needs no sign-in,
//! so resolving is one POST. The web-client fallback (player-script solver, proof of origin) and
//! SABR streaming are not implemented; a video this client refuses reports why.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use mosaic_core::model::Track;

use super::api::youtube_track;
use crate::Services;

const ENDPOINT: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
const CLIENT_NAME_ID: &str = "101";
const CLIENT_VERSION: &str = "1.02";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

/// An audio-only format ready to hand to the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub url: String,
    pub mime: String,
    pub bitrate: u64,
    /// Seconds the URL stays valid for, as YouTube reports it.
    pub expires_in: u64,
}

fn context(visitor: &str) -> Value {
    json!({
        "client": {
            "visitorData": visitor,
            "clientName": "VISIONOS",
            "clientVersion": CLIENT_VERSION,
            "deviceMake": "Apple",
            "deviceModel": "RealityDevice17,1",
            "userAgent": USER_AGENT,
            "osName": "visionOS",
            "osVersion": "26.5.23O471",
            "hl": "en",
            "gl": "US",
        }
    })
}

const VISITOR_ENDPOINT: &str = "https://www.youtube.com/youtubei/v1/visitor_id?prettyPrint=false";
const SEARCH_ENDPOINT: &str = "https://www.youtube.com/youtubei/v1/search?prettyPrint=false";
/// The website's own client; the visionOS one gets no video results from `/search`. YouTube
/// accepts old versions of this for a long time, so it is only bumped if search starts failing.
const WEB_VERSION: &str = "2.20260101.00.00";
/// The "videos only" result filter.
const VIDEOS_ONLY: &str = "EgIQAQ==";
const SEARCH_LIMIT: usize = 20;

/// A video id is 11 URL-safe characters; anything else would only ever be a malformed request.
pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether a format is the plain stereo stream (not a loudness-boosted or dubbed variant).
fn is_main(format: &Value) -> bool {
    if format["isDrc"].as_bool().unwrap_or(false) {
        return false;
    }
    format["audioTrack"]["audioIsDefault"]
        .as_bool()
        .unwrap_or(true)
}

/// Opus in WebM first, AAC in MP4 as the fallback; the highest bitrate of each. The engine seeks
/// WebM over HTTP reliably, but not YouTube's fragmented MP4.
pub fn pick_audio(response: &Value) -> Option<Playback> {
    let expires_in = response["streamingData"]["expiresInSeconds"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| response["streamingData"]["expiresInSeconds"].as_u64())
        .unwrap_or(0);
    let formats = response["streamingData"]["adaptiveFormats"].as_array()?;
    formats
        .iter()
        .filter(|f| is_main(f))
        .filter_map(|f| {
            let mime = f["mimeType"].as_str()?;
            if !mime.starts_with("audio/") {
                return None;
            }
            Some(Playback {
                url: f["url"].as_str()?.to_string(),
                mime: mime.to_string(),
                bitrate: f["bitrate"].as_u64().unwrap_or(0),
                expires_in,
            })
        })
        .max_by_key(|p| (p.mime.starts_with("audio/webm"), p.bitrate))
}

/// Why `/player` won't give a stream for this video, as a message for the player bar.
pub fn refusal(response: &Value) -> String {
    let status = &response["playabilityStatus"];
    let reason = status["reason"]
        .as_str()
        .or_else(|| {
            status["errorScreen"]["playerErrorMessageRenderer"]["reason"]["simpleText"].as_str()
        })
        .unwrap_or("");
    match status["status"].as_str().unwrap_or("") {
        "LOGIN_REQUIRED" if reason.contains("bot") => {
            "YouTube asked for a sign-in to confirm you're not a bot. Try again later.".into()
        }
        "LOGIN_REQUIRED" => format!(
            "YouTube won't play this video without a sign-in{}",
            suffix(reason)
        ),
        "LIVE_STREAM_OFFLINE" => "This YouTube live stream isn't on right now".into(),
        _ if !reason.is_empty() => format!("YouTube: {reason}"),
        _ => "YouTube couldn't provide this video".into(),
    }
}

/// The "confirm you're not a bot" refusal, which a fresh visitor id can clear.
fn is_bot_check(response: &Value) -> bool {
    let status = &response["playabilityStatus"];
    status["status"].as_str() == Some("LOGIN_REQUIRED")
        && status["reason"].as_str().is_some_and(|r| r.contains("bot"))
}

fn suffix(reason: &str) -> String {
    if reason.is_empty() {
        String::new()
    } else {
        format!(" ({reason})")
    }
}

/// `4:02` / `1:27:57` to seconds; `None` for anything else (live streams have no length).
fn clock_seconds(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut parts = 0;
    for part in text.split(':') {
        total = total * 60 + part.trim().parse::<u64>().ok()?;
        parts += 1;
    }
    (parts > 0 && parts <= 3).then_some(total)
}

fn runs_text(node: &Value) -> String {
    node["runs"]
        .as_array()
        .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
        .or_else(|| node["simpleText"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn collect_videos<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            if let Some(video) = map.get("videoRenderer") {
                out.push(video);
            }
            map.values().for_each(|v| collect_videos(v, out));
        }
        Value::Array(list) => list.iter().for_each(|v| collect_videos(v, out)),
        _ => {}
    }
}

/// Playable videos out of a `/search` response, in order, without duplicates.
pub fn parse_search(response: &Value) -> Vec<Track> {
    let mut found = Vec::new();
    collect_videos(response, &mut found);
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .filter_map(|v| {
            let id = v["videoId"].as_str().filter(|id| valid_video_id(id))?;
            let seconds = clock_seconds(v["lengthText"]["simpleText"].as_str()?)?;
            if seconds == 0 || !seen.insert(id.to_string()) {
                return None;
            }
            let art = v["thumbnail"]["thumbnails"]
                .as_array()
                .and_then(|t| t.last())
                .and_then(|t| t["url"].as_str())
                .map_or_else(
                    || format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
                    str::to_string,
                );
            let channel = runs_text(&v["ownerText"]);
            Some(youtube_track(
                id,
                runs_text(&v["title"]),
                &channel,
                seconds * 1000,
                &art,
                &art,
            ))
        })
        .take(SEARCH_LIMIT)
        .collect()
}

impl Services {
    /// Searches the way the YouTube website does: no account, no API quota.
    pub async fn yt_search_public(&self, query: &str) -> Result<Vec<Track>> {
        let body = json!({
            "context": {"client": {
                "clientName": "WEB",
                "clientVersion": WEB_VERSION,
                "hl": "en",
                "gl": "US",
            }},
            "query": query,
            "params": VIDEOS_ONLY,
        });
        let res = self
            .http()
            .post(SEARCH_ENDPOINT)
            .header("Origin", "https://www.youtube.com")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| anyhow!("Could not reach YouTube: {e}"))?;
        let status = res.status();
        if !status.is_success() {
            bail!("YouTube said {}", status.as_u16());
        }
        let json: Value = res.json().await?;
        Ok(parse_search(&json))
    }

    /// The anonymous visitor id, fetched once. Without one YouTube answers most videos with a
    /// "confirm you're not a bot" refusal; with one the same request is served.
    async fn yt_visitor(&self, refresh: bool) -> Result<String> {
        if !refresh {
            if let Some(v) = self
                .inner
                .youtube
                .visitor
                .lock()
                .ok()
                .and_then(|v| v.clone())
            {
                return Ok(v);
            }
        }
        let body = json!({"context": {"client": {
            "clientName": "WEB",
            "clientVersion": WEB_VERSION,
            "hl": "en",
            "gl": "US",
        }}});
        let json: Value = self
            .http()
            .post(VISITOR_ENDPOINT)
            .header("Origin", "https://www.youtube.com")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| anyhow!("Could not reach YouTube: {e}"))?
            .json()
            .await?;
        let visitor = json["responseContext"]["visitorData"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("YouTube didn't start a session"))?
            .to_string();
        if let Ok(mut slot) = self.inner.youtube.visitor.lock() {
            *slot = Some(visitor.clone());
        }
        Ok(visitor)
    }

    async fn yt_player(&self, video_id: &str, visitor: &str) -> Result<Value> {
        let body = json!({
            "context": context(visitor),
            "videoId": video_id,
            "contentCheckOk": true,
            "racyCheckOk": true,
        });
        let res = self
            .http()
            .post(ENDPOINT)
            .header("X-YouTube-Client-Name", CLIENT_NAME_ID)
            .header("X-YouTube-Client-Version", CLIENT_VERSION)
            .header("X-Goog-Visitor-Id", visitor)
            .header("Origin", "https://www.youtube.com")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| anyhow!("Could not reach YouTube: {e}"))?;
        let status = res.status();
        if !status.is_success() {
            bail!("YouTube said {}", status.as_u16());
        }
        Ok(res.json().await?)
    }

    /// Resolves a video to a fresh audio URL. Nothing is cached: a URL that stopped working is
    /// re-resolved by the player, and an old one handed back would stop it again.
    pub async fn yt_stream(&self, video_id: &str) -> Result<Playback> {
        if !valid_video_id(video_id) {
            bail!("YouTube: this isn't a valid video");
        }
        let mut json = Value::Null;
        for attempt in 0..2 {
            let visitor = self.yt_visitor(attempt > 0).await?;
            json = self.yt_player(video_id, &visitor).await?;
            if let Some(playback) = pick_audio(&json) {
                return Ok(playback);
            }
            if !is_bot_check(&json) {
                break;
            }
        }
        Err(anyhow!(refusal(&json)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> Value {
        json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {
                "expiresInSeconds": "21540",
                "adaptiveFormats": [
                    {"itag": 137, "mimeType": "video/mp4; codecs=\"avc1\"", "bitrate": 4_000_000, "url": "https://v/137"},
                    {"itag": 139, "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 50_152, "url": "https://v/139"},
                    {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130_677, "url": "https://v/140"},
                    {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 136_544, "url": "https://v/251"},
                    {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 256_000, "url": "https://v/141-drc", "isDrc": true},
                    {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 130_000, "url": "https://v/dub",
                     "audioTrack": {"audioIsDefault": false}},
                    {"itag": 142, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 999_000, "signatureCipher": "s=x"},
                ]
            }
        })
    }

    #[test]
    fn picks_the_best_opus_stream_and_skips_video_drc_dubs_and_ciphers() {
        let p = pick_audio(&response()).expect("a format");
        assert_eq!(p.url, "https://v/251");
        assert_eq!(p.expires_in, 21540);
    }

    #[test]
    fn falls_back_to_aac_when_there_is_no_opus() {
        let mut r = response();
        r["streamingData"]["adaptiveFormats"]
            .as_array_mut()
            .unwrap()
            .retain(|f| !f["mimeType"].as_str().unwrap().starts_with("audio/webm"));
        assert_eq!(pick_audio(&r).unwrap().url, "https://v/140");
    }

    #[test]
    fn no_audio_formats_means_no_pick() {
        assert!(pick_audio(&json!({"playabilityStatus": {"status": "OK"}})).is_none());
    }

    #[test]
    fn refusals_read_as_messages() {
        let bot = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm you're not a bot"}});
        assert!(refusal(&bot).contains("not a bot"));
        let gone =
            json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": "Video unavailable"}});
        assert_eq!(refusal(&gone), "YouTube: Video unavailable");
        assert_eq!(refusal(&json!({})), "YouTube couldn't provide this video");
    }

    fn renderer(id: &str, length: Option<&str>) -> Value {
        let mut v = json!({
            "videoId": id,
            "title": {"runs": [{"text": "Xtal"}, {"text": " (HQ)"}]},
            "ownerText": {"runs": [{"text": "Aphex Twin - Topic"}]},
            "thumbnail": {"thumbnails": [{"url": "https://i/small"}, {"url": "https://i/big"}]},
        });
        if let Some(l) = length {
            v["lengthText"] = json!({"simpleText": l});
        }
        v
    }

    #[test]
    fn search_results_become_tracks() {
        let response = json!({"contents": {"sections": [
            {"itemSectionRenderer": {"contents": [
                {"videoRenderer": renderer("2tOutF8B3f8", Some("4:55"))},
                {"videoRenderer": renderer("lpmUtiSRT1A", Some("1:27:57"))},
                {"videoRenderer": renderer("2tOutF8B3f8", Some("4:55"))},
                {"videoRenderer": renderer("LIVEstream1", None)},
                {"videoRenderer": renderer("bad", Some("1:00"))},
                {"shelfRenderer": {"content": {"videoRenderer": renderer("shelfVideo1", Some("0:30"))}}},
            ]}}
        ]}});
        let tracks = parse_search(&response);
        let ids: Vec<String> = tracks.iter().map(|t| t.id.key()).collect();
        assert_eq!(ids, ["yt:2tOutF8B3f8", "yt:lpmUtiSRT1A", "yt:shelfVideo1"]);
        assert_eq!(tracks[0].title, "Xtal (HQ)");
        assert_eq!(tracks[0].artist, "Aphex Twin");
        assert_eq!(tracks[0].duration, 295_000);
        assert_eq!(tracks[1].duration, 5_277_000);
        assert_eq!(tracks[0].artwork, "https://i/big");
        assert_eq!(tracks[0].video_id.as_deref(), Some("2tOutF8B3f8"));
    }

    #[test]
    fn clock_text_reads_as_seconds() {
        assert_eq!(clock_seconds("4:02"), Some(242));
        assert_eq!(clock_seconds("1:27:57"), Some(5277));
        assert_eq!(clock_seconds("LIVE"), None);
        assert_eq!(clock_seconds(""), None);
    }

    #[test]
    fn only_the_bot_refusal_counts_as_a_bot_check() {
        let bot = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm you’re not a bot"}});
        assert!(is_bot_check(&bot));
        let age = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm your age"}});
        assert!(!is_bot_check(&age));
        assert!(!is_bot_check(
            &json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": "not a bot"}})
        ));
    }

    #[test]
    fn video_ids_are_checked() {
        assert!(valid_video_id("dQw4w9WgXcQ"));
        assert!(!valid_video_id("short"));
        assert!(!valid_video_id("dQw4w9WgXc/"));
    }
}
