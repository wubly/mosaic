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
/// Pages of a radio read for autoplay (about 50 songs each).
const RADIO_PAGES: usize = 3;
const SEPARATOR: &str = " • ";

pub(super) fn text_of(node: &Value) -> String {
    node["runs"]
        .as_array()
        .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
        .or_else(|| node["simpleText"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The runs of a subtitle (`Artist • Album • 3:45`) joined into its `•`-separated parts.
pub(super) fn segments(runs: &Value) -> Vec<String> {
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

pub(super) fn clock_ms(text: &str) -> Option<u64> {
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
        Some((base, size)) if size.starts_with(|c: char| c.is_ascii_digit()) => {
            format!("{base}=w{px}-h{px}-l90-rj")
        }
        _ => url.to_string(),
    }
}

pub(super) fn art_of(thumbnails: &Value) -> (String, String) {
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

/// A play or view count in text like `3.7B plays`, `21K views`, `1,234 views` or `1.2M`
/// (also `mln`/`tys`-style suffixes). `None` when the text isn't a count.
pub fn parse_count(text: &str) -> Option<u64> {
    let lower = text.trim().to_lowercase();
    let body = ["plays", "play", "views", "view", "wyświetleń", "odtworzeń"]
        .iter()
        .find_map(|w| lower.strip_suffix(w))
        .unwrap_or(&lower)
        .trim();
    let split = body
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c.is_whitespace()))
        .unwrap_or(body.len());
    let (number, suffix) = body.split_at(split);
    let multiplier: f64 = match suffix.trim() {
        "" => 1.0,
        "k" | "tys" | "tys." => 1e3,
        "m" | "mln" | "mio" => 1e6,
        "b" | "bn" | "mld" | "mrd" => 1e9,
        "t" => 1e12,
        _ => return None,
    };
    let number = number.trim();
    if !number.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let value = if multiplier > 1.0 {
        // A decimal mark: `1.2M` or `1,2 mln`.
        number
            .replace(',', ".")
            .replace(char::is_whitespace, "")
            .parse::<f64>()
            .ok()?
    } else {
        // Thousands separators: `1,234,567`.
        number
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse::<f64>()
            .ok()?
    };
    Some((value * multiplier).round() as u64)
}

/// The count among a subtitle's parts, or else in `column`; `0` when there is none.
fn count_of(parts: &[String], column: &str) -> u64 {
    let counted = |p: &str| {
        let l = p.to_lowercase();
        ["plays", "views", "play", "view"]
            .iter()
            .any(|w| l.ends_with(&format!(" {w}")))
            .then(|| parse_count(p))
            .flatten()
    };
    parts
        .iter()
        .find_map(|p| counted(p))
        .or_else(|| counted(column))
        .unwrap_or(0)
}

/// The first artist and album pages a row links to (its subtitle's names are links), so a
/// track can lead to its artist's and album's pages.
pub(super) fn link_ids(node: &Value) -> (Option<String>, Option<String>) {
    fn walk(node: &Value, artist: &mut Option<String>, album: &mut Option<String>) {
        match node {
            Value::Object(map) => {
                if let Some(browse) = map.get("browseEndpoint") {
                    let id = browse["browseId"].as_str().map(str::to_string);
                    match browse["browseEndpointContextSupportedConfigs"]
                        ["browseEndpointContextMusicConfig"]["pageType"]
                        .as_str()
                    {
                        Some("MUSIC_PAGE_TYPE_ARTIST") if artist.is_none() => *artist = id,
                        Some("MUSIC_PAGE_TYPE_ALBUM") if album.is_none() => *album = id,
                        _ => {}
                    }
                }
                map.values().for_each(|v| walk(v, artist, album));
            }
            Value::Array(list) => list.iter().for_each(|v| walk(v, artist, album)),
            _ => {}
        }
    }
    let (mut artist, mut album) = (None, None);
    walk(node, &mut artist, &mut album);
    (artist, album)
}

/// `track` with the artist and album pages `node` links to.
pub(super) fn linked(mut track: Track, node: &Value) -> Track {
    let (artist, album) = link_ids(node);
    track.artist_id = track.artist_id.or(artist);
    track.album_id = track.album_id.or(album);
    track
}

fn finish(id: &str, title: String, parts: &[String], ms: u64, thumbnails: &Value) -> Track {
    let (artist, album) = artist_and_album(parts);
    let (art, thumb) = art_of(thumbnails);
    let mut track = youtube_track(id, title, &artist, ms, &art, &thumb);
    track.album = album;
    track
}

/// What a result says it is, from the label YouTube Music puts before the artist
/// (`Song • Artist`). The "Songs" filter's own rows carry no label.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Song,
    Video,
    Unlabeled,
}

/// Rows with a video id that aren't music at all.
const NOT_MUSIC_LABELS: &[&str] = &["Episode", "Podcast", "Profile"];

/// The length in a subtitle's parts: the last one that reads as a clock (`3:21`). A bare number
/// is a year or a count, never a length, and a missing length is `0`.
fn duration_of(parts: &[String]) -> u64 {
    parts
        .iter()
        .rev()
        .filter(|p| p.contains(':'))
        .find_map(|p| clock_ms(p))
        .unwrap_or(0)
}

fn kind_of(parts: &[String]) -> Option<Kind> {
    match parts.first().map(String::as_str) {
        Some("Song") => Some(Kind::Song),
        Some("Video") => Some(Kind::Video),
        Some(label) if NOT_MUSIC_LABELS.contains(&label) => None,
        _ => Some(Kind::Unlabeled),
    }
}

/// `collect`, but not into the top-result card (its `contents` are other videos).
fn collect_outside_card<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                out.push(found);
            }
            map.iter()
                .filter(|(k, _)| k.as_str() != "musicCardShelfRenderer")
                .for_each(|(_, v)| collect_outside_card(v, key, out));
        }
        Value::Array(list) => list.iter().for_each(|v| collect_outside_card(v, key, out)),
        _ => {}
    }
}

