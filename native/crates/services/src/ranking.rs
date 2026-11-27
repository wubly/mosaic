//! Makes SoundCloud and YouTube search results read like Spotify's: songs first, the one you
//! typed on top, no hour-long mixes, no "(Official Video)" in titles, no five copies of one song.
//!
//! The services' own ordering is kept as one signal among several (`trust` says how much it
//! is worth): YouTube Music's song search is already good, SoundCloud's is raw.

use std::collections::HashSet;

use mosaic_core::model::Track;
use regex::Regex;

/// Longest a result can be and still be a song rather than a set, a mix or a full album.
const MAX_SONG_MS: u64 = 20 * 60 * 1000;
/// Shorter is a preview, a snippet or a sound effect.
const MIN_SONG_MS: u64 = 30 * 1000;
/// Two uploads of one title this close in length are the same song.
const SAME_LENGTH_MS: u64 = 2000;

/// Dropped unless the search asked for them.
const NOT_SONGS: &[&str] = &[
    "podcast",
    "episode",
    "full album",
    "nightcore",
    "type beat",
    "karaoke",
    "reaction",
    "dj set",
    "radio show",
    "tutorial",
    "mixtape",
    "live set",
];

/// Real songs, but not the one you meant unless you said so: pushed down the list.
const VARIANTS: &[&str] = &[
    "remix",
    "bootleg",
    "edit",
    "flip",
    "cover",
    "live",
    "sped up",
    "speed up",
    "speedup",
    "slowed",
    "daycore",
    "8d",
    "acoustic",
    "remake",
    "full",
    "version",
    "x",
    "vs",
    "reverb",
    "bass boosted",
    "instrumental",
    "acapella",
    "mashup",
    "rework",
    "vip",
    "mix",
    "tribute",
];

/// What the ranker looks at of one result.
pub struct Hit {
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
    /// Plays, if the service says (0 if not).
    pub plays: u64,
    /// A verified or official channel.
    pub verified: bool,
}

/// Lowercase words only, so titles compare without punctuation, case or spacing in the way.
pub fn words(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_phrase(haystack: &str, needle: &str) -> bool {
    !needle.is_empty() && format!(" {haystack} ").contains(&format!(" {needle} "))
}

fn junk_suffix() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\s*[\(\[][^\)\]]*\b(official|lyrics?|lyric video|audio|video|music video|hd|hq|4k|free download|free dl|out now|premiere|visuali[sz]er|explicit|clip officiel)\b[^\)\]]*[\)\]]\s*$",
        )
        .expect("valid regex")
    })
}

/// A title without its "(Official Video)" / "[Free Download]" tail. Never returns nothing.
pub fn tidy_title(title: &str) -> String {
    static EXT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let ext = EXT.get_or_init(|| {
        Regex::new(r"(?i)(\s*\(?\d+k\)?)?\.(mp3|wav|flac|m4a|aac|ogg)$").expect("valid regex")
    });
    let trimmed = title.trim();
    let stripped = ext.replace(trimmed, "");
    let mut out = if stripped.trim().is_empty() {
        trimmed.to_string()
    } else {
        stripped.trim().to_string()
    };
    loop {
        let next = junk_suffix().replace(&out, "").trim().to_string();
        if next == out || next.is_empty() {
            return out;
        }
        out = next;
    }
}

/// `Artist - Title` uploads: the part before the dash is the artist when it is the uploader
/// (or contains it, "Artist Official"), or short enough to be a name and followed by a title
/// rather than a version note.
pub fn split_artist(title: &str, uploader: &str) -> Option<(String, String)> {
    let (artist, song) = title.split_once(" - ")?;
    if song.contains(" - ") {
        return None;
    }
    let (artist, song) = (artist.trim(), song.trim());
    if artist.is_empty() || song.is_empty() {
        return None;
    }
    let left = words(artist);
    // "A x B - Song" is a mashup or a collab named by its title, not an artist.
    if has_phrase(&left, "x") || has_phrase(&left, "vs") {
        return None;
    }
    let a = left.replace(' ', "");
    let u = words(uploader).replace(' ', "");
    let by_uploader = !u.is_empty() && (a.contains(&u) || u.contains(&a));
    let song_words = words(song);
    let note = VARIANTS
        .iter()
        .chain(NOT_SONGS)
        .any(|m| song_words.starts_with(m));
    let name_like = artist.split_whitespace().count() <= 5 && !note;
    (by_uploader || name_like).then(|| (artist.to_string(), song.to_string()))
}

fn mentions(query: &str, marker: &str) -> bool {
    has_phrase(query, marker)
}

struct Scored {
    index: usize,
    score: f64,
}

