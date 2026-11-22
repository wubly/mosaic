use chrono::{Datelike, Duration, NaiveDate};
use indexmap::IndexMap;
use std::collections::HashMap;

use crate::model::PlayStat;

pub const DAYS_SHOWN: usize = 14;

#[derive(Clone, Debug)]
pub struct ArtistStat {
    pub name: String,
    pub count: u64,
    pub seconds: f64,
    pub top: PlayStat,
}

#[derive(Clone, Debug)]
pub struct DayStat {
    pub date: NaiveDate,
    pub seconds: f64,
}

#[derive(Clone, Debug)]
pub struct Summary {
    pub seconds: f64,
    pub plays: u64,
    pub tracks: Vec<PlayStat>,
    pub artists: Vec<ArtistStat>,
    pub artist_count: usize,
    pub recent: Vec<DayStat>,
    pub week: f64,
    pub streak: u32,
}

pub fn day_key(date: NaiveDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

pub fn summarize(
    plays: &IndexMap<String, PlayStat>,
    days: &HashMap<String, f64>,
    today: NaiveDate,
) -> Summary {
    let mut artists: IndexMap<String, ArtistStat> = IndexMap::new();
    for p in plays.values() {
        let a = artists
            .entry(p.track.artist.clone())
            .or_insert_with(|| ArtistStat {
                name: p.track.artist.clone(),
                count: 0,
                seconds: 0.0,
                top: p.clone(),
            });
        a.count += p.count;
        a.seconds += p.seconds;
        if p.count > a.top.count {
            a.top = p.clone();
        }
    }

    let recent: Vec<DayStat> = (0..DAYS_SHOWN)
        .rev()
        .map(|i| {
            let date = today - Duration::days(i as i64);
            DayStat {
                date,
                seconds: days.get(&day_key(date)).copied().unwrap_or(0.0),
            }
        })
        .collect();

    let mut streak = 0;
    for (i, d) in recent.iter().enumerate().rev() {
        if d.seconds > 0.0 {
            streak += 1;
        } else if i != recent.len() - 1 {
            break;
        }
    }

    let mut tracks: Vec<PlayStat> = plays.values().cloned().collect();
    tracks.sort_by(|a, b| b.count.cmp(&a.count).then(b.seconds.total_cmp(&a.seconds)));
    tracks.truncate(6);

    let mut top_artists: Vec<ArtistStat> = artists.values().cloned().collect();
    top_artists.sort_by(|a, b| b.count.cmp(&a.count).then(b.seconds.total_cmp(&a.seconds)));
    top_artists.truncate(6);

    Summary {
        seconds: plays.values().map(|p| p.seconds).sum(),
        plays: plays.values().map(|p| p.count).sum(),
        tracks,
        artists: top_artists,
        artist_count: artists.len(),
        week: recent.iter().rev().take(7).map(|d| d.seconds).sum(),
        recent,
        streak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(id: u64, artist: &str, count: u64, seconds: f64) -> (String, PlayStat) {
        let track = serde_json::from_value(serde_json::json!({
            "id": id, "source": "soundcloud", "title": format!("t{id}"), "artist": artist
        }))
        .unwrap();
        (
            id.to_string(),
            PlayStat {
                track,
                count,
                seconds,
            },
        )
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 22).unwrap()
    }

    fn days(keys: &[(&str, f64)]) -> HashMap<String, f64> {
        keys.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn aggregates_and_sorts() {
        let plays: IndexMap<_, _> = [
            stat(1, "A", 2, 100.0),
            stat(2, "B", 5, 50.0),
            stat(3, "A", 4, 10.0),
        ]
        .into_iter()
        .collect();
        let s = summarize(&plays, &HashMap::new(), today());
        assert_eq!(s.plays, 11);
        assert_eq!(s.seconds, 160.0);
        assert_eq!(s.artist_count, 2);
        assert_eq!(s.tracks[0].count, 5);
        assert_eq!(s.artists[0].name, "A");
        assert_eq!(s.artists[0].count, 6);
        assert_eq!(s.artists[0].top.count, 4);
    }

    #[test]
    fn streak_counts_back_from_today() {
        let d = days(&[
            ("2026-11-22", 10.0),
            ("2026-11-21", 10.0),
            ("2026-11-20", 10.0),
            ("2026-11-18", 10.0),
        ]);
        assert_eq!(summarize(&IndexMap::new(), &d, today()).streak, 3);
    }

    #[test]
    fn empty_today_does_not_break_streak() {
        let d = days(&[("2026-11-21", 10.0), ("2026-11-20", 10.0)]);
        let s = summarize(&IndexMap::new(), &d, today());
        assert_eq!(s.streak, 2);
        assert_eq!(
            summarize(&IndexMap::new(), &days(&[("2026-11-19", 5.0)]), today()).streak,
            0
        );
    }

    #[test]
    fn week_and_window() {
        let d = days(&[
            ("2026-11-22", 10.0),
            ("2026-11-16", 5.0),
            ("2026-11-15", 99.0),
        ]);
        let s = summarize(&IndexMap::new(), &d, today());
        assert_eq!(s.recent.len(), 14);
        assert_eq!(s.recent[13].date, today());
        assert_eq!(s.week, 15.0);
    }
}