/// The playable rows of a music `/search` response, in order, without duplicates. A row needs
/// a video id and a title; its length is `0` when YouTube doesn't give one (the unfiltered
/// search leaves it out for songs).
fn parse_rows(response: &Value) -> Vec<(Kind, Track, u64)> {
    let mut items = Vec::new();
    collect_outside_card(response, "musicResponsiveListItemRenderer", &mut items);
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
            let kind = kind_of(&parts)?;
            if title.is_empty() || !seen.insert(id.to_string()) {
                return None;
            }
            let ms = duration_of(&parts);
            let plays = count_of(
                &parts,
                &column(2).map(|c| text_of(&c["text"])).unwrap_or_default(),
            );
            Some((
                kind,
                linked(
                    finish(
                        id,
                        title,
                        &parts,
                        ms,
                        &item["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"],
                    ),
                    &item["flexColumns"],
                ),
                plays,
            ))
        })
        .collect()
}

fn watch_video_id(endpoint: &Value) -> Option<&str> {
    endpoint["watchEndpoint"]["videoId"]
        .as_str()
        .filter(|id| valid_video_id(id))
}

/// The top-result card (the exact song or its video, usually) of an unfiltered `/search`
/// response, if it is a song or a video.
fn parse_card(response: &Value) -> Option<(Kind, Track, u64)> {
    let mut cards = Vec::new();
    collect(response, "musicCardShelfRenderer", &mut cards);
    let card = cards.first()?;
    let id = watch_video_id(&card["title"]["runs"][0]["navigationEndpoint"])
        .or_else(|| watch_video_id(&card["onTap"]))
        .or_else(|| {
            card["buttons"].as_array()?.iter().find_map(|b| {
                watch_video_id(&b["buttonRenderer"]["command"])
                    .or_else(|| watch_video_id(&b["buttonRenderer"]["navigationEndpoint"]))
            })
        })?;
    let title = text_of(&card["title"]);
    let parts = segments(&card["subtitle"]["runs"]);
    let kind = kind_of(&parts).filter(|k| *k != Kind::Unlabeled)?;
    if title.is_empty() {
        return None;
    }
    let ms = duration_of(&parts);
    let plays = count_of(&parts, "");
    Some((
        kind,
        linked(
            finish(
                id,
                title,
                &parts,
                ms,
                &card["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"],
            ),
            &card["subtitle"],
        ),
        plays,
    ))
}