/// Indices of `hits` in the order to show them, with the ones that aren't songs left out.
pub fn order(query: &str, hits: &[Hit], trust: f64) -> Vec<usize> {
    let q = words(query);
    let q_words: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    let mut scored: Vec<Scored> = Vec::new();
    let mut cover: Vec<f64> = Vec::new();

    for (index, hit) in hits.iter().enumerate() {
        let title = words(&hit.title);
        let artist = words(&hit.artist);
        let ms = hit.duration_ms;
        if ms != 0 && !(MIN_SONG_MS..=MAX_SONG_MS).contains(&ms) {
            continue;
        }
        if NOT_SONGS
            .iter()
            .any(|m| has_phrase(&title, m) && !mentions(&q, m))
        {
            continue;
        }
        let hay: HashSet<&str> = title.split(' ').chain(artist.split(' ')).collect();
        let matched: f64 = q_words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if hay.contains(w) {
                    1.0
                } else if i + 1 == q_words.len()
                    && w.len() >= 2
                    && hay.iter().any(|h| h.starts_with(w))
                {
                    // Still typing the last word.
                    0.8
                } else {
                    0.0
                }
            })
            .sum();
        let coverage = if q_words.is_empty() {
            1.0
        } else {
            matched / q_words.len() as f64
        };

        let mut score = 4.0 * coverage;
        if !q.is_empty() {
            if title == q {
                score += 3.0;
            } else if title.starts_with(&format!("{q} ")) {
                score += 1.0;
            } else if has_phrase(&title, &q) {
                score += 0.8;
            }
            if artist == q {
                score += 2.0;
            } else if has_phrase(&artist, &q) {
                score += 1.0;
            }
            // "radiohead creep" for Radiohead - Creep, in either order.
            let covered = title
                .split(' ')
                .chain(artist.split(' '))
                .all(|w| q_words.contains(&w));
            if covered && coverage >= 1.0 {
                score += 2.5;
            }
        }
        let extra = title
            .split(' ')
            .filter(|w| !q_words.contains(w) && !artist.split(' ').any(|a| a == *w))
            .count();
        score -= (extra as f64 * 0.08).min(1.0);
        if hit.plays > 0 {
            // 1K plays is nothing, 1B is a hit: a cover with 21K plays trails the original with
            // 3.7B by about what a place or two in the services' own order is worth.
            score += 1.2 * (((hit.plays as f64).log10() - 3.0) / 6.5).clamp(0.0, 1.0);
        }
        if hit.verified {
            score += 0.4;
        }
        score += trust / (1.0 + 0.2 * index as f64);
        let variants = VARIANTS
            .iter()
            .filter(|m| has_phrase(&title, m) && !mentions(&q, m))
            .count();
        score -= (variants as f64 * 2.0).min(4.0);
        if ms > 10 * 60 * 1000 {
            score -= 1.0;
        }
        scored.push(Scored { index, score });
        cover.push(coverage);
    }

    // Something that shares no word with the search is only worth showing when nothing does.
    if cover.iter().any(|c| *c > 0.0) {
        let mut k = 0;
        scored.retain(|_| {
            k += 1;
            cover[k - 1] > 0.0
        });
    }
    scored.sort_by(|a, b| b.score.total_cmp(&a.score));

    let mut seen: Vec<(String, u64)> = Vec::new();
    scored
        .into_iter()
        .filter(|s| {
            let h = &hits[s.index];
            let title = words(&h.title);
            if seen
                .iter()
                .any(|(t, d)| *t == title && d.abs_diff(h.duration_ms) <= SAME_LENGTH_MS)
            {
                return false;
            }
            seen.push((title, h.duration_ms));
            true
        })
        .map(|s| s.index)
        .collect()
}

