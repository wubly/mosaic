use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use futures::future::join_all;
use mosaic_core::lyrics_timing::LyricLine;
use mosaic_core::model::TrackId;
use mosaic_core::storage::{read_json, write_json_atomic, LYRICS_CHOICES_FILE};
use serde_json::Value;
use std::collections::HashMap;

use super::parse::{empty_lyrics, has_word_stamps, kind_of, parse_lrc, parse_title, shape_lyrics};
use super::{Lyrics, LyricsKind, LyricsMeta, Record};
use crate::Services;

#[derive(Clone, Debug)]
pub struct LyricsArgs {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    pub lrc_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: i64,
    pub diff: i64,
    pub kind: LyricsKind,
}

#[derive(Clone, Debug)]
pub struct Candidates {
    pub list: Vec<Candidate>,
    pub chosen: Option<String>,
    pub query: String,
}

type Choices = HashMap<String, Record>;

fn enc(params: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish()
}

fn sidecar(lrc_path: Option<&str>) -> Option<Vec<LyricLine>> {
    let lines = parse_lrc(&std::fs::read_to_string(lrc_path?).ok()?);
    (!lines.is_empty()).then_some(lines)
}

fn kind_of_lines(lines: &[LyricLine]) -> LyricsKind {
    if lines.iter().any(|l| l.words.is_some()) {
        LyricsKind::Word
    } else {
        LyricsKind::Synced
    }
}

struct Plan {
    dur: i64,
    tracks: Vec<String>,
    artists: Vec<String>,
    exact: Vec<String>,
    fuzzy: Vec<String>,
}

fn plan(title: &str, artist: &str, duration_ms: u64) -> Plan {
    let dur = (duration_ms as f64 / 1000.0).round() as i64;
    let parsed = parse_title(title, artist);
    let (tracks, artists) = (parsed.tracks, parsed.artists);
    let first = tracks.first().cloned().unwrap_or_default();
    let owner = artists.first().cloned().unwrap_or_default();

    let dur_s = dur.to_string();
    let exact = tracks
        .iter()
        .flat_map(|track| artists.iter().map(move |who| (who, track)))
        .take(8)
        .map(|(who, track)| {
            format!(
                "get?{}",
                enc(&[
                    ("artist_name", who),
                    ("track_name", track),
                    ("duration", &dur_s)
                ])
            )
        })
        .collect();

    let mut fuzzy: Vec<String> = artists
        .iter()
        .take(2)
        .map(|who| {
            format!(
                "search?{}",
                enc(&[("track_name", &first), ("artist_name", who)])
            )
        })
        .collect();
    fuzzy.push(format!("search?{}", enc(&[("track_name", &first)])));
    fuzzy.push(format!(
        "search?{}",
        enc(&[("q", format!("{first} {owner}").trim())])
    ));
    if let Some(second) = tracks.get(1) {
        fuzzy.push(format!(
            "search?{}",
            enc(&[("q", format!("{second} {owner}").trim())])
        ));
    }
    Plan {
        dur,
        tracks,
        artists,
        exact,
        fuzzy,
    }
}

fn diff(record: &Record, dur: i64) -> f64 {
    (record.duration.unwrap_or(0.0) - dur as f64).abs()
}

fn records(value: Option<Value>) -> Vec<Record> {
    match value {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Some(v @ Value::Object(_)) => serde_json::from_value(v).ok().into_iter().collect(),
        _ => Vec::new(),
    }
}

