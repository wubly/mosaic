//! What plays after a track ("autoplay") and "Start radio", for every source.
//!
//! The list starts from YouTube Music's radio for the playing song: it follows the song's
//! style closely, where SoundCloud's "related" mostly shares tags. Each radio track is then
//! found on the seed's own service, so a Spotify queue stays on Spotify and so on. A service's
//! own suggestions only pad a list that comes up short.

use std::collections::HashSet;

use anyhow::{bail, Result};
use futures::stream::{self, StreamExt};
use mosaic_core::model::{Source, Track};

use crate::monochrome::{best_match, core_title};
use crate::Services;

/// Lookups in flight at once while matching a radio list.
const CONCURRENCY: usize = 6;
/// Most tracks by one artist in a batch, so a radio doesn't turn into one artist's discography.
const PER_ARTIST: usize = 2;
/// Radio order is shuffled within runs of this many, so the same song doesn't always bring the
/// same queue while what's closest still comes first.
const JITTER_RUN: usize = 5;
/// Of a second seed's radio, how much is blended in (every third track is from it).
const BLEND_EVERY: usize = 3;

/// Shuffles `list` within consecutive runs of `run`, keeping the coarse order.
pub fn jitter<T>(mut list: Vec<T>, run: usize, rng: &mut impl rand::Rng) -> Vec<T> {
    use rand::seq::SliceRandom;
    for chunk in list.chunks_mut(run.max(1)) {
        chunk.shuffle(rng);
    }
    list
}

/// `main` with `extra` slotted in as every `every`-th track, until either runs out.
pub fn blend<T>(main: Vec<T>, extra: Vec<T>, every: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(main.len() + extra.len());
    let mut extra = extra.into_iter();
    for (i, t) in main.into_iter().enumerate() {
        if every > 0 && i % every == every - 1 {
            out.extend(extra.next());
        }
        out.push(t);
    }
    out
}

fn lead_artist(artist: &str) -> &str {
    artist
        .split([',', '&'])
        .next()
        .unwrap_or("")
        .split(" feat")
        .next()
        .unwrap_or("")
        .trim()
}

/// Same song for the purposes of not queueing it twice.
pub fn song_key(t: &Track) -> String {
    format!(
        "{}|{}",
        crate::ranking::words(lead_artist(&t.artist)),
        core_title(&t.title)
    )
}

/// Drops repeats of a song (or of the seeds), and keeps at most `PER_ARTIST` per artist.
/// Order is kept: radio order is the closest-first order.
pub fn curate(seeds: &[Track], list: Vec<Track>, limit: usize) -> Vec<Track> {
    let mut songs: HashSet<String> = seeds.iter().map(song_key).collect();
    let mut ids: HashSet<_> = seeds.iter().map(|t| t.id.clone()).collect();
    let mut per_artist: std::collections::HashMap<String, usize> = Default::default();
    let mut out = Vec::new();
    for t in list {
        if out.len() >= limit {
            break;
        }
        let artist = crate::ranking::words(lead_artist(&t.artist));
        let n = per_artist.entry(artist).or_default();
        if *n >= PER_ARTIST || !ids.insert(t.id.clone()) || !songs.insert(song_key(&t)) {
            continue;
        }
        *n += 1;
        out.push(t);
    }
    out
}

impl Services {
    /// YouTube Music's radio for a song from any service.
    async fn ytm_radio(&self, seed: &Track) -> Result<Vec<Track>> {
        let video_id = match (&seed.source, &seed.video_id) {
            (Source::YouTube, Some(id)) => id.clone(),
            _ => {
                let query = format!("{} {}", lead_artist(&seed.artist), core_title(&seed.title));
                let hits = self.yt_search_music(query.trim()).await?;
                match best_match(seed, &hits).and_then(|t| t.video_id.clone()) {
                    Some(id) => id,
                    None => bail!("YouTube Music doesn't have this song"),
                }
            }
        };
        self.yt_related(&video_id).await
    }

    /// `wanted` (from another service) on `source`, if it's there.
    async fn find_on(&self, source: Source, wanted: Track) -> Option<Track> {
        let query = format!(
            "{} {}",
            lead_artist(&wanted.artist),
            core_title(&wanted.title)
        );
        let query = query.trim();
        let hits = match source {
            Source::Spotify => self.spotify_find_tracks(query, 5).await.ok()?,
            Source::SoundCloud => self.sc_search(query, 8).await.ok()?,
            Source::Monochrome => self.mono_search(query).await.ok()?,
            _ => return Some(wanted),
        };
        best_match(&wanted, &hits).cloned()
    }