/// Tidies, ranks and trims search results. `plays` and `verified` line up with `tracks` (and
/// may be empty when the service doesn't say).
pub fn refine(
    query: &str,
    mut tracks: Vec<Track>,
    plays: &[u64],
    verified: &[bool],
    trust: f64,
    limit: usize,
) -> Vec<Track> {
    for t in &mut tracks {
        let title = tidy_title(&t.title);
        match split_artist(&title, &t.artist) {
            Some((artist, song)) => {
                t.artist = artist;
                t.title = tidy_title(&song);
            }
            None => t.title = title,
        }
    }
    let hits: Vec<Hit> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| Hit {
            title: t.title.clone(),
            artist: t.artist.clone(),
            duration_ms: t.duration,
            plays: plays.get(i).copied().unwrap_or(0),
            verified: verified.get(i).copied().unwrap_or(false),
        })
        .collect();
    let mut slots: Vec<Option<Track>> = tracks.into_iter().map(Some).collect();
    order(query, &hits, trust)
        .into_iter()
        .take(limit)
        .filter_map(|i| slots[i].take())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: &str, artist: &str, secs: u64, plays: u64) -> Hit {
        Hit {
            title: title.into(),
            artist: artist.into(),
            duration_ms: secs * 1000,
            plays,
            verified: false,
        }
    }

    fn titles(query: &str, hits: &[Hit]) -> Vec<String> {
        order(query, hits, 1.5)
            .into_iter()
            .map(|i| hits[i].title.clone())
            .collect()
    }

    #[test]
    fn titles_lose_their_video_tags_and_keep_their_versions() {
        assert_eq!(tidy_title("Creep (Official Video)"), "Creep");
        assert_eq!(tidy_title("Creep [Lyrics] (HD)"), "Creep");
        assert_eq!(
            tidy_title("Strobe (Club Edit) [Free Download]"),
            "Strobe (Club Edit)"
        );
        assert_eq!(tidy_title("Creep - Acoustic"), "Creep - Acoustic");
        assert_eq!(tidy_title("(Official)"), "(Official)");
        assert_eq!(tidy_title("Blinding Lights (128k).mp3"), "Blinding Lights");
        assert_eq!(tidy_title("Creep (original).mp3"), "Creep (original)");
    }

    #[test]
    fn artist_dash_title_splits_when_it_reads_like_a_name() {
        assert_eq!(
            split_artist("Radiohead - Creep", "radioheadVEVO"),
            Some(("Radiohead".into(), "Creep".into()))
        );
        assert_eq!(
            split_artist("Some Label Guy - Night Drive", "xX_uploader_Xx"),
            Some(("Some Label Guy".into(), "Night Drive".into()))
        );
        // A version note after the dash is not a title.
        assert_eq!(split_artist("Creep - Live at Glastonbury", "Fan"), None);
        assert_eq!(split_artist("A - B - C", "x"), None);
        assert_eq!(split_artist("Berserk x Creep - Radiohead", "Berserk"), None);
        assert_eq!(split_artist("No dash here", "x"), None);
    }

    #[test]
    fn the_song_you_typed_comes_before_remixes_and_covers() {
        let hits = [
            hit("Creep (Skrillex Remix)", "dj", 200, 9_000_000),
            hit("Creep (cover)", "someone", 210, 50_000),
            hit("Creep", "Radiohead", 238, 80_000_000),
            hit("Creeping Death", "Metallica", 400, 1_000_000),
        ];
        let out = titles("radiohead creep", &hits);
        assert_eq!(out[0], "Creep");
        assert!(out.iter().position(|t| t == "Creeping Death").is_none() || out.len() == 4);
    }

    #[test]
    fn mixes_sets_and_previews_are_dropped() {
        let hits = [
            hit("Night Drive", "Artist", 215, 100),
            hit("Night Drive (1 hour loop)", "Artist", 3600, 100),
            hit("Night Drive snippet", "Artist", 12, 100),
            hit("Night Drive nightcore", "Artist", 180, 100),
        ];
        assert_eq!(titles("night drive", &hits), ["Night Drive"]);
    }

    #[test]
    fn asking_for_a_remix_keeps_remixes() {
        let hits = [
            hit("Strobe", "deadmau5", 600, 100),
            hit("Strobe (Club Edit)", "deadmau5", 400, 100),
            hit("Strobe (Remix)", "someone", 300, 100),
        ];
        let out = titles("strobe remix", &hits);
        assert_eq!(out[0], "Strobe (Remix)");
    }

    #[test]
    fn duplicates_collapse_to_the_better_upload() {
        let hits = [
            hit("Midnight City", "random upload", 244, 10),
            hit("Midnight City", "M83", 244, 90_000_000),
            hit("Midnight City", "Other Artist", 301, 5),
        ];
        let out = order("midnight city", &hits, 1.5);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], 1);
    }

    #[test]
    fn unrelated_results_only_show_when_nothing_matches() {
        let hits = [
            hit("Completely Different", "Nobody", 200, 5),
            hit("Creep", "Radiohead", 238, 5),
        ];
        assert_eq!(titles("creep", &hits), ["Creep"]);
        let none = [hit("Completely Different", "Nobody", 200, 5)];
        assert_eq!(titles("zzz", &none), ["Completely Different"]);
    }

    #[test]
    fn a_half_typed_last_word_still_matches() {
        let hits = [
            hit("Creep", "Radiohead", 238, 5),
            hit("Karma Police", "Radiohead", 260, 5),
        ];
        assert_eq!(titles("radiohead cre", &hits)[0], "Creep");
    }

    #[test]
    fn the_services_own_order_counts_when_trusted() {
        let hits = [hit("Song A", "x", 200, 0), hit("Song B", "x", 200, 0)];
        assert_eq!(order("song", &hits, 3.0), [0, 1]);
    }
}
