use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::widget::image::Handle;

const FADE: Duration = Duration::from_millis(150);
/// Decoded handles kept around. Past this, the ones drawn longest ago are dropped (they reload
/// from the services' memory or disk cache if they come back into view).
const CAPACITY: usize = 160;
/// Never drop something drawn this recently, even over capacity.
const KEEP_RECENT: Duration = Duration::from_secs(3);
/// Handles nothing has drawn for this long are dropped even under capacity, so memory goes
/// back down after browsing; coming back reloads them from the services' caches.
const IDLE_EXPIRY: Duration = Duration::from_secs(60);
/// How often idle handles are looked for (only while there are any).
pub const TRIM_EVERY: Duration = Duration::from_secs(20);

pub type Key = (String, u32);

struct Entry {
    handle: Handle,
    loaded: Instant,
    /// Last time `view` drew it.
    used: std::cell::Cell<Instant>,
}

/// Decoded artwork handles. `view` can't start work, so it records what it wants in `wanted`
/// and the app drains that after each update.
#[derive(Default)]
pub struct ImageCache {
    ready: HashMap<Key, Entry>,
    requested: RefCell<HashSet<Key>>,
    wanted: RefCell<Vec<Key>>,
    /// The newest draw: what that drew is on screen now, however long ago it was.
    latest: std::cell::Cell<Option<Instant>>,
}

impl ImageCache {
    /// The cached handle and its fade-in opacity, or `None` (and a load request).
    pub fn art(&self, src: &str, px: u32, now: Instant) -> Option<(Handle, f32)> {
        if src.is_empty() {
            return None;
        }
        let key = (src.to_string(), px);
        if self.latest.get().is_none_or(|l| now > l) {
            self.latest.set(Some(now));
        }
        match self.ready.get(&key) {
            Some(e) => {
                e.used.set(now);
                let t = now.saturating_duration_since(e.loaded).as_secs_f32() / FADE.as_secs_f32();
                let opacity = if crate::anim::reduced_motion() {
                    1.0
                } else {
                    t.clamp(0.0, 1.0)
                };
                Some((e.handle.clone(), opacity))
            }
            None => {
                if self.requested.borrow_mut().insert(key.clone()) {
                    self.wanted.borrow_mut().push(key);
                }
                None
            }
        }
    }

    pub fn has_wanted(&self) -> bool {
        !self.wanted.borrow().is_empty()
    }

    pub fn take_wanted(&self) -> Vec<Key> {
        std::mem::take(&mut *self.wanted.borrow_mut())
    }

    pub fn insert(&mut self, key: Key, handle: Handle, now: Instant) {
        self.ready.insert(
            key,
            Entry {
                handle,
                loaded: now,
                used: std::cell::Cell::new(now),
            },
        );
        self.trim(now);
    }

    /// Drops the least recently drawn handles once over capacity.
    fn trim(&mut self, now: Instant) {
        if self.ready.len() <= CAPACITY {
            return;
        }
        let mut by_age: Vec<(Instant, Key)> = self
            .ready
            .iter()
            .filter(|(_, e)| now.saturating_duration_since(e.used.get()) > KEEP_RECENT)
            .map(|(k, e)| (e.used.get(), k.clone()))
            .collect();
        by_age.sort_by_key(|(used, _)| *used);
        let excess = self.ready.len() - CAPACITY * 3 / 4;
        let mut requested = self.requested.borrow_mut();
        for (_, key) in by_age.into_iter().take(excess) {
            self.ready.remove(&key);
            requested.remove(&key);
        }
    }

    /// A handle over shared pixels, without copying them.
    pub fn handle_for(img: std::sync::Arc<::image::RgbaImage>) -> Handle {
        struct Pixels(std::sync::Arc<::image::RgbaImage>);
        impl AsRef<[u8]> for Pixels {
            fn as_ref(&self) -> &[u8] {
                self.0.as_raw()
            }
        }
        let (w, h) = img.dimensions();
        Handle::from_rgba(w, h, bytes::Bytes::from_owner(Pixels(img)))
    }

    pub fn is_empty(&self) -> bool {
        self.ready.is_empty()
    }

    /// Drops handles not drawn for `IDLE_EXPIRY`; returns how many went. What the latest draw
    /// showed stays: the window may simply not have been redrawn in a while.
    pub fn expire(&mut self, now: Instant) -> usize {
        let before = self.ready.len();
        let latest = self.latest.get();
        let mut requested = self.requested.borrow_mut();
        self.ready.retain(|key, e| {
            let used = e.used.get();
            let keep = Some(used) == latest || now.saturating_duration_since(used) < IDLE_EXPIRY;
            if !keep {
                requested.remove(key);
            }
            keep
        });
        before - self.ready.len()
    }

    pub fn is_fading(&self, now: Instant) -> bool {
        self.ready
            .values()
            .any(|e| now.saturating_duration_since(e.loaded) < FADE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_handles_expire_and_can_load_again() {
        let mut cache = ImageCache::default();
        let now = Instant::now();
        let handle = Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
        cache.insert(("a".into(), 64), handle.clone(), now);
        cache.insert(("b".into(), 64), handle, now);
        let later = now + IDLE_EXPIRY / 2;
        assert!(cache.art("a", 64, later).is_some());
        // Long after, with nothing redrawn since: "a" is still on screen, "b" isn't.
        assert_eq!(cache.expire(now + IDLE_EXPIRY * 3), 1, "b went");
        assert!(cache.art("a", 64, later).is_some(), "a stayed");
        assert!(cache.art("b", 64, now).is_none(), "b loads again");
        assert_eq!(cache.take_wanted(), [("b".to_string(), 64)]);
    }

    #[test]
    fn art_requests_once_and_fades_in_when_ready() {
        let mut cache = ImageCache::default();
        let now = Instant::now();
        assert!(cache.art("http://x/a.jpg", 64, now).is_none());
        assert!(cache.art("http://x/a.jpg", 64, now).is_none());
        assert_eq!(cache.take_wanted().len(), 1);
        assert!(!cache.has_wanted());

        let handle = Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
        cache.insert(("http://x/a.jpg".into(), 64), handle, now);
        let (_, opacity) = cache.art("http://x/a.jpg", 64, now).expect("ready");
        assert_eq!(opacity, 0.0);
        assert!(cache.is_fading(now));
        let (_, later) = cache
            .art("http://x/a.jpg", 64, now + Duration::from_millis(400))
            .expect("ready");
        assert_eq!(later, 1.0);
        assert!(cache.art("", 64, now).is_none());
    }

    #[test]
    fn old_handles_are_dropped_over_capacity_and_can_reload() {
        let mut cache = ImageCache::default();
        let start = Instant::now();
        for i in 0..CAPACITY + 1 {
            let h = Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
            cache.insert(
                (format!("old{i}"), 64),
                h,
                start + Duration::from_millis(i as u64),
            );
        }
        assert_eq!(
            cache.ready.len(),
            CAPACITY + 1,
            "just-drawn images are kept"
        );
        let later = start + Duration::from_secs(10);
        let h = Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
        cache.insert(("new".into(), 64), h, later);
        assert!(cache.ready.len() <= CAPACITY * 3 / 4 + 1);
        assert!(cache.ready.contains_key(&("new".to_string(), 64)));
        assert!(
            cache.art("old0", 64, later).is_none(),
            "an evicted image asks again"
        );
        assert_eq!(cache.take_wanted().len(), 1);
    }
}