/// Song and video hits out of a music `/search` response: the top-result card first, then the
/// list rows in order, without duplicates.
pub fn parse_search(response: &Value) -> Vec<Track> {
    let mut out: Vec<Track> = Vec::new();
    for track in parse_card(response)
        .into_iter()
        .chain(parse_rows(response))
        .map(|(_, t, _)| t)
    {
        if !out.iter().any(|t| t.video_id == track.video_id) {
            out.push(track);
        }
    }
    out
}

/// A title without video tags and a "(Digital Remaster)" tail, so a song and its music video
/// compare equal.
fn base_title(title: &str) -> String {
    static REMASTER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = REMASTER.get_or_init(|| {
        regex::Regex::new(r"(?i)\s*[\(\[][^\)\]]*remaster[^\)\]]*[\)\]]\s*$").expect("valid regex")
    });
    let tidy = crate::ranking::tidy_title(title);
    clean(&re.replace(&tidy, ""))
}

fn same_song(a: &Track, b: &Track) -> bool {
    let artist = |t: &Track| squashed(lead_artist(&t.artist));
    let title = base_title(&a.title);
    !title.is_empty() && title == base_title(&b.title) && artist(a) == artist(b)
}

/// The unfiltered search (`all`) and the "Songs" search (`songs`) merged: the song or video the
/// top-result card names first (the studio song when the card is its music video), then the
/// songs-only hits, then the songs only the unfiltered search found. Each video once, with a
/// length from whichever response had one.
pub fn merge_search(all: &Value, songs: &Value) -> Vec<Track> {
    merge_search_counted(all, songs)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

/// `merge_search` with each hit's play or view count (`0` when unknown).
pub fn merge_search_counted(all: &Value, songs: &Value) -> Vec<(Track, u64)> {
    let mut rest: Vec<(Track, u64)> = Vec::new();
    let extras = parse_rows(all)
        .into_iter()
        .filter(|(kind, _, _)| *kind == Kind::Song);
    for (track, plays) in parse_rows(songs)
        .into_iter()
        .chain(extras)
        .map(|(_, t, p)| (t, p))
    {
        match rest.iter_mut().find(|(t, _)| t.video_id == track.video_id) {
            Some((seen, seen_plays)) => {
                *seen_plays = (*seen_plays).max(plays);
                if seen.duration == 0 {
                    seen.duration = track.duration;
                }
                if seen.album.is_empty() {
                    seen.album = track.album;
                }
            }
            None => rest.push((track, plays)),
        }
    }
    let Some((kind, mut card, mut card_plays)) = parse_card(all) else {
        return rest;
    };
    let mut out = Vec::with_capacity(rest.len() + 1);
    if let Some(i) = rest.iter().position(|(t, _)| t.video_id == card.video_id) {
        let (seen, plays) = rest.remove(i);
        card_plays = card_plays.max(plays);
        if card.duration == 0 {
            card.duration = seen.duration;
        }
        if card.album.is_empty() {
            card.album = seen.album;
        }
    }
    if kind == Kind::Video {
        if let Some(i) = rest.iter().position(|(t, _)| same_song(t, &card)) {
            out.push(rest.remove(i));
        }
    }
    out.push((card, card_plays));
    out.extend(rest);
    out
}

/// What a search suggestion leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuggestionKind {
    /// A completed search query: `text` is what to search for.
    Query,
    Song,
    Video,
    Artist,
    Album,
}