/// A title or artist reduced for matching: lowercase, accents and punctuation gone, bracketed
/// parts and "feat." credits dropped. Unlike `norm` it keeps every script, so Cyrillic or
/// Japanese titles still compare.
fn match_key(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let lower = s.to_lowercase();
    let mut bare = String::with_capacity(lower.len());
    let mut depth = 0usize;
    for c in lower.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            c if depth == 0 => bare.push(c),
            _ => {}
        }
    }
    for credit in [
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " with ",
    ] {
        if let Some(i) = bare.find(credit) {
            bare.truncate(i);
        }
    }
    let mut out = String::new();
    let mut gap = false;
    for c in bare.nfkd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)) {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

/// Whether two match keys name the same song: equal, or one is the other plus trailing words
/// ("song" and "song remastered"), never a word somewhere inside a longer title.
fn same_title(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short == long || (short.chars().count() >= 3 && long.starts_with(&format!("{short} ")))
}

fn same_artist(want: &[String], theirs: &str) -> bool {
    !theirs.is_empty()
        && want
            .iter()
            .any(|w| !w.is_empty() && (theirs.contains(w.as_str()) || w.contains(theirs)))
}

/// The best synced record that is this song by this artist. Duration alone never qualifies
/// a record: a different song of the same length is still a different song.
fn pick(records: Vec<Record>, plan: &Plan) -> Option<Record> {
    let want_track: Vec<String> = plan.tracks.iter().map(|t| match_key(t)).collect();
    let want_artist: Vec<String> = plan.artists.iter().map(|a| match_key(a)).collect();

    let mut acceptable: Vec<Record> = records
        .into_iter()
        .filter(Record::has_synced)
        .filter(|r| {
            let theirs = match_key(r.track_name.as_deref().unwrap_or(""));
            let title = want_track.iter().any(|w| same_title(w, &theirs));
            let artist = same_artist(
                &want_artist,
                &match_key(r.artist_name.as_deref().unwrap_or("")),
            );
            let d = diff(r, plan.dur);
            title && artist && (plan.dur <= 0 || d <= 20.0)
        })
        .collect();
    acceptable.sort_by(|a, b| diff(a, plan.dur).total_cmp(&diff(b, plan.dur)));
    acceptable.into_iter().next()
}

/// Lower is better; synced and word-stamped versions get a bonus, distance is capped at 60 s.
pub fn score_candidate(record: &Record, dur: i64) -> f64 {
    let synced = record.synced_lyrics.as_deref().filter(|s| !s.is_empty());
    diff(record, dur).min(60.0)
        - if synced.is_some() { 12.0 } else { 0.0 }
        - if synced.is_some_and(has_word_stamps) {
            6.0
        } else {
            0.0
        }
}

impl Services {
    fn choices_path(&self) -> std::path::PathBuf {
        self.inner.dirs.file(LYRICS_CHOICES_FILE)
    }

    fn read_choices(&self) -> Choices {
        read_json(&self.choices_path())
    }

    fn cache_get(&self, id: &str) -> Option<Lyrics> {
        self.inner.lyrics.auto_cache.lock().ok()?.get(id).cloned()
    }

    fn cache_put(&self, id: &str, lyrics: &Lyrics) {
        if let Ok(mut cache) = self.inner.lyrics.auto_cache.lock() {
            cache.insert(id.to_string(), lyrics.clone());
        }
    }

    async fn embedded(&self, id: &TrackId) -> Option<(Vec<LyricLine>, String, LyricsKind)> {
        let TrackId::Str(s) = id else { return None };
        if !s.starts_with("local:") {
            return None;
        }
        let file = self.local_path(id)?;
        tokio::task::spawn_blocking(move || read_embedded(&file))
            .await
            .ok()
            .flatten()
    }

    async fn auto_lyrics(&self, args: &LyricsArgs) -> Option<Record> {
        let plan = plan(&args.title, &args.artist, args.duration_ms);
        let exact = join_all(plan.exact.iter().map(|q| self.lrclib(q, 2))).await;
        if let Some(found) = pick(exact.into_iter().flat_map(records).collect(), &plan) {
            return Some(found);
        }
        let fuzzy = join_all(plan.fuzzy.iter().map(|q| self.lrclib(q, 2))).await;
        pick(fuzzy.into_iter().flat_map(records).collect(), &plan)
    }

    pub async fn get_lyrics(&self, args: &LyricsArgs) -> Lyrics {
        let key = args.id.key();

        if let Some(chosen) = self.read_choices().get(&key) {
            return shape_lyrics(chosen, "custom");
        }

        if let Some(own) = sidecar(args.lrc_path.as_deref()) {
            let name = args
                .lrc_path
                .as_deref()
                .and_then(|p| Path::new(p).file_name())
                .map(|n| n.to_string_lossy().into_owned());
            return Lyrics {
                meta: LyricsMeta {
                    origin: "file".into(),
                    id: Some("file".into()),
                    title: name.unwrap_or_default(),
                    artist: String::new(),
                    album: String::new(),
                    kind: kind_of_lines(&own),
                },
                synced: own,
                plain: String::new(),
            };
        }

        if let Some(hit) = self.cache_get(&key) {
            return hit;
        }

        if let Some((synced, plain, kind)) = self
            .embedded(&args.id)
            .await
            .filter(|(synced, _, _)| !synced.is_empty())
        {
            let result = Lyrics {
                synced,
                plain,
                meta: LyricsMeta {
                    origin: "file".into(),
                    id: Some("embedded".into()),
                    title: "Embedded in file".into(),
                    artist: String::new(),
                    album: String::new(),
                    kind,
                },
            };
            self.cache_put(&key, &result);
            return result;
        }

        let before = self.lrclib_failures();
        let mut found = self.auto_lyrics(args).await;
        if found.is_none() && self.lrclib_failures() > before {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            found = self.auto_lyrics(args).await;
        }

        match found {
            Some(record) => {
                let result = shape_lyrics(&record, "lrclib");
                self.cache_put(&key, &result);
                result
            }
            None => empty_lyrics(),
        }
    }

    pub async fn get_candidates(&self, args: &LyricsArgs, typed_query: &str) -> Candidates {
        let plan = plan(&args.title, &args.artist, args.duration_ms);
        let typed = typed_query.trim();

        let requests: Vec<String> = if typed.is_empty() {
            plan.exact.iter().chain(&plan.fuzzy).cloned().collect()
        } else {
            vec![
                format!("search?{}", enc(&[("q", typed)])),
                format!("search?{}", enc(&[("track_name", typed)])),
            ]
        };

        let ask = || async {
            join_all(requests.iter().map(|q| self.lrclib(q, 2)))
                .await
                .into_iter()
                .flat_map(records)
                .collect::<Vec<_>>()
        };
        let before = self.lrclib_failures();
        let mut results = ask().await;
        if results.is_empty() && self.lrclib_failures() > before {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            results = ask().await;
        }

        let mut unique: Vec<Record> = Vec::new();
        for r in results {
            let usable = r.has_synced();
            if let Some(id) = r.id.filter(|_| usable) {
                if !unique.iter().any(|u| u.id == Some(id)) {
                    unique.push(r);
                }
            }
        }
        unique
            .sort_by(|a, b| score_candidate(a, plan.dur).total_cmp(&score_candidate(b, plan.dur)));
        unique.truncate(30);

        if let Ok(mut store) = self.inner.lyrics.candidates.lock() {
            for r in &unique {
                if let Some(id) = r.id {
                    store.insert(id, r.clone());
                }
            }
        }

        let mut list: Vec<Candidate> = unique
            .iter()
            .map(|r| {
                let duration = r.duration.unwrap_or(0.0);
                Candidate {
                    id: r.id.map(|i| i.to_string()).unwrap_or_default(),
                    title: r.track_name.clone().unwrap_or_default(),
                    artist: r.artist_name.clone().unwrap_or_default(),
                    album: r.album_name.clone().unwrap_or_default(),
                    duration: duration.round() as i64,
                    diff: (duration - plan.dur as f64).round() as i64,
                    kind: kind_of(r),
                }
            })
            .collect();

        if typed.is_empty() {
            if let Some(own) = sidecar(args.lrc_path.as_deref()) {
                let name = args
                    .lrc_path
                    .as_deref()
                    .and_then(|p| Path::new(p).file_name())
                    .map(|n| n.to_string_lossy().into_owned());
                list.insert(
                    0,
                    Candidate {
                        id: "file".into(),
                        title: name.unwrap_or_default(),
                        artist: "Next to your file".into(),
                        album: String::new(),
                        duration: 0,
                        diff: 0,
                        kind: kind_of_lines(&own),
                    },
                );
            }
        }

        let query = format!(
            "{} {}",
            plan.artists.first().map_or("", String::as_str),
            plan.tracks.first().map_or("", String::as_str)
        );
        Candidates {
            list,
            chosen: self
                .read_choices()
                .get(&args.id.key())
                .and_then(|r| r.id)
                .map(|i| i.to_string()),
            query: query.trim().to_string(),
        }
    }

    /// `cand_id` of `None` or `"file"` resets to automatic selection.
    pub async fn select_lyrics(&self, args: &LyricsArgs, cand_id: Option<&str>) -> Result<Lyrics> {
        let mut choices = self.read_choices();
        let key = args.id.key();

        let record = cand_id
            .filter(|c| *c != "file")
            .and_then(|c| c.parse::<i64>().ok())
            .and_then(|id| self.inner.lyrics.candidates.lock().ok()?.get(&id).cloned());

        if let Some(record) = record {
            let shaped = shape_lyrics(&record, "custom");
            choices.insert(key, record);
            write_json_atomic(&self.choices_path(), &choices)?;
            return Ok(shaped);
        }

        choices.remove(&key);
        write_json_atomic(&self.choices_path(), &choices)?;
        Ok(self.get_lyrics(args).await)
    }
}

fn read_sylt(file: &Path) -> Option<Vec<LyricLine>> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, SynchronizedTextFrame, TimestampFormat};
    use lofty::mpeg::MpegFile;

    let mut handle = std::fs::File::open(file).ok()?;
    let mpeg = MpegFile::read_from(&mut handle, ParseOptions::new()).ok()?;
    for frame in mpeg.id3v2()? {
        let Frame::Binary(bin) = frame else { continue };
        if bin.id().as_str() != "SYLT" {
            continue;
        }
        let Ok(sylt) = SynchronizedTextFrame::parse(&bin.data, Default::default()) else {
            continue;
        };
        if sylt.timestamp_format != TimestampFormat::MS {
            continue;
        }
        let synced: Vec<LyricLine> = sylt
            .content
            .iter()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(ms, text)| LyricLine {
                t: f64::from(*ms) / 1000.0,
                text: text.trim().to_string(),
                words: None,
            })
            .collect();
        if !synced.is_empty() {
            return Some(synced);
        }
    }
    None
}

