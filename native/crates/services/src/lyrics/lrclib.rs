use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::Value;

use crate::Services;

pub const MAX_IN_FLIGHT: usize = 4;

impl Services {
    pub fn lrclib_failures(&self) -> u64 {
        self.inner.lyrics.failures.load(Ordering::Relaxed)
    }

    /// GET `https://lrclib.net/api/<path_and_query>`; `None` for 404s and persistent failures.
    pub async fn lrclib(&self, path_and_query: &str, attempts: u32) -> Option<Value> {
        let _permit = self.inner.lyrics.permits.acquire().await.ok()?;
        for i in 0..attempts {
            let res = self
                .inner
                .http
                .get(format!("https://lrclib.net/api/{path_and_query}"))
                .timeout(Duration::from_secs(8))
                .header("User-Agent", "Mosaic/0.5 (desktop player)")
                .header("Lrclib-Client", "Mosaic/0.5")
                .send()
                .await;
            if let Ok(res) = res {
                let status = res.status();
                if status.as_u16() == 404 {
                    return None;
                }
                if status.is_success() {
                    if let Ok(json) = res.json().await {
                        return Some(json);
                    }
                } else if status.as_u16() < 500 {
                    return None;
                }
            }
            tokio::time::sleep(Duration::from_millis(500 * (u64::from(i) + 1))).await;
        }
        self.inner.lyrics.failures.fetch_add(1, Ordering::Relaxed);
        None
    }
}
