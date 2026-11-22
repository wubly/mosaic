use chrono::NaiveDate;
use rand::random_range;

use crate::model::{HistoryEntry, PlayStat, Playlist, Session, Store, Track, TrackId};
use crate::stats::day_key;

const HISTORY_CAP: usize = 100;

fn new_playlist_id() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..8)
        .map(|_| CHARS[random_range(0..CHARS.len())] as char)
        .collect()
}

impl Store {
    pub fn save_session(&mut self, patch: Session) {
        self.session
            .get_or_insert_with(Session::default)
            .merge(patch);
    }

    pub fn is_liked(&self, id: &TrackId) -> bool {
        self.likes.iter().any(|t| &t.id == id)
    }

    /// Returns the new liked state.
    pub fn toggle_like(&mut self, track: &Track) -> bool {
        if self.is_liked(&track.id) {
            self.likes.retain(|t| t.id != track.id);
            false
        } else {
            self.likes.insert(0, track.strip());
            true
        }
    }

    pub fn create_playlist(&mut self, name: &str, tracks: &[Track]) -> String {
        let id = new_playlist_id();
        let name = name.trim();
        self.playlists.push(Playlist {
            id: id.clone(),
            name: if name.is_empty() {
                "Untitled playlist".into()
            } else {
                name.into()
            },
            tracks: tracks.iter().map(Track::strip).collect(),
            extra: Default::default(),
        });
        id
    }

