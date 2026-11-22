//! Port of `electron/spotify/match.js`: find the SoundCloud track that best matches a Spotify one.

use std::collections::HashSet;
use std::sync::LazyLock;

use mosaic_core::model::Track;
use regex::Regex;

use super::embed::RemoteTrack;
use crate::lyrics::parse::norm;
use crate::soundcloud::tracks::playable;
use crate::Services;

const MIN_SCORE: f64 = 0.6;
const GOOD_ENOUGH: f64 = 0.8;

static VARIANTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(remix|rmx|slowed|sped|speed|fast|nightcore|cover|instrumental|reverb|8d|bass boosted|karaoke|mashup|edit|flip|bootleg|vip|acapella|a cappella|lofi|lo-fi|pitched|tiktok)\b")
        .expect("static regex")
});
static BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[[^\]]*\]|\([^)]*\)").expect("static regex"));
static SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s+-\s+(remaster(ed)?|single|radio edit|album version|live|mono|stereo).*$")
        .expect("static regex")
});
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("static regex"));
static ARTIST_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i),|&| x |feat\.?").expect("static regex"));
static TITLE_DASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+[-–—]\s+").expect("static regex"));

fn tokens(text: &str) -> HashSet<String> {
    norm(text)
        .split(' ')
        .filter(|w| w.chars().count() > 1)
        .map(str::to_string)
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count() as f64;
    shared / (a.len() as f64 + b.len() as f64 - shared)
}

pub fn strip_noise(text: &str) -> String {
    let t = BRACKETS.replace_all(text, " ");
    let t = SUFFIX.replace(&t, "");
    SPACES.replace_all(&t, " ").trim().to_string()
}

pub fn score_candidate(spotify: &RemoteTrack, sc: &Track) -> f64 {
    let spotify_title = strip_noise(&spotify.title);
    let artists: Vec<String> = ARTIST_SPLIT
        .split(&spotify.artist)
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let parts: Vec<&str> = TITLE_DASH.split(&sc.title).collect();
    let sc_title = strip_noise(&if parts.len() > 1 {
        parts[1..].join(" ")
    } else {
        sc.title.clone()
    });

    let spotify_tokens = tokens(&spotify_title);
    let title_sim = jaccard(&spotify_tokens, &tokens(&sc_title))
        .max(jaccard(&spotify_tokens, &tokens(&sc.title)));

    let haystack = tokens(&format!("{} {}", sc.title, sc.artist));
    let artist_sim = if artists.is_empty() {
        0.0
    } else {
        artists
            .iter()
            .filter(|a| tokens(a).iter().all(|w| haystack.contains(w)))
            .count() as f64
            / artists.len() as f64
    };

    let seconds = (sc.duration as f64 - spotify.duration as f64).abs() / 1000.0;
    let duration_sim = if spotify.duration == 0 {
        0.5
    } else if seconds <= 3.0 {
        1.0
    } else if seconds <= 10.0 {
        0.6
    } else if seconds <= 30.0 {
        0.25
    } else {
        0.0
    };

    let mut score = 0.5 * title_sim + 0.3 * artist_sim + 0.2 * duration_sim;
    if VARIANTS.is_match(&sc.title) && !VARIANTS.is_match(&spotify.title) {
        score -= 0.35;
    }
    score
}

#[derive(Clone, Debug)]
pub struct Match {
    pub track: Track,
    pub score: f64,
}

impl Services {
    pub async fn find_match(&self, spotify: &RemoteTrack) -> Option<Match> {
        let artist = spotify
            .artist
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let title = strip_noise(&spotify.title);
        let mut best: Option<Match> = None;
        for q in [format!("{artist} {title}").trim().to_string(), title] {
            if let Ok(json) = self
                .sc_get(
                    "https://api-v2.soundcloud.com/search/tracks",
                    &[("q", q), ("limit", "12".into())],
                )
                .await
            {
                for track in playable(json.get("collection")) {
                    let score = score_candidate(spotify, &track);
                    if best.as_ref().is_none_or(|b| score > b.score) {
                        best = Some(Match { track, score });
                    }
                }
            }
            if best.as_ref().is_some_and(|b| b.score >= GOOD_ENOUGH) {
                break;
            }
        }
        best.filter(|b| b.score >= MIN_SCORE).map(|b| Match {
            score: (b.score * 100.0).round() / 100.0,
            ..b
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(title: &str, artist: &str, dur: u64) -> Track {
        serde_json::from_value(serde_json::json!({"id": 1, "source": "soundcloud", "title": title, "artist": artist, "duration": dur})).unwrap()
    }

    fn sp(title: &str, artist: &str, dur: u64) -> RemoteTrack {
        RemoteTrack {
            title: title.into(),
            artist: artist.into(),
            duration: dur,
        }
    }

    #[test]
    fn strip_noise_removes_brackets_and_release_suffixes() {
        assert_eq!(strip_noise("Song (feat. X) [Live]"), "Song");
        assert_eq!(strip_noise("Song - 2011 Remaster"), "Song - 2011 Remaster");
        assert_eq!(strip_noise("Song - Remastered 2011"), "Song");
        assert_eq!(strip_noise("  A   B "), "A B");
    }

    #[test]
    fn exact_match_scores_high_and_variants_are_penalised() {
        let spotify = sp("Midnight City", "M83", 243_000);
        let exact = score_candidate(&spotify, &sc("M83 - Midnight City", "M83", 243_500));
        assert!(exact > 0.9, "{exact}");
        let remix = score_candidate(
            &spotify,
            &sc("Midnight City (Eric Prydz Remix)", "M83", 243_500),
        );
        assert!(exact - remix > 0.3);
        let other = score_candidate(&spotify, &sc("Completely different", "Someone", 100_000));
        assert!(other < 0.2);
    }

    #[test]
    fn duration_buckets() {
        let t = sc("Song", "Artist", 100_000);
        let near = score_candidate(&sp("Song", "Artist", 102_000), &t);
        let mid = score_candidate(&sp("Song", "Artist", 108_000), &t);
        let far = score_candidate(&sp("Song", "Artist", 160_000), &t);
        assert!(near > mid && mid > far);
        let unknown = score_candidate(&sp("Song", "Artist", 0), &t);
        assert!((unknown - (0.5 + 0.3 + 0.2 * 0.5)).abs() < 1e-9);
    }
}
