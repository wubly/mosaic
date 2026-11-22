use std::sync::LazyLock;

use mosaic_core::lyrics_timing::{LyricLine, LyricWord};
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use super::{Lyrics, LyricsKind, LyricsMeta, Record};

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static regex")
}

static BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| re(r"\[[^\]]*\]|\([^)]*\)|\{[^}]*\}|\|[^|]*\|"));
static FEAT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\s+(ft|feat|featuring|with)\.?\s.*$"));
static JUNK_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\s+(official|lyric|lyrics|audio|video|visualizer|music video)(?-u:\b).*$")
});
static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
static DASH: LazyLock<Regex> = LazyLock::new(|| re(r"\s+[-–—]\s+"));
static ARTIST_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\s*(?:,|&|(?-u:\b)x(?-u:\b)|(?-u:\b)and(?-u:\b))\s*"));
static OWNER_SPLIT: LazyLock<Regex> = LazyLock::new(|| re(r"\s*[;/,&]\s*"));
static LINE_STAMP: LazyLock<Regex> = LazyLock::new(|| re(r"\[(\d+):(\d+(?:\.\d+)?)\]"));
static WORD_STAMP: LazyLock<Regex> = LazyLock::new(|| re(r"<(\d+):(\d+(?:\.\d+)?)>\s*([^<]*)"));
static ANY_WORD_STAMP: LazyLock<Regex> = LazyLock::new(|| re(r"<\d+:\d+(?:\.\d+)?>"));

pub fn uniq<I, S>(list: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for x in list {
        let x = x.as_ref().trim();
        if !x.is_empty() && !out.iter().any(|o| o == x) {
            out.push(x.to_string());
        }
    }
    out
}

pub fn norm(x: &str) -> String {
    let stripped: String = x
        .to_lowercase()
        .nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect();
    let mut out = String::new();
    let mut gap = false;
    for c in stripped.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
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

fn clean(x: &str) -> String {
    let x = BRACKETS.replace_all(x, " ");
    let x = FEAT.replace(&x, "");
    let x = JUNK_TAIL.replace(&x, "");
    SPACES.replace_all(&x, " ").trim().to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedTitle {
    pub tracks: Vec<String>,
    pub artists: Vec<String>,
}

pub fn parse_title(title: &str, uploader: &str) -> ParsedTitle {
    let split: Vec<&str> = DASH.split(title).collect();
    let mut tracks: Vec<String> = Vec::new();
    let mut artists: Vec<String> = Vec::new();

    if split.len() > 1 {
        let prefix = clean(split[0]);
        artists.push(prefix.clone());
        artists.extend(ARTIST_SPLIT.split(&prefix).map(str::to_string));
        tracks.push(clean(&split[1..].join(" - ")));
        tracks.push(clean(split[1]));
    }
    tracks.push(clean(title));
    let owner = clean(uploader);
    artists.push(owner.clone());
    artists.extend(OWNER_SPLIT.split(&owner).map(str::to_string));

    let mut tracks = uniq(tracks);
    let mut artists = uniq(artists);
    tracks.truncate(3);
    artists.truncate(3);
    ParsedTitle { tracks, artists }
}

fn stamp(minutes: &str, seconds: &str) -> f64 {
    minutes.parse::<f64>().unwrap_or(0.0) * 60.0 + seconds.parse::<f64>().unwrap_or(0.0)
}

pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();

    for raw in lrc.split('\n') {
        let stamps: Vec<_> = LINE_STAMP.captures_iter(raw).collect();
        if stamps.is_empty() {
            continue;
        }

        let body = LINE_STAMP.replace_all(raw, "").trim().to_string();
        let text = SPACES
            .replace_all(&ANY_WORD_STAMP.replace_all(&body, ""), " ")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }

        let word_stamps: Vec<_> = WORD_STAMP.captures_iter(&body).collect();
        let words = (word_stamps.len() > 1).then(|| {
            word_stamps
                .iter()
                .map(|m| LyricWord {
                    t: stamp(&m[1], &m[2]),
                    w: m[3].trim().to_string(),
                })
                .filter(|w| !w.w.is_empty())
                .collect::<Vec<_>>()
        });
        for m in &stamps {
            lines.push(LyricLine {
                t: stamp(&m[1], &m[2]),
                text: text.clone(),
                words: words.clone(),
            });
        }
    }
    lines.sort_by(|a, b| a.t.total_cmp(&b.t));
    lines
}

pub fn has_word_stamps(text: &str) -> bool {
    ANY_WORD_STAMP.is_match(text)
}

pub fn kind_of(record: &Record) -> LyricsKind {
    if record.instrumental == Some(true) {
        return LyricsKind::Instrumental;
    }
    if let Some(synced) = record.synced_lyrics.as_deref().filter(|s| !s.is_empty()) {
        return if has_word_stamps(synced) {
            LyricsKind::Word
        } else {
            LyricsKind::Synced
        };
    }
    if record
        .plain_lyrics
        .as_deref()
        .is_some_and(|s| !s.is_empty())
    {
        LyricsKind::Plain
    } else {
        LyricsKind::None
    }
}

