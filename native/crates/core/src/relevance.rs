use std::sync::LazyLock;

use regex::Regex;

use crate::model::{Source, Track};

const MIN: u64 = 45_000;
const MAX: u64 = 12 * 60_000;

static TALK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(podcast|episode|ep\.?\s?\d+|interview|full (album|mixtape|show|set)|radio show|live ?stream|audiobook|asmr|sermon|mixtape)\b",
    )
    .expect("static regex")
});

fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in s.to_lowercase().chars() {
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

pub fn fits_along(candidate: &Track, seed: Option<&Track>) -> bool {
    if candidate.source != Source::SoundCloud {
        return true;
    }
    let length = candidate.duration;
    let seed_len = seed.map_or(0.0, |s| s.duration as f64 * 1.5);
    let max = (MAX as f64).max(seed_len);

    if length != 0 && (length < MIN || length as f64 > max) {
        return false;
    }
    let seed_title = seed.map_or("", |s| s.title.as_str());
    if TALK.is_match(&candidate.title) && !TALK.is_match(seed_title) {
        return false;
    }
    match seed {
        None => true,
        Some(s) => clean(&candidate.title) != clean(&s.title) || candidate.id == s.id,
    }
}

pub fn filter_along(list: Vec<Track>, seed: Option<&Track>) -> Vec<Track> {
    list.into_iter().filter(|t| fits_along(t, seed)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackId;

    fn track(id: u64, title: &str, duration: u64) -> Track {
        serde_json::from_value(serde_json::json!({
            "id": id, "source": "soundcloud", "title": title, "artist": "a", "duration": duration
        }))
        .unwrap()
    }

    #[test]
    fn rejects_talk_unless_seed_is_talk() {
        let seed = track(1, "Song", 200_000);
        assert!(!fits_along(
            &track(2, "My Podcast Ep. 12", 200_000),
            Some(&seed)
        ));
        assert!(!fits_along(
            &track(2, "Full Album stream", 200_000),
            Some(&seed)
        ));
        let talk_seed = track(1, "Podcast 1", 200_000);
        assert!(fits_along(
            &track(2, "Podcast 2", 200_000),
            Some(&talk_seed)
        ));
        assert!(fits_along(&track(2, "Repeat", 200_000), Some(&seed)));
    }

    #[test]
    fn duration_bounds() {
        let seed = track(1, "Song", 200_000);
        assert!(!fits_along(&track(2, "Short", 44_999), Some(&seed)));
        assert!(fits_along(&track(2, "Edge", 45_000), Some(&seed)));
        assert!(!fits_along(&track(2, "Long", 12 * 60_000 + 1), Some(&seed)));
        assert!(fits_along(&track(2, "Unknown length", 0), Some(&seed)));
        let long_seed = track(1, "Set", 20 * 60_000);
        assert!(fits_along(&track(2, "Long", 25 * 60_000), Some(&long_seed)));
        assert!(!fits_along(
            &track(2, "Longer", 31 * 60_000),
            Some(&long_seed)
        ));
    }

    #[test]
    fn same_title_dedupe() {
        let seed = track(1, "Hello, World!", 200_000);
        assert!(!fits_along(&track(2, "hello world", 200_000), Some(&seed)));
        let mut same = track(1, "hello world", 200_000);
        same.id = TrackId::Num(1);
        assert!(fits_along(&same, Some(&seed)));
        assert!(fits_along(&track(2, "hello world", 200_000), None));
    }

    #[test]
    fn non_soundcloud_always_fits() {
        let mut t = track(2, "Podcast", 1);
        t.source = Source::Local;
        assert!(fits_along(&t, None));
    }

    #[test]
    fn clean_collapses_separators() {
        assert_eq!(clean("  Hi -- There!! "), "hi there");
    }
}