/// One entry of the search box's drop-down.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Suggestion {
    /// The query to search for (`Query`), or the title or name of what it jumps to.
    pub text: String,
    pub kind: SuggestionKind,
    /// The artist for a song or video, the artist or year for an album; empty for a query.
    pub subtitle: String,
    /// Small cover art or portrait; empty for a query.
    pub thumb: String,
    /// The video to play, for a song or a video.
    pub video_id: Option<String>,
    /// The page to open, for an artist or an album.
    pub browse_id: Option<String>,
}

fn suggestion_entity(item: &Value) -> Option<Suggestion> {
    let columns = item["flexColumns"].as_array()?;
    let column = |i: usize| {
        columns
            .get(i)
            .map(|c| &c["musicResponsiveListItemFlexColumnRenderer"]["text"])
    };
    let text = text_of(column(0)?);
    if text.is_empty() {
        return None;
    }
    let parts = segments(column(1).map_or(&Value::Null, |c| &c["runs"]));
    let endpoint = &item["navigationEndpoint"];
    let video_id = watch_video_id(endpoint)
        .or_else(|| {
            item["playlistItemData"]["videoId"]
                .as_str()
                .filter(|id| valid_video_id(id))
        })
        .map(str::to_string);
    let browse = &endpoint["browseEndpoint"];
    let page_type = browse["browseEndpointContextSupportedConfigs"]
        ["browseEndpointContextMusicConfig"]["pageType"]
        .as_str()
        .unwrap_or_default();
    let kind = match page_type {
        "MUSIC_PAGE_TYPE_ARTIST" => SuggestionKind::Artist,
        "MUSIC_PAGE_TYPE_ALBUM" => SuggestionKind::Album,
        _ if video_id.is_none() => return None,
        _ if parts.first().is_some_and(|p| p == "Song") => SuggestionKind::Song,
        _ => SuggestionKind::Video,
    };
    // `Song • Artist • 3.7B plays` and `542M monthly audience`: only the names are of use.
    let subtitle = parts
        .iter()
        .skip_while(|p| matches!(p.as_str(), "Song" | "Video" | "Album" | "Artist"))
        .filter(|p| {
            !["plays", "views", "audience", "subscribers"]
                .iter()
                .any(|w| p.ends_with(w))
        })
        .cloned()
        .collect::<Vec<_>>()
        .join(SEPARATOR);
    let playable = matches!(kind, SuggestionKind::Song | SuggestionKind::Video);
    Some(Suggestion {
        text,
        kind,
        subtitle,
        thumb: art_of(&item["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]).1,
        video_id: video_id.filter(|_| playable),
        browse_id: browse["browseId"].as_str().map(str::to_string),
    })
}

/// The suggestions of a `get_search_suggestions` response, in order: text completions, then
/// the songs, videos, artists and albums YouTube Music would jump to. Anything else is left out.
pub fn parse_suggestions(response: &Value) -> Vec<Suggestion> {
    let mut sections = Vec::new();
    collect(response, "searchSuggestionsSectionRenderer", &mut sections);
    let mut out: Vec<Suggestion> = Vec::new();
    for entry in sections
        .into_iter()
        .filter_map(|s| s["contents"].as_array())
        .flatten()
    {
        let suggestion = if let Some(q) = entry.get("searchSuggestionRenderer") {
            let text = q["navigationEndpoint"]["searchEndpoint"]["query"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| text_of(&q["suggestion"]));
            (!text.is_empty()).then(|| Suggestion {
                text,
                kind: SuggestionKind::Query,
                subtitle: String::new(),
                thumb: String::new(),
                video_id: None,
                browse_id: None,
            })
        } else {
            entry
                .get("musicResponsiveListItemRenderer")
                .and_then(suggestion_entity)
        };
        if let Some(s) = suggestion {
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// The token for a radio's next page, if it has one.
fn radio_continuation(response: &Value) -> Option<String> {
    let mut found = Vec::new();
    collect(response, "nextRadioContinuationData", &mut found);
    found
        .first()
        .and_then(|c| c["continuation"].as_str())
        .map(str::to_string)
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
            Some(linked(
                finish(
                    id,
                    text_of(&item["title"]),
                    &segments(&item["longBylineText"]["runs"]),
                    ms,
                    &item["thumbnail"]["thumbnails"],
                ),
                &item["longBylineText"],
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
    pub(crate) async fn music_post(&self, endpoint: &str, mut body: Value) -> Result<Value> {
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

    /// Songs matching `query` on YouTube Music: the top result first, then the "Songs" search
    /// (which alone leaves out the exact song when the query names its artist) and what only
    /// the unfiltered search found. Either search may fail as long as the other works.
    pub async fn yt_search_music(&self, query: &str) -> Result<Vec<Track>> {
        let counted = self.yt_search_music_counted(query).await?;
        Ok(counted.into_iter().map(|(t, _)| t).collect())
    }

    /// `yt_search_music` with each hit's play or view count (`0` when unknown).
    pub async fn yt_search_music_counted(&self, query: &str) -> Result<Vec<(Track, u64)>> {
        let (all, songs) = tokio::join!(
            self.music_post("search", json!({"query": query})),
            self.music_post("search", json!({"query": query, "params": SONGS_ONLY})),
        );
        if let (Err(e), Err(_)) = (&all, &songs) {
            bail!("{e}");
        }
        Ok(merge_search_counted(
            &all.unwrap_or(Value::Null),
            &songs.unwrap_or(Value::Null),
        ))
    }

    /// What YouTube Music suggests for the start of a search: completed queries first, then the
    /// songs, videos, artists and albums it would jump straight to.
    pub async fn yt_search_suggestions(&self, query: &str) -> Result<Vec<Suggestion>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let json = self
            .music_post("music/get_search_suggestions", json!({"input": query}))
            .await?;
        Ok(parse_suggestions(&json))
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

    /// Tracks YouTube Music's radio plays after `video_id`, not including it: the first page
    /// (about 50) and the next ones up to `RADIO_PAGES`, each video once.
    pub async fn yt_related(&self, video_id: &str) -> Result<Vec<Track>> {
        self.yt_radio(video_id, RADIO_PAGES).await
    }

    /// `yt_related` reading `pages` pages of the radio.
    pub async fn yt_radio(&self, video_id: &str, pages: usize) -> Result<Vec<Track>> {
        if !valid_video_id(video_id) {
            bail!("YouTube: this isn't a valid video");
        }
        let mut json = self
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
        let mut seen = std::collections::HashSet::from([video_id.to_string()]);
        let mut out = Vec::new();
        for page in 0..pages.max(1) {
            out.extend(
                parse_queue(&json)
                    .into_iter()
                    .filter(|t| t.video_id.as_ref().is_some_and(|v| seen.insert(v.clone()))),
            );
            let Some(token) = radio_continuation(&json) else {
                break;
            };
            if page + 1 == pages {
                break;
            }
            // A later page that fails just ends the radio where it got to.
            match self
                .music_post("next", json!({ "continuation": token }))
                .await
            {
                Ok(next) => json = next,
                Err(e) => {
                    tracing::debug!("YouTube Music radio page {}: {e}", page + 2);
                    break;
                }
            }
        }
        Ok(out)
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
        assert_eq!(tracks.len(), 4, "a row without a length is still a song");
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
        assert_eq!(tracks[2].id.key(), "yt:noLengthxxx");
        assert_eq!(tracks[2].duration, 0);
        assert_eq!(tracks[3].duration, 120_000);
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

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn merged(slug: &str) -> Vec<Track> {
        merge_search(
            &fixture(&format!("ytm_search_{slug}_all")),
            &fixture(&format!("ytm_search_{slug}_songs")),
        )
    }

    #[test]
    fn the_top_result_card_is_the_first_hit() {
        let tracks = parse_search(&fixture("ytm_search_the_weeknd_blinding_lights_all"));
        assert_eq!(tracks[0].video_id.as_deref(), Some("4NRXx6U8ABQ"));
        assert_eq!(tracks[0].title, "Blinding Lights (Official Video)");
        assert_eq!(tracks[0].artist, "The Weeknd");
        assert_eq!(tracks[0].duration, 263_000);
        assert!(!tracks[0].artwork.is_empty());
    }

    #[test]
    fn a_song_card_has_its_artist_and_length() {
        let (kind, card, _) = parse_card(&fixture("ytm_search_xtal_all")).unwrap();
        assert_eq!(kind, Kind::Song);
        assert_eq!(card.video_id.as_deref(), Some("sWcLccMuCA8"));
        assert_eq!(card.artist, "Aphex Twin");
        assert_eq!(card.duration, 294_000);
    }

    #[test]
    fn unfiltered_song_rows_without_a_length_are_kept() {
        let tracks = parse_search(&fixture("ytm_search_blinding_lights_all"));
        let song = tracks
            .iter()
            .find(|t| t.video_id.as_deref() == Some("H64a2ggVIWc"))
            .expect("the studio song row");
        assert_eq!((song.artist.as_str(), song.duration), ("The Weeknd", 0));
        assert!(
            tracks.iter().all(|t| !t.title.contains("MIX IT UP")),
            "episodes are not music"
        );
    }

    #[test]
    fn searching_artist_and_title_puts_the_top_result_first() {
        // The "Songs" search alone returns covers here and not the song at all.
        let songs = fixture("ytm_search_the_weeknd_blinding_lights_songs");
        assert!(parse_search(&songs)
            .iter()
            .all(|t| t.video_id.as_deref() != Some("4NRXx6U8ABQ")));
        let tracks = merged("the_weeknd_blinding_lights");
        assert_eq!(tracks[0].video_id.as_deref(), Some("4NRXx6U8ABQ"));
        assert!(tracks.len() > 5);
    }

    #[test]
    fn the_studio_song_goes_before_its_music_video() {
        for (slug, song) in [
            ("blinding_lights", "H64a2ggVIWc"),
            ("not_like_us", "phLb_SoPBlA"),
            ("bohemian_rhapsody", "BSTsnWoslP4"),
            ("xtal", "sWcLccMuCA8"),
        ] {
            let tracks = merged(slug);
            assert_eq!(tracks[0].video_id.as_deref(), Some(song), "{slug}");
            let mut ids: Vec<_> = tracks.iter().map(|t| t.video_id.clone()).collect();
            let all = ids.len();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), all, "{slug}: each video once");
        }
        // The Songs search missed the studio song in this capture: only the unfiltered search
        // had it, without a length. The music video comes right after it.
        let tracks = merged("blinding_lights");
        assert_eq!(tracks[0].duration, 0);
        assert_eq!(tracks[1].video_id.as_deref(), Some("4NRXx6U8ABQ"));
    }

    #[test]
    fn one_failed_search_still_gives_the_other_ones_hits() {
        let songs = fixture("ytm_search_xtal_songs");
        assert_eq!(
            merge_search(&Value::Null, &songs)[0].video_id.as_deref(),
            Some("sWcLccMuCA8")
        );
        let all = fixture("ytm_search_xtal_all");
        assert_eq!(
            merge_search(&all, &Value::Null)[0].video_id.as_deref(),
            Some("sWcLccMuCA8")
        );
    }

    #[test]
    fn counts_read_with_suffixes_and_separators() {
        for (text, want) in [
            ("3.7B plays", Some(3_700_000_000)),
            ("21K views", Some(21_000)),
            ("2.8M plays", Some(2_800_000)),
            ("993 plays", Some(993)),
            ("1,234 views", Some(1234)),
            ("1,234,567 views", Some(1_234_567)),
            ("1 234 views", Some(1234)),
            ("1,2 mln", Some(1_200_000)),
            ("1.2M", Some(1_200_000)),
            ("1 play", Some(1)),
            ("248M monthly audience", None),
            ("3:52", None),
            ("Song", None),
            ("", None),
        ] {
            assert_eq!(parse_count(text), want, "{text}");
        }
    }

    #[test]
    fn rows_and_cards_carry_their_counts() {
        let counted = merge_search_counted(
            &fixture("ytm_search_blinding_lights_all"),
            &fixture("ytm_search_blinding_lights_songs"),
        );
        let plays = |id: &str| {
            counted
                .iter()
                .find(|(t, _)| t.video_id.as_deref() == Some(id))
                .map(|(_, p)| *p)
        };
        assert_eq!(plays("H64a2ggVIWc"), Some(3_700_000_000));
        assert_eq!(plays("4NRXx6U8ABQ"), Some(1_000_000_000));
    }

    #[test]
    fn the_original_ranks_first_once_play_counts_count() {
        for (slug, query, artist) in [
            ("blinding_lights", "blinding lights", "The Weeknd"),
            ("bohemian_rhapsody", "bohemian rhapsody", "Queen"),
            ("not_like_us", "not like us", "Kendrick Lamar"),
            ("xtal", "xtal", "Aphex Twin"),
        ] {
            let (tracks, plays): (Vec<_>, Vec<_>) = merge_search_counted(
                &fixture(&format!("ytm_search_{slug}_all")),
                &fixture(&format!("ytm_search_{slug}_songs")),
            )
            .into_iter()
            .unzip();
            // Even with a cover in front of it, as the services' own order sometimes has it.
            let mut order: Vec<usize> = (0..tracks.len()).collect();
            let best = (0..tracks.len()).max_by_key(|&i| plays[i]).unwrap();
            order.retain(|&i| i != best);
            order.insert(1.min(order.len()), best);
            let skewed = crate::ranking::refine(
                query,
                order.iter().map(|&i| tracks[i].clone()).collect(),
                &order.iter().map(|&i| plays[i]).collect::<Vec<_>>(),
                &[],
                3.0,
                30,
            );
            assert_eq!(
                skewed[0].artist, artist,
                "{slug} skewed: {:?}",
                skewed[0].title
            );
            let refined = crate::ranking::refine(query, tracks, &plays, &[], 3.0, 30);
            assert_eq!(refined[0].artist, artist, "{slug}: {:?}", refined[0].title);
        }
    }

    #[test]
    fn suggestions_are_queries_then_what_they_jump_to() {
        let found = parse_suggestions(&fixture("ytm_suggestions_aphex"));
        assert_eq!(found[0].text, "aphex twin");
        assert_eq!(found[0].kind, SuggestionKind::Query);
        assert!(found[0].video_id.is_none() && found[0].browse_id.is_none());
        let artist = found
            .iter()
            .find(|s| s.kind == SuggestionKind::Artist)
            .unwrap();
        assert_eq!(artist.text, "Aphex Twin");
        assert_eq!(
            artist.browse_id.as_deref(),
            Some("UCWmnkYUzoOiOztmPBhIlZjg")
        );
        assert!(artist.thumb.starts_with("https://"));
        let song = found
            .iter()
            .find(|s| s.kind == SuggestionKind::Song)
            .unwrap();
        assert!(song.video_id.is_some() && !song.subtitle.contains("plays"));
        let video = found
            .iter()
            .find(|s| s.kind == SuggestionKind::Video)
            .unwrap();
        assert!(video.video_id.is_some() && video.subtitle.is_empty());
        assert!(found.iter().all(|s| !s.text.is_empty()));

        let kendrick = parse_suggestions(&fixture("ytm_suggestions_kendrick"));
        let video = kendrick
            .iter()
            .find(|s| s.video_id.as_deref() == Some("H58vbez_m4E"))
            .unwrap();
        assert_eq!(video.text, "Not Like Us");
        assert_eq!(video.subtitle, "Kendrick Lamar");
    }
}
