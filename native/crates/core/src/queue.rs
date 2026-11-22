use rand::random_range;

use crate::model::{Repeat, Track};

#[derive(Clone, Debug)]
pub struct Queue {
    pub list: Vec<Track>,
    pub index: isize,
}

impl Default for Queue {
    fn default() -> Self {
        Queue {
            list: Vec::new(),
            index: -1,
        }
    }
}

#[derive(Clone, Debug)]
pub enum QueueOutcome {
    /// Nothing queued yet: start playing instead of enqueueing.
    PlayNow(Box<Track>, Option<Vec<Track>>),
    AlreadyQueued,
    Added,
    AddedMany(usize),
    PlayingNext,
    Nothing,
}

impl QueueOutcome {
    pub fn toast(&self) -> Option<String> {
        match self {
            QueueOutcome::AlreadyQueued => Some("Already in your queue".into()),
            QueueOutcome::Added => Some("Added to queue".into()),
            QueueOutcome::AddedMany(n) => Some(format!(
                "Added {n} track{} to the queue",
                if *n > 1 { "s" } else { "" }
            )),
            QueueOutcome::PlayingNext => Some("Playing next".into()),
            _ => None,
        }
    }
}

pub fn radio_start_toast(title: &str) -> String {
    format!("Starting radio from “{title}”")
}

pub const RADIO_EMPTY: &str = "No similar tracks found";
pub const RADIO_FAILED: &str = "Could not start radio";

impl Queue {
    pub fn current(&self) -> Option<&Track> {
        usize::try_from(self.index)
            .ok()
            .and_then(|i| self.list.get(i))
    }

    /// Replaces the queue: dedupes by id (first wins), makes sure `target` is in it, and points at it.
    pub fn play(&mut self, target: &Track, list: Option<&[Track]>) {
        let mut unique: Vec<Track> = Vec::new();
        match list {
            Some(list) => {
                for t in list {
                    if !unique.iter().any(|u| u.id == t.id) {
                        unique.push(t.clone());
                    }
                }
            }
            None => unique.push(target.clone()),
        }
        if !unique.iter().any(|t| t.id == target.id) {
            unique.insert(0, target.clone());
        }
        let index = unique.iter().position(|t| t.id == target.id).unwrap_or(0);
        self.list = unique;
        self.index = index as isize;
    }

    pub fn add(&mut self, track: &Track) -> QueueOutcome {
        if self.list.is_empty() {
            return QueueOutcome::PlayNow(Box::new(track.clone()), None);
        }
        if self.list.iter().any(|t| t.id == track.id) {
            return QueueOutcome::AlreadyQueued;
        }
        self.list.push(track.clone());
        QueueOutcome::Added
    }

    pub fn enqueue_many(&mut self, tracks: &[Track]) -> QueueOutcome {
        let Some(first) = tracks.first() else {
            return QueueOutcome::Nothing;
        };
        if self.list.is_empty() {
            return QueueOutcome::PlayNow(Box::new(first.clone()), Some(tracks.to_vec()));
        }
        let fresh: Vec<Track> = tracks
            .iter()
            .filter(|t| !self.list.iter().any(|h| h.id == t.id))
            .cloned()
            .collect();
        if fresh.is_empty() {
            return QueueOutcome::AlreadyQueued;
        }
        let n = fresh.len();
        self.list.extend(fresh);
        QueueOutcome::AddedMany(n)
    }

    pub fn play_next(&mut self, track: &Track) -> QueueOutcome {
        if self.list.is_empty() {
            return QueueOutcome::PlayNow(Box::new(track.clone()), None);
        }
        let current_id = self.current().map(|t| t.id.clone());
        let mut without: Vec<Track> = self
            .list
            .iter()
            .filter(|t| t.id != track.id)
            .cloned()
            .collect();
        let find = |list: &[Track]| {
            current_id
                .as_ref()
                .and_then(|id| list.iter().position(|t| &t.id == id))
        };
        let insert_at = find(&without).unwrap_or(0) + 1;
        without.insert(insert_at.min(without.len()), track.clone());

        self.index = find(&without).map_or(-1, |i| i as isize);
        self.list = without;
        QueueOutcome::PlayingNext
    }