fn read_embedded(file: &Path) -> Option<(Vec<LyricLine>, String, LyricsKind)> {
    use lofty::file::TaggedFileExt;
    use lofty::tag::ItemKey;

    if let Some(synced) = read_sylt(file) {
        return Some((synced, String::new(), LyricsKind::Synced));
    }

    let tagged = lofty::read_from_path(file).ok()?;
    let text = tagged
        .tags()
        .iter()
        .flat_map(|t| {
            [ItemKey::Lyrics, ItemKey::UnsyncLyrics]
                .into_iter()
                .filter_map(|k| t.get_string(k))
        })
        .find(|t| !t.trim().is_empty())?;

    let lines = parse_lrc(text);
    if !lines.is_empty() {
        let kind = kind_of_lines(&lines);
        return Some((lines, String::new(), kind));
    }
    Some((Vec::new(), text.trim().to_string(), LyricsKind::Plain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        id: i64,
        track: &str,
        artist: &str,
        duration: f64,
        synced: Option<&str>,
        plain: Option<&str>,
    ) -> Record {
        Record {
            id: Some(id),
            track_name: Some(track.into()),
            artist_name: Some(artist.into()),
            duration: Some(duration),
            synced_lyrics: synced.map(Into::into),
            plain_lyrics: plain.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn score_prefers_synced_and_close() {
        let plain = rec(1, "t", "a", 200.0, None, Some("x"));
        let synced = rec(2, "t", "a", 200.0, Some("[00:01.00] a"), None);
        let word = rec(
            3,
            "t",
            "a",
            200.0,
            Some("[00:01.00] <00:01.00> a <00:02.00> b"),
            None,
        );
        assert_eq!(score_candidate(&plain, 200), 0.0);
        assert_eq!(score_candidate(&synced, 200), -12.0);
        assert_eq!(score_candidate(&word, 200), -18.0);
        assert_eq!(score_candidate(&plain, 500), 60.0);
        assert_eq!(score_candidate(&plain, 205), 5.0);
    }

    #[test]
    fn plan_builds_queries() {
        let p = plan("Artist - Song (Official Video)", "Uploader", 200_400);
        assert_eq!(p.dur, 200);
        assert_eq!(p.tracks[0], "Song");
        assert_eq!(
            p.exact[0],
            "get?artist_name=Artist&track_name=Song&duration=200"
        );
        assert!(p.exact.len() <= 8);
        assert_eq!(p.fuzzy[0], "search?track_name=Song&artist_name=Artist");
        assert!(p.fuzzy.iter().any(|q| q == "search?track_name=Song"));
        assert!(p.fuzzy.iter().any(|q| q == "search?q=Song+Artist"));
    }

    #[test]
    fn pick_needs_the_same_song_and_artist_with_synced_lyrics() {
        let p = plan("Artist - Song", "Uploader", 200_000);
        let synced = Some("[00:01.00] a");
        // Right length, wrong song: never good enough on its own.
        let close_other = rec(1, "Other", "Nobody", 201.0, synced, None);
        let other_title = rec(2, "Different", "Artist", 200.0, synced, None);
        let other_artist = rec(3, "Song", "Somebody Else", 200.0, synced, None);
        let plain_only = rec(4, "Song", "Artist", 200.0, None, Some("plain"));
        let too_long = rec(5, "Song", "Artist", 230.0, synced, None);
        let near = rec(6, "Song", "Artist", 210.0, synced, None);
        let exact = rec(7, "Song", "Artist", 200.0, synced, None);
        let featuring = rec(
            8,
            "Song (feat. Someone)",
            "Artist, Someone",
            201.0,
            synced,
            None,
        );

        for wrong in [close_other, other_title, other_artist, plain_only, too_long] {
            let id = wrong.id;
            assert_eq!(pick(vec![wrong], &p).and_then(|r| r.id), None, "{id:?}");
        }
        assert_eq!(pick(vec![near.clone()], &p).and_then(|r| r.id), Some(6));
        assert_eq!(pick(vec![near, exact], &p).and_then(|r| r.id), Some(7));
        assert_eq!(pick(vec![featuring], &p).and_then(|r| r.id), Some(8));
    }

    #[test]
    fn titles_match_whole_not_by_a_short_word_inside() {
        assert!(same_title("song", "song"));
        assert!(same_title("song", "song remastered"));
        assert!(!same_title("love", "i will always love you"));
        assert!(!same_title("", "x"));
        assert_eq!(match_key("Song (feat. Someone) [Remix]"), "song");
        // Marks are stripped on both sides alike (й → и), so this still matches itself.
        assert_eq!(match_key("Главный герой - Я"), "главныи герои я");
        assert_eq!(match_key("Nie mów do mnie"), "nie mow do mnie");
    }

    #[tokio::test]
    async fn sidecar_and_choice_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let lrc = dir.path().join("a.lrc");
        std::fs::write(&lrc, "[00:01.00]hello").unwrap();
        let services = Services::new(
            mosaic_core::storage::Dirs::with_root(dir.path()),
            Default::default(),
        )
        .unwrap();
        let args = LyricsArgs {
            id: TrackId::Str("local:abc".into()),
            title: "t".into(),
            artist: "a".into(),
            duration_ms: 1000,
            lrc_path: Some(lrc.to_string_lossy().into_owned()),
        };
        let lyrics = services.get_lyrics(&args).await;
        assert_eq!(
            (lyrics.meta.origin.as_str(), lyrics.synced.len()),
            ("file", 1)
        );
        assert_eq!(lyrics.meta.title, "a.lrc");

        let chosen = Record {
            id: Some(9),
            synced_lyrics: Some("[00:02.00] x".into()),
            ..Default::default()
        };
        let mut choices = Choices::new();
        choices.insert("local:abc".into(), chosen);
        write_json_atomic(&dir.path().join(LYRICS_CHOICES_FILE), &choices).unwrap();
        let lyrics = services.get_lyrics(&args).await;
        assert_eq!(
            (lyrics.meta.origin.as_str(), lyrics.synced[0].t),
            ("custom", 2.0)
        );
    }
}
