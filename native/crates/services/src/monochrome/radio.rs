//! Autoplay and "Start radio" for Monochrome. The catalog has no related-tracks endpoint, so
//! the list comes from YouTube Music's radio for the same song, and each track on it is
//! looked up in Monochrome's catalog by title, artist and length. When that comes up short
//! (no YouTube Music match, or few of its tracks are in the catalog), the seed artist's top
//! tracks and releases fill in.

use std::collections::HashSet;

use anyhow::Result;
use futures::stream::{self, StreamExt};
use mosaic_core::model::Track;
use regex::Regex;
use serde_json::Value;

use super::slim_all;
use crate::ranking::words;
use crate::Services;

/// Lookups in flight at once while matching a YouTube Music radio list.
const CONCURRENCY: usize = 8;
/// Two takes of one song can differ this much in length (radio edits, longer intros).
const MAX_LENGTH_GAP_MS: u64 = 15_000;

/// A title without brackets or a "feat." tail: "Get Lucky (Radio Edit) [feat. Pharrell]" and
/// "Get Lucky (feat. Pharrell Williams)" both become "get lucky".
pub fn core_title(title: &str) -> String {
    static BRACKETS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static FEAT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let brackets = BRACKETS.get_or_init(|| Regex::new(r"[\(\[][^\)\]]*[\)\]]").expect("regex"));
    let feat = FEAT
        .get_or_init(|| Regex::new(r"(?i)\s+(feat\.?|ft\.?|featuring|with)\s.*$").expect("regex"));
    let stripped = brackets.replace_all(title, " ");
    let stripped = feat.replace(&stripped, "");
    let out = words(&stripped);
    if out.is_empty() {
        words(title)
    } else {
        out
    }
}

/// Words in brackets or after a dash that make it a different take of the song.
fn version_marks(title: &str) -> HashSet<&'static str> {
    const MARKS: &[&str] = &[
        "remix",
        "live",
        "acoustic",
        "instrumental",
        "remaster",
        "remastered",
        "demo",
        "edit",
        "mix",
        "version",
        "slowed",
        "sped",
        "karaoke",
        "cover",
    ];
    let w = words(title);
    let set: HashSet<&str> = w.split(' ').collect();
    MARKS.iter().copied().filter(|m| set.contains(m)).collect()
}

fn squash(s: &str) -> String {
    words(s).replace(' ', "")
}

/// Every credited artist, squashed, from "A, B & C" style strings.
fn artist_names(artists: &str) -> Vec<String> {
    artists
        .split([',', '&'])
        .flat_map(|a| a.split(" x "))
        .map(squash)
        .filter(|a| !a.is_empty())
        .collect()
}

fn shares_artist(a: &str, b: &str) -> bool {
    let (a, b) = (artist_names(a), artist_names(b));
    a.iter().any(|x| {
        b.iter()
            .any(|y| x == y || x.contains(y.as_str()) || y.contains(x.as_str()))
    })
}

/// How far apart two takes are in length, if both lengths are known.
fn length_gap(a: &Track, b: &Track) -> Option<u64> {
    (a.duration > 0 && b.duration > 0).then(|| a.duration.abs_diff(b.duration))
}

/// Whether `candidate` is the song `wanted` names; a lower score is a closer take.
pub fn match_score(wanted: &Track, candidate: &Track) -> Option<u64> {
    if core_title(&wanted.title) != core_title(&candidate.title) {
        return None;
    }
    if !shares_artist(&wanted.artist, &candidate.artist) {
        return None;
    }
    let gap = length_gap(wanted, candidate).unwrap_or(5_000);
    if gap > MAX_LENGTH_GAP_MS {
        return None;
    }
    // A remix or live take when the original was asked for (or the other way round) is a
    // worse pick, but still the right song if nothing else is there.
    let marks = version_marks(&wanted.title)
        .symmetric_difference(&version_marks(&candidate.title))
        .count() as u64;
    Some(gap + marks * 20_000)
}

/// The closest take of `wanted` among `candidates`.
pub fn best_match<'a>(wanted: &Track, candidates: &'a [Track]) -> Option<&'a Track> {
    candidates
        .iter()
        .filter_map(|c| match_score(wanted, c).map(|s| (s, c)))
        .min_by_key(|(s, _)| *s)
        .map(|(_, c)| c)
}

/// Same song for the purposes of not playing it twice in one radio.
fn song_key(t: &Track) -> String {
    let lead = artist_names(&t.artist)
        .into_iter()
        .next()
        .unwrap_or_default();
    format!("{lead}|{}", core_title(&t.title))
}

impl Services {
    /// Raw catalog hits for a lookup, without the ranker (matching does its own checks).
    async fn mono_lookup(&self, query: &str) -> Result<Vec<Track>> {
        let json = self
            .mono_get(
                "/search/tracks",
                &[("q", query.to_string()), ("limit", "10".into())],
            )
            .await?;
        Ok(slim_all(&json["tracks"]))
    }

    /// The catalog's take of a song found elsewhere, if it has one.
    async fn mono_find(&self, wanted: &Track) -> Option<Track> {
        let lead = wanted.artist.split([',', '&']).next().unwrap_or("").trim();
        let query = format!("{lead} {}", core_title(&wanted.title));
        let hits = self.mono_lookup(query.trim()).await.ok()?;
        best_match(wanted, &hits).cloned()
    }