    /// Removing the playing entry is a no-op.
    pub fn remove(&mut self, index: usize) {
        let i = index as isize;
        if i == self.index || index >= self.list.len() {
            return;
        }
        self.list.remove(index);
        if i < self.index {
            self.index -= 1;
        }
    }

    pub fn reorder(&mut self, list: Vec<Track>) {
        let current = self.current().map(|t| t.id.clone());
        let index = current
            .and_then(|id| list.iter().position(|t| t.id == id))
            .unwrap_or(0);
        self.list = list;
        self.index = index as isize;
    }

    pub fn clear(&mut self) {
        match self.current().cloned() {
            Some(t) => {
                self.list = vec![t];
                self.index = 0;
            }
            None => *self = Queue::default(),
        }
    }

    pub fn radio_list(track: &Track, related: Vec<Track>) -> Vec<Track> {
        let mut list = vec![track.clone()];
        list.extend(related.into_iter().filter(|t| t.id != track.id));
        list
    }

    /// Index to step to, or `None` at the end of the queue.
    pub fn next_index(&self, direction: isize, shuffle: bool, repeat: Repeat) -> Option<usize> {
        self.next_index_with(direction, shuffle, repeat, |n| random_range(0..n))
    }

    pub fn next_index_with(
        &self,
        direction: isize,
        shuffle: bool,
        repeat: Repeat,
        pick: impl FnMut(usize) -> usize,
    ) -> Option<usize> {
        step_index(
            self.list.len(),
            self.index,
            direction,
            shuffle,
            repeat,
            pick,
        )
    }
}

