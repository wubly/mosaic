use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::widget::image::Handle;

const FADE: Duration = Duration::from_millis(150);

pub type Key = (String, u32);

struct Entry {
    handle: Handle,
    loaded: Instant,
}

/// Decoded artwork handles. `view` can't start work, so it records what it wants in `wanted`
/// and the app drains that after each update.
#[derive(Default)]
pub struct ImageCache {
    ready: HashMap<Key, Entry>,
    requested: RefCell<HashSet<Key>>,
    wanted: RefCell<Vec<Key>>,
}

impl ImageCache {
    /// The cached handle and its fade-in opacity, or `None` (and a load request).
    pub fn art(&self, src: &str, px: u32, now: Instant) -> Option<(Handle, f32)> {
        if src.is_empty() {
            return None;
        }
        let key = (src.to_string(), px);
        match self.ready.get(&key) {
            Some(e) => {
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
            },
        );
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
}
