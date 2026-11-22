use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde_json::Value;

use crate::Services;

const UNREACHABLE: &str = "Could not reach SoundCloud. Check your connection.";

impl Services {
    async fn discover_client_id(&self) -> Result<String> {
        let found = self.try_discover_client_id().await;
        match found {
            Ok(Some(id)) => {
                if let Ok(mut slot) = self.inner.sc_client_id.lock() {
                    *slot = Some(id.clone());
                }
                self.update_config(|c| c.client_id = Some(id.clone()));
                Ok(id)
            }
            _ => bail!(UNREACHABLE),
        }
    }

    async fn try_discover_client_id(&self) -> Result<Option<String>> {
        let html = self
            .inner
            .http
            .get("https://soundcloud.com")
            .send()
            .await?
            .text()
            .await?;
        let script =
            Regex::new(r#"<script[^>]+src="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)""#)?;
        let id = Regex::new(r#"client_id\s*[:=]\s*"([a-zA-Z0-9]{32})""#)?;

        let mut scripts: Vec<String> = script
            .captures_iter(&html)
            .map(|c| c[1].to_string())
            .collect();
        scripts.reverse();
        for src in scripts {
            let js = self.inner.http.get(&src).send().await?.text().await?;
            if let Some(m) = id.captures(&js) {
                return Ok(Some(m[1].to_string()));
            }
        }
        Ok(None)
    }

    async fn client_id(&self) -> Result<String> {
        let cached = self
            .inner
            .sc_client_id
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .clone();
        match cached {
            Some(id) => Ok(id),
            None => self.discover_client_id().await,
        }
    }

    pub async fn sc_get(&self, url: &str, params: &[(&str, String)]) -> Result<Value> {
        let mut retry = true;
        loop {
            let id = self.client_id().await?;
            let mut target = url::Url::parse(url)?;
            {
                let mut q = target.query_pairs_mut();
                for (k, v) in params {
                    q.append_pair(k, v);
                }
                q.append_pair("client_id", &id);
            }
            let res = self
                .inner
                .http
                .get(target)
                .header("Accept", "application/json")
                .timeout(Duration::from_secs(15))
                .send()
                .await?;

            let status = res.status();
            if (status.as_u16() == 401 || status.as_u16() == 403) && retry {
                retry = false;
                self.discover_client_id().await?;
                continue;
            }
            if !status.is_success() {
                bail!("SoundCloud request failed ({})", status.as_u16());
            }
            return Ok(res.json().await?);
        }
    }
}