    /// YouTube Music's radio for `seed`, as songs found in the catalog, in radio order.
    async fn mono_related_via_ytm(&self, seed: &Track, want: usize) -> Result<Vec<Track>> {
        let lead = seed.artist.split([',', '&']).next().unwrap_or("").trim();
        let hits = self
            .yt_search_music(&format!("{lead} {}", seed.title))
            .await?;
        let Some(video_id) = best_match(seed, &hits).and_then(|t| t.video_id.clone()) else {
            anyhow::bail!("YouTube Music doesn't have this song");
        };
        let radio = self.yt_related(&video_id).await?;
        // Some won't be in the catalog, so look up more than needed.
        // Owned values in and out: borrowed closures here trip the `Send` check of the
        // future the app spawns.
        let found: Vec<Option<Track>> = stream::iter(radio.into_iter().take(want + want / 2 + 5))
            .map(|t| {
                let this = self.clone();
                async move { this.mono_find(&t).await }
            })
            .buffered(CONCURRENCY)
            .collect()
            .await;
        Ok(found.into_iter().flatten().collect())
    }

    /// The seed artist's top tracks, then the tracks of their latest albums and singles.
    async fn mono_artist_tracks(&self, artist_id: &str, want: usize) -> Result<Vec<Track>> {
        let json = self.mono_get(&format!("/artists/{artist_id}"), &[]).await?;
        let mut out = slim_all(&json["topTracks"]);
        let release_ids: Vec<String> = ["albums", "singles"]
            .iter()
            .flat_map(|k| json[*k].as_array().cloned().unwrap_or_default())
            .filter_map(|r: Value| {
                r["releaseId"]
                    .as_str()
                    .or_else(|| r["id"].as_str())
                    .map(str::to_string)
            })
            .take(4)
            .collect();
        let releases: Vec<Result<Value>> = stream::iter(release_ids)
            .map(|id| {
                let this = self.clone();
                async move { this.mono_get(&format!("/releases/{id}"), &[]).await }
            })
            .buffered(4)
            .collect()
            .await;
        for release in releases.into_iter().flatten() {
            let album = release["title"].as_str().unwrap_or_default().to_string();
            out.extend(slim_all(&release["tracks"]).into_iter().map(|mut t| {
                if t.album.is_empty() {
                    t.album = album.clone();
                }
                t
            }));
            if out.len() >= want * 2 {
                break;
            }
        }
        Ok(out)
    }

    /// Songs to play after (or as a radio of) a Monochrome track: up to `limit`, never the
    /// seed itself and never one song twice.
    pub async fn mono_related(&self, seed: &Track, limit: usize) -> Result<Vec<Track>> {
        let mut seen: HashSet<String> = HashSet::from([song_key(seed)]);
        let mut ids = HashSet::from([seed.id.clone()]);
        let mut out = Vec::new();
        let mut take = |list: Vec<Track>, out: &mut Vec<Track>| {
            for t in list {
                if out.len() >= limit {
                    break;
                }
                if ids.insert(t.id.clone()) && seen.insert(song_key(&t)) {
                    out.push(t);
                }
            }
        };

        let via_ytm = self.mono_related_via_ytm(seed, limit).await;
        match via_ytm {
            Ok(list) => take(list, &mut out),
            Err(e) => tracing::info!("Monochrome radio: YouTube Music list unavailable: {e}"),
        }
        // A thin list (few of the radio's songs are in the catalog) is padded from the artist.
        if out.len() < limit / 2 {
            if let Some(artist_id) = seed.artist_id.as_deref() {
                match self.mono_artist_tracks(artist_id, limit).await {
                    Ok(list) => take(list, &mut out),
                    Err(e) => tracing::warn!("Monochrome radio: artist fallback failed: {e}"),
                }
            }
        }
        if out.is_empty() {
            anyhow::bail!("Couldn't find anything to play after this");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mosaic_core::model::{Source, TrackId};

    fn t(title: &str, artist: &str, secs: u64) -> Track {
        Track {
            id: TrackId::Str(format!("x:{title}")),
            source: Source::Monochrome,
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            artwork: String::new(),
            thumb: String::new(),
            duration: secs * 1000,
            permalink: None,
            streamable: true,
            folder: None,
            lrc_path: None,
            uri: None,
            artist_id: None,
            album_id: None,
            video_id: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn core_title_drops_brackets_and_features() {
        assert_eq!(
            core_title("Get Lucky (Radio Edit) [feat. Pharrell Williams and Nile Rodgers]"),
            "get lucky"
        );
        assert_eq!(core_title("Get Lucky feat. Pharrell Williams"), "get lucky");
        assert_eq!(core_title("(Untitled)"), "untitled");
    }

    #[test]
    fn matches_same_song_across_services() {
        let ytm = t("One More Time", "Daft Punk", 320);
        let original = t("One More Time", "Daft Punk", 320);
        let edit = t("One More Time - Radio Edit", "Daft Punk", 235);
        let other = t("One More Time", "Britney Spears", 320);
        let list = [other, edit, original.clone()];
        let picked = best_match(&ytm, &list).unwrap();
        assert_eq!(picked.id, original.id);
    }

    #[test]
    fn prefers_the_original_over_a_remix() {
        let wanted = t("Midnight City", "M83", 244);
        let remix = t("Midnight City (Eric Prydz Remix)", "M83", 244);
        let original = t("Midnight City", "M83", 250);
        let list = [remix, original.clone()];
        let picked = best_match(&wanted, &list).unwrap();
        assert_eq!(picked.id, original.id);
    }

    #[test]
    fn credits_match_any_listed_artist() {
        let wanted = t("Get Lucky", "Daft Punk, Pharrell Williams", 248);
        let hit = t(
            "Get Lucky",
            "Daft Punk & Pharrell Williams & Nile Rodgers",
            248,
        );
        assert!(match_score(&wanted, &hit).is_some());
        assert!(match_score(&wanted, &t("Get Lucky", "Daft Punk", 400)).is_none());
    }
}
