use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use image::imageops::FilterType;
use image::RgbaImage;
use lru::LruCache;
use sha1::{Digest, Sha1};
use tokio::sync::{OnceCell, Semaphore};

use crate::Services;

/// Decoded images kept here for re-requests. The app holds its own handles to what is on screen
/// (sharing these pixels), so this only needs to cover recently scrolled-past artwork.
const MEMORY_ENTRIES: usize = 48;
/// Decodes running at once. A page of covers otherwise starts one blocking thread per image, each
/// holding a full-size decode, which spikes memory and leaves the allocator fragmented.
const DECODE_PERMITS: usize = 4;

type Key = (String, u32);
type Slot = Arc<OnceCell<Result<Arc<RgbaImage>, String>>>;

pub struct State {
    lru: Mutex<LruCache<Key, Arc<RgbaImage>>>,
    pending: Mutex<HashMap<Key, Slot>>,
    decoding: Semaphore,
}

impl State {
    pub fn new() -> State {
        State {
            lru: Mutex::new(LruCache::new(
                NonZeroUsize::new(MEMORY_ENTRIES).expect("non-zero"),
            )),
            pending: Mutex::default(),
            decoding: Semaphore::new(DECODE_PERMITS),
        }
    }
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}

fn is_remote(src: &str) -> bool {
    src.starts_with("http://") || src.starts_with("https://")
}

fn extension_of(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match path.rsplit_once('.') {
        Some((_, ext))
            if (1..=4).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            ext
        }
        _ => "img",
    }
}

fn decode(bytes: &[u8], max_px: u32) -> Result<RgbaImage> {
    let mut image = image::load_from_memory(bytes)?;
    // Box-sample big artwork (a 3000 px album cover) down to twice the target first: Lanczos over
    // the full image allocates a float buffer of width × target height, tens of MB per cover.
    let near = max_px.saturating_mul(2);
    if image.width().max(image.height()) > near {
        image = image.thumbnail(near, near);
    }
    let image = if image.width().max(image.height()) > max_px {
        image.resize(max_px, max_px, FilterType::Lanczos3)
    } else {
        image
    };
    Ok(image.into_rgba8())
}

impl Services {
    fn cached_file(&self, url: &str) -> PathBuf {
        let hash: String = Sha1::digest(url.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.inner
            .dirs
            .images()
            .join(format!("{hash}.{}", extension_of(url)))
    }

    async fn image_bytes(&self, src: &str) -> Result<Vec<u8>> {
        if !is_remote(src) {
            return Ok(tokio::fs::read(src).await?);
        }
        let file = self.cached_file(src);
        if let Ok(bytes) = tokio::fs::read(&file).await {
            return Ok(bytes);
        }
        let res = self.inner.http.get(src).send().await?;
        if !res.status().is_success() {
            return Err(anyhow!("image request failed ({})", res.status().as_u16()));
        }
        let bytes = res.bytes().await?.to_vec();
        if let Some(dir) = file.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        let _ = tokio::fs::write(&file, &bytes).await;
        Ok(bytes)
    }

    /// Forgets the decoded images kept for re-requests, once the app has let go of artwork it no
    /// longer shows. What is still on screen is shared with the app and stays; the rest reloads
    /// from the disk cache if it comes back.
    pub fn trim_image_memory(&self) {
        if let Ok(mut lru) = self.inner.images.lru.lock() {
            lru.clear();
        }
    }

    /// Loads a URL or absolute path, fits it inside `max_px`, and caches it on disk and in memory.
    pub async fn load_image(&self, src: &str, max_px: u32) -> Result<Arc<RgbaImage>> {
        let key: Key = (src.to_string(), max_px);
        if let Some(hit) = self
            .inner
            .images
            .lru
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .get(&key)
        {
            return Ok(Arc::clone(hit));
        }

        let slot: Slot = {
            let mut pending = self
                .inner
                .images
                .pending
                .lock()
                .map_err(|_| anyhow!("poisoned"))?;
            Arc::clone(pending.entry(key.clone()).or_default())
        };
        let result = slot
            .get_or_init(|| async {
                let load = async {
                    let bytes = self.image_bytes(&key.0).await?;
                    let max = key.1;
                    let _permit = self.inner.images.decoding.acquire().await?;
                    tokio::task::spawn_blocking(move || decode(&bytes, max)).await?
                };
                load.await.map(Arc::new).map_err(|e| e.to_string())
            })
            .await
            .clone();

        if let Ok(mut pending) = self.inner.images.pending.lock() {
            pending.remove(&key);
        }
        let image = result.map_err(|e| anyhow!(e))?;
        if let Ok(mut lru) = self.inner.images.lru.lock() {
            lru.put(key, Arc::clone(&image));
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(extension_of("https://a/b/c.jpg?x=1"), "jpg");
        assert_eq!(extension_of("https://a/b/c"), "img");
        assert_eq!(extension_of("https://a.b/c"), "img");
    }

    #[tokio::test]
    async fn loads_and_resizes_local_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        RgbaImage::new(400, 200).save(&path).unwrap();
        let services = Services::new(
            mosaic_core::storage::Dirs::with_root(dir.path()),
            Default::default(),
        )
        .unwrap();
        let img = services
            .load_image(path.to_str().unwrap(), 100)
            .await
            .unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        let again = services
            .load_image(path.to_str().unwrap(), 100)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&img, &again));
        assert!(services.load_image("/nonexistent.png", 100).await.is_err());
    }

    #[test]
    fn big_artwork_is_fitted_through_a_thumbnail() {
        let mut big = RgbaImage::from_pixel(1200, 600, image::Rgba([200, 40, 60, 255]));
        big.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        big.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let img = decode(bytes.get_ref(), 100).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        let px = img.get_pixel(50, 25).0;
        assert!(px[0] > 190 && px[1] < 50, "keeps the colour: {px:?}");
    }
}