    pub fn rename_playlist(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) {
            p.name = name.into();
        }
    }

    pub fn delete_playlist(&mut self, id: &str) {
        self.playlists.retain(|p| p.id != id);
    }

    /// Returns `false` when the playlist is missing or already has the track.
    pub fn add_to_playlist(&mut self, id: &str, track: &Track) -> bool {
        match self.playlists.iter_mut().find(|p| p.id == id) {
            Some(p) if !p.tracks.iter().any(|t| t.id == track.id) => {
                p.tracks.push(track.strip());
                true
            }
            _ => false,
        }
    }

    pub fn remove_from_playlist(&mut self, id: &str, track_id: &TrackId) {
        if let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) {
            p.tracks.retain(|t| &t.id != track_id);
        }
    }

    pub fn reorder_playlist(&mut self, id: &str, tracks: Vec<Track>) {
        if let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) {
            p.tracks = tracks;
        }
    }

    pub fn record_play(&mut self, track: &Track, at_ms: i64) {
        let stripped = track.strip();
        if self.history.first().is_some_and(|h| h.track.id == track.id) {
            self.history.remove(0);
        }
        self.history.insert(
            0,
            HistoryEntry {
                track: stripped.clone(),
                at: at_ms,
            },
        );
        self.history.truncate(HISTORY_CAP);

        let key = track.id.key();
        let seconds = self.plays.get(&key).map_or(0.0, |p| p.seconds);
        let count = self.plays.get(&key).map_or(0, |p| p.count);
        self.plays.insert(
            key,
            PlayStat {
                track: stripped,
                count: count + 1,
                seconds,
            },
        );
    }

    pub fn add_seconds(&mut self, track: &Track, secs: f64, today: NaiveDate) {
        let stat = self
            .plays
            .entry(track.id.key())
            .or_insert_with(|| PlayStat {
                track: track.strip(),
                count: 0,
                seconds: 0.0,
            });
        stat.seconds += secs;
        *self.days.entry(day_key(today)).or_insert(0.0) += secs;
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        self.plays.clear();
        self.days.clear();
    }

    pub fn add_sources(&mut self, paths: &[String]) {
        for p in paths {
            if !self.sources.contains(p) {
                self.sources.push(p.clone());
            }
        }
    }

    pub fn remove_source(&mut self, path: &str) {
        self.sources.retain(|s| s != path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: u64) -> Track {
        serde_json::from_value(serde_json::json!({
            "id": id, "source": "soundcloud", "title": format!("t{id}"), "artist": "a"
        }))
        .unwrap()
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 22).unwrap()
    }

    #[test]
    fn likes_toggle_newest_first() {
        let mut s = Store::default();
        assert!(s.toggle_like(&t(1)));
        assert!(s.toggle_like(&t(2)));
        assert_eq!(s.likes[0].id, TrackId::Num(2));
        assert!(!s.toggle_like(&t(1)));
        assert!(!s.is_liked(&TrackId::Num(1)));
    }

    #[test]
    fn playlist_lifecycle() {
        let mut s = Store::default();
        let id = s.create_playlist("  ", &[t(1)]);
        assert_eq!(id.len(), 8);
        assert!(id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_eq!(s.playlists[0].name, "Untitled playlist");
        s.rename_playlist(&id, "  Mix ");
        assert_eq!(s.playlists[0].name, "Mix");
        s.rename_playlist(&id, "   ");
        assert_eq!(s.playlists[0].name, "Mix");
        assert!(!s.add_to_playlist(&id, &t(1)));
        assert!(s.add_to_playlist(&id, &t(2)));
        assert!(!s.add_to_playlist("nope", &t(3)));
        s.reorder_playlist(&id, vec![t(2), t(1)]);
        assert_eq!(s.playlists[0].tracks[0].id, TrackId::Num(2));
        s.remove_from_playlist(&id, &TrackId::Num(2));
        assert_eq!(s.playlists[0].tracks.len(), 1);
        s.delete_playlist(&id);
        assert!(s.playlists.is_empty());
    }

    #[test]
    fn strip_drops_lrc_path_for_non_local() {
        let mut track = t(1);
        track.lrc_path = Some("/a.lrc".into());
        let mut s = Store::default();
        s.toggle_like(&track);
        assert!(s.likes[0].lrc_path.is_none());
        track.source = crate::model::Source::Local;
        assert!(track.strip().lrc_path.is_some());
    }

    #[test]
    fn history_dedupes_head_only_and_caps() {
        let mut s = Store::default();
        s.record_play(&t(1), 1);
        s.record_play(&t(1), 2);
        assert_eq!(s.history.len(), 1);
        assert_eq!(s.history[0].at, 2);
        s.record_play(&t(2), 3);
        s.record_play(&t(1), 4);
        assert_eq!(s.history.len(), 3);
        for i in 10..200 {
            s.record_play(&t(i), i as i64);
        }
        assert_eq!(s.history.len(), 100);
        assert_eq!(s.plays["1"].count, 3);
    }

    #[test]
    fn seconds_accumulate_per_track_and_day() {
        let mut s = Store::default();
        s.record_play(&t(1), 1);
        s.add_seconds(&t(1), 15.0, day());
        s.add_seconds(&t(1), 15.0, day());
        s.add_seconds(&t(9), 5.0, day());
        assert_eq!(s.plays["1"].seconds, 30.0);
        assert_eq!(s.plays["1"].count, 1);
        assert_eq!(s.plays["9"].count, 0);
        assert_eq!(s.days["2026-11-22"], 35.0);
        s.clear_history();
        assert!(s.plays.is_empty() && s.days.is_empty() && s.history.is_empty());
    }

    #[test]
    fn sources_dedupe() {
        let mut s = Store::default();
        s.add_sources(&["/a".into(), "/b".into(), "/a".into()]);
        s.add_sources(&["/b".into()]);
        assert_eq!(s.sources, ["/a", "/b"]);
        s.remove_source("/a");
        assert_eq!(s.sources, ["/b"]);
    }

    #[test]
    fn session_patches_merge() {
        let mut s = Store::default();
        s.save_session(Session {
            index: Some(2),
            ..Default::default()
        });
        s.save_session(Session {
            position: Some(4.5),
            ..Default::default()
        });
        let session = s.session.unwrap();
        assert_eq!((session.index, session.position), (Some(2), Some(4.5)));
    }
}