pub fn empty_lyrics() -> Lyrics {
    Lyrics {
        synced: Vec::new(),
        plain: String::new(),
        meta: LyricsMeta {
            origin: "none".into(),
            id: None,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            kind: LyricsKind::None,
        },
    }
}

pub fn shape_lyrics(record: &Record, origin: &str) -> Lyrics {
    Lyrics {
        synced: record
            .synced_lyrics
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(parse_lrc)
            .unwrap_or_default(),
        plain: record.plain_lyrics.clone().unwrap_or_default(),
        meta: LyricsMeta {
            origin: origin.into(),
            id: record.id.map(|i| i.to_string()),
            title: record.track_name.clone().unwrap_or_default(),
            artist: record.artist_name.clone().unwrap_or_default(),
            album: record.album_name.clone().unwrap_or_default(),
            kind: kind_of(record),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_with_artist_prefix() {
        let p = parse_title(
            "Daft Punk & Pharrell - Get Lucky (Official Video) [HD]",
            "Daft Punk Vevo",
        );
        assert_eq!(p.tracks, ["Get Lucky", "Daft Punk & Pharrell - Get Lucky"]);
        assert_eq!(p.artists, ["Daft Punk & Pharrell", "Daft Punk", "Pharrell"]);
    }

    #[test]
    fn title_without_prefix_uses_uploader() {
        let p = parse_title("Blinding Lights ft. Someone", "The Weeknd; Other");
        assert_eq!(p.tracks, ["Blinding Lights"]);
        assert_eq!(p.artists, ["The Weeknd; Other", "The Weeknd", "Other"]);
    }

    #[test]
    fn multiple_dashes_and_junk_tail() {
        let p = parse_title("A - B - C lyrics video", "x");
        assert_eq!(p.tracks, ["B - C", "B", "A - B - C"]);
        assert_eq!(parse_title("", "").tracks, Vec::<String>::new());
    }

    #[test]
    fn x_and_and_split_artists() {
        let p = parse_title("Foo x Bar and Baz - Song", "");
        assert_eq!(p.artists, ["Foo x Bar and Baz", "Foo", "Bar"]);
    }

    #[test]
    fn norm_strips_accents_and_punctuation() {
        assert_eq!(norm("  Beyoncé — Halo!! "), "beyonce halo");
        assert_eq!(norm("ＡＢＣ"), "abc");
    }

    #[test]
    fn lrc_plain_and_multi_stamps() {
        let lines = parse_lrc(
            "[ti:x]\n[00:12.50]Hello  world\r\n[01:00.00][00:30.25]Again\n[00:40.00]\nnoise",
        );
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].t, lines[0].text.as_str()), (12.5, "Hello world"));
        assert_eq!((lines[1].t, lines[1].text.as_str()), (30.25, "Again"));
        assert_eq!(lines[2].t, 60.0);
        assert!(lines[0].words.is_none());
    }

    #[test]
    fn lrc_word_stamps() {
        let lines = parse_lrc("[00:10.00]<00:10.00> Hello <00:10.50> big <00:11.00>world");
        assert_eq!(lines[0].text, "Hello big world");
        let words = lines[0].words.as_ref().unwrap();
        assert_eq!(
            words.iter().map(|w| w.w.as_str()).collect::<Vec<_>>(),
            ["Hello", "big", "world"]
        );
        assert_eq!(words[1].t, 10.5);
        let single = parse_lrc("[00:10.00]<00:10.00> Only");
        assert!(single[0].words.is_none());
        assert_eq!(single[0].text, "Only");
        assert!(has_word_stamps("a <00:01.00> b"));
        assert!(!has_word_stamps("[00:01.00] b"));
    }

    #[test]
    fn kinds() {
        let mut r = Record::default();
        assert_eq!(kind_of(&r), LyricsKind::None);
        r.plain_lyrics = Some("la".into());
        assert_eq!(kind_of(&r), LyricsKind::Plain);
        r.synced_lyrics = Some("[00:01.00] a".into());
        assert_eq!(kind_of(&r), LyricsKind::Synced);
        r.synced_lyrics = Some("[00:01.00] <00:01.00> a <00:02.00> b".into());
        assert_eq!(kind_of(&r), LyricsKind::Word);
        r.instrumental = Some(true);
        assert_eq!(kind_of(&r), LyricsKind::Instrumental);
    }

    #[test]
    fn shape() {
        let r = Record {
            id: Some(7),
            track_name: Some("T".into()),
            artist_name: Some("A".into()),
            synced_lyrics: Some("[00:01.00] a".into()),
            ..Default::default()
        };
        let l = shape_lyrics(&r, "custom");
        assert_eq!(l.synced.len(), 1);
        assert_eq!(
            (l.meta.origin.as_str(), l.meta.id.as_deref(), l.meta.kind),
            ("custom", Some("7"), LyricsKind::Synced)
        );
        assert_eq!(empty_lyrics().meta.origin, "none");
    }
}