/// `next_index` on bare numbers, so callers can walk past unplayable entries.
pub fn step_index(
    len: usize,
    from: isize,
    direction: isize,
    shuffle: bool,
    repeat: Repeat,
    mut pick: impl FnMut(usize) -> usize,
) -> Option<usize> {
    if shuffle && direction > 0 && len > 1 {
        loop {
            let p = pick(len);
            if p as isize != from {
                return Some(p);
            }
        }
    }
    let index = from + direction;
    if index >= len as isize {
        return (repeat == Repeat::All).then_some(0);
    }
    Some(index.max(0) as usize)
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

    fn ids(q: &Queue) -> Vec<String> {
        q.list.iter().map(|t| t.id.key()).collect()
    }

    fn queue(ids: &[u64], index: isize) -> Queue {
        Queue {
            list: ids.iter().map(|i| t(*i)).collect(),
            index,
        }
    }

    #[test]
    fn play_dedupes_and_inserts_target() {
        let mut q = Queue::default();
        q.play(&t(3), Some(&[t(1), t(2), t(1), t(3)]));
        assert_eq!(ids(&q), ["1", "2", "3"]);
        assert_eq!(q.index, 2);
        q.play(&t(9), Some(&[t(1), t(2)]));
        assert_eq!(ids(&q), ["9", "1", "2"]);
        assert_eq!(q.index, 0);
        q.play(&t(5), None);
        assert_eq!(ids(&q), ["5"]);
    }

    #[test]
    fn add_variants() {
        let mut q = Queue::default();
        assert!(matches!(q.add(&t(1)), QueueOutcome::PlayNow(..)));
        let mut q = queue(&[1], 0);
        assert!(matches!(q.add(&t(1)), QueueOutcome::AlreadyQueued));
        assert_eq!(q.add(&t(2)).toast().as_deref(), Some("Added to queue"));
        assert_eq!(ids(&q), ["1", "2"]);
    }

    #[test]
    fn enqueue_many_variants() {
        let mut q = Queue::default();
        assert!(matches!(q.enqueue_many(&[]), QueueOutcome::Nothing));
        assert!(matches!(
            q.enqueue_many(&[t(1), t(2)]),
            QueueOutcome::PlayNow(_, Some(_))
        ));
        let mut q = queue(&[1], 0);
        assert!(matches!(
            q.enqueue_many(&[t(1)]),
            QueueOutcome::AlreadyQueued
        ));
        let o = q.enqueue_many(&[t(1), t(2), t(3)]);
        assert_eq!(o.toast().as_deref(), Some("Added 2 tracks to the queue"));
        let o = q.enqueue_many(&[t(4)]);
        assert_eq!(o.toast().as_deref(), Some("Added 1 track to the queue"));
    }

    #[test]
    fn play_next_moves_after_current() {
        let mut q = queue(&[1, 2, 3, 4], 1);
        q.play_next(&t(4));
        assert_eq!(ids(&q), ["1", "2", "4", "3"]);
        assert_eq!(q.index, 1);
        q.play_next(&t(1));
        assert_eq!(ids(&q), ["2", "1", "4", "3"]);
        assert_eq!(q.index, 0);
        let mut e = Queue::default();
        assert!(matches!(e.play_next(&t(1)), QueueOutcome::PlayNow(..)));
    }

    #[test]
    fn remove_adjusts_index() {
        let mut q = queue(&[1, 2, 3], 1);
        q.remove(1);
        assert_eq!(ids(&q), ["1", "2", "3"]);
        q.remove(0);
        assert_eq!((ids(&q), q.index), (vec!["2".into(), "3".into()], 0));
        q.remove(1);
        assert_eq!((ids(&q), q.index), (vec!["2".into()], 0));
    }

    #[test]
    fn reorder_follows_current() {
        let mut q = queue(&[1, 2, 3], 0);
        q.reorder(vec![t(3), t(1), t(2)]);
        assert_eq!(q.index, 1);
    }

    #[test]
    fn clear_keeps_current() {
        let mut q = queue(&[1, 2, 3], 1);
        q.clear();
        assert_eq!((ids(&q), q.index), (vec!["2".into()], 0));
        let mut e = Queue::default();
        e.clear();
        assert_eq!(e.index, -1);
    }

    #[test]
    fn radio_excludes_seed_duplicates() {
        let l = Queue::radio_list(&t(1), vec![t(2), t(1), t(3)]);
        assert_eq!(
            l.iter().map(|t| t.id.key()).collect::<Vec<_>>(),
            ["1", "2", "3"]
        );
    }

    #[test]
    fn next_index_matrix() {
        let q = queue(&[1, 2, 3], 1);
        let n = |q: &Queue, d, s, r| q.next_index_with(d, s, r, |_| 0);
        assert_eq!(n(&q, 1, false, Repeat::Off), Some(2));
        assert_eq!(n(&q, -1, false, Repeat::Off), Some(0));
        let end = queue(&[1, 2, 3], 2);
        assert_eq!(n(&end, 1, false, Repeat::Off), None);
        assert_eq!(n(&end, 1, false, Repeat::All), Some(0));
        assert_eq!(n(&end, 1, false, Repeat::One), None);
        let start = queue(&[1, 2, 3], 0);
        assert_eq!(n(&start, -1, false, Repeat::Off), Some(0));
        let single = queue(&[1], 0);
        assert_eq!(n(&single, 1, true, Repeat::Off), None);
    }

    #[test]
    fn shuffle_never_picks_current() {
        let q = queue(&[1, 2, 3], 1);
        let mut seq = [1usize, 1, 2].into_iter();
        assert_eq!(
            q.next_index_with(1, true, Repeat::Off, |_| seq.next().unwrap()),
            Some(2)
        );
        for _ in 0..200 {
            assert_ne!(q.next_index(1, true, Repeat::Off), Some(1));
        }
        assert_eq!(q.next_index_with(-1, true, Repeat::Off, |_| 0), Some(0));
    }
}