    /// The service's own suggestions, used to pad a short radio.
    async fn own_related(&self, seed: &Track, limit: usize) -> Result<Vec<Track>> {
        match seed.source {
            Source::SoundCloud => self.sc_related(&seed.id, limit as u32).await,
            Source::Monochrome => self.mono_related(seed, limit).await,
            _ => Ok(Vec::new()),
        }
    }

    /// Up to `limit` tracks to play after `seeds` (the last one is the playing track), on the
    /// playing track's service. Never one of the seeds, never a song twice.
    pub async fn autoplay(&self, seeds: &[Track], limit: usize) -> Result<Vec<Track>> {
        self.autoplay_mixed(seeds, limit, false).await
    }

    /// `autoplay`; with `mixed`, a song the playing track's service doesn't have comes from
    /// YouTube Music instead of being left out (the experimental mixed-services queue).
    pub async fn autoplay_mixed(
        &self,
        seeds: &[Track],
        limit: usize,
        mixed: bool,
    ) -> Result<Vec<Track>> {
        let Some(seed) = seeds.last() else {
            bail!("Nothing to continue from");
        };
        let mut list = Vec::new();
        // Another recent track, picked at random, flavours the radio a little, so the same
        // song played twice (or on Monochrome and on YouTube) doesn't bring the same queue.
        let second = {
            use rand::seq::IndexedRandom;
            seeds[..seeds.len() - 1].choose(&mut rand::rng()).cloned()
        };
        let (radio, second_radio) = tokio::join!(self.ytm_radio(seed), async {
            match &second {
                Some(s) => self.ytm_radio(s).await.unwrap_or_default(),
                None => Vec::new(),
            }
        });
        let radio = radio.map(|r| {
            let mut rng = rand::rng();
            let main = jitter(r, JITTER_RUN, &mut rng);
            blend(
                main,
                jitter(second_radio, JITTER_RUN, &mut rng),
                BLEND_EVERY,
            )
        });
        match radio {
            Ok(radio) if seed.source == Source::YouTube => list = radio,
            Ok(radio) => {
                // Some won't be on the seed's service, so look up more than needed.
                let source = seed.source.clone();
                let found: Vec<Option<Track>> =
                    stream::iter(radio.into_iter().take(limit + limit / 2 + 4))
                        .map(|t| {
                            let (this, source) = (self.clone(), source.clone());
                            async move {
                                let fallback = mixed.then(|| t.clone());
                                this.find_on(source, t).await.or(fallback)
                            }
                        })
                        .buffered(CONCURRENCY)
                        .collect()
                        .await;
                list = found.into_iter().flatten().collect();
            }
            Err(e) => tracing::info!("autoplay: no YouTube Music radio for this song: {e}"),
        }
        let mut out = curate(seeds, list, limit);
        if out.len() < limit / 2 {
            match self.own_related(seed, limit).await {
                Ok(more) => {
                    let mut all = out;
                    all.extend(more);
                    out = curate(seeds, all, limit);
                }
                Err(e) => tracing::warn!("autoplay: fallback failed: {e}"),
            }
        }
        if out.is_empty() {
            bail!("Couldn't find anything to play after this");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_keeps_runs_and_blend_slots_in_extras() {
        let mut rng = rand::rng();
        let out = jitter((0..20).collect::<Vec<_>>(), 5, &mut rng);
        for (i, run) in out.chunks(5).enumerate() {
            let mut sorted = run.to_vec();
            sorted.sort();
            assert_eq!(
                sorted,
                (i * 5..i * 5 + 5).collect::<Vec<_>>(),
                "stays in its run"
            );
        }
        assert_eq!(
            blend(vec![1, 2, 3, 4, 5, 6], vec![10, 20, 30], 3),
            [1, 2, 10, 3, 4, 5, 20, 6]
        );
        assert_eq!(blend(vec![1, 2], Vec::new(), 3), [1, 2]);
    }
    use mosaic_core::model::TrackId;

    fn t(id: u64, title: &str, artist: &str) -> Track {
        Track {
            id: TrackId::Num(id),
            source: Source::SoundCloud,
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            artwork: String::new(),
            thumb: String::new(),
            duration: 200_000,
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
    fn curate_drops_seeds_repeats_and_caps_artists() {
        let seeds = [t(1, "Get Lucky", "Daft Punk")];
        let list = vec![
            t(2, "Get Lucky (Radio Edit)", "Daft Punk"),
            t(3, "One More Time", "Daft Punk"),
            t(4, "Around the World", "Daft Punk"),
            t(5, "Digital Love", "Daft Punk"),
            t(6, "Midnight City", "M83"),
            t(6, "Midnight City", "M83"),
            t(7, "Midnight City", "M83 & Someone"),
        ];
        let got: Vec<_> = curate(&seeds, list, 10)
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(got, ["One More Time", "Around the World", "Midnight City"]);
    }
}
