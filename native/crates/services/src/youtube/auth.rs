//! PKCE sign-in against the user's own Google OAuth client (port of `electron/youtube/auth.js`).
//! The sign-in only unlocks browsing the account (search, liked videos, playlists); playing a
//! video needs no account.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::Services;
use mosaic_core::storage::{read_json, write_json_atomic};

pub const PORT: u16 = 8899;
pub const REDIRECT: &str = "http://127.0.0.1:8899/callback";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";
const AUTH_FILE: &str = "youtube-auth.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    /// Epoch milliseconds.
    pub expires: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub client_id: String,
    pub has_secret: bool,
    pub connected: bool,
    pub user: Option<String>,
}

#[derive(Default)]
pub struct State {
    tokens: Mutex<Option<Tokens>>,
    channel: Mutex<Option<String>>,
    /// The anonymous visitor id InnerTube asks `/player` callers to present (see `innertube`).
    pub(super) visitor: Mutex<Option<String>>,
}

impl State {
    pub fn load(dirs: &mosaic_core::storage::Dirs) -> State {
        let tokens: Option<Tokens> = read_json(&dirs.file(AUTH_FILE));
        State {
            tokens: Mutex::new(tokens.filter(|t| t.refresh.is_some())),
            channel: Mutex::default(),
            visitor: Mutex::default(),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn random_b64(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill(&mut buf[..]);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(client_id: &str, challenge: &str, state: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT),
            ("scope", SCOPE),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge),
            ("state", state),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ])
        .finish();
    format!("https://accounts.google.com/o/oauth2/v2/auth?{query}")
}

impl Services {
    fn yt_client_id(&self) -> String {
        self.config()
            .youtube_client_id
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    fn yt_client_secret(&self) -> String {
        self.config()
            .youtube_client_secret
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// An empty secret keeps the stored one, so the field can stay blank after the first save.
    pub fn yt_set_credentials(&self, id: &str, secret: &str) {
        let (id, secret) = (id.trim().to_string(), secret.trim().to_string());
        self.update_config(|c| {
            c.youtube_client_id = Some(id);
            if !secret.is_empty() {
                c.youtube_client_secret = Some(secret);
            }
        });
    }

    async fn yt_token_request(&self, params: &[(&str, &str)]) -> Result<Value> {
        let (id, secret) = (self.yt_client_id(), self.yt_client_secret());
        let mut form: Vec<(&str, &str)> = vec![("client_id", &id)];
        if !secret.is_empty() {
            form.push(("client_secret", &secret));
        }
        form.extend_from_slice(params);
        let res = self
            .http()
            .post("https://oauth2.googleapis.com/token")
            .form(&form)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let status = res.status();
        let json: Value = res.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!(json["error_description"]
                .as_str()
                .or_else(|| json["error"].as_str())
                .map_or_else(
                    || format!("Google said {}", status.as_u16()),
                    str::to_string
                ));
        }
        Ok(json)
    }

    fn yt_store(&self, json: &Value) -> Result<()> {
        let previous = self
            .inner
            .youtube
            .tokens
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .clone();
        let tokens = Tokens {
            access: json["access_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            refresh: json["refresh_token"]
                .as_str()
                .map(str::to_string)
                .or_else(|| previous.and_then(|t| t.refresh)),
            expires: now_ms() + (json["expires_in"].as_i64().unwrap_or(3600) - 60) * 1000,
        };
        let _ = write_json_atomic(&self.inner.dirs.file(AUTH_FILE), &tokens);
        *self
            .inner
            .youtube
            .tokens
            .lock()
            .map_err(|_| anyhow!("poisoned"))? = Some(tokens);
        Ok(())
    }

    pub(crate) async fn yt_token(&self) -> Result<String> {
        let current = self
            .inner
            .youtube
            .tokens
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .clone()
            .filter(|t| t.refresh.is_some())
            .ok_or_else(|| anyhow!("Not connected to YouTube"))?;
        if now_ms() < current.expires {
            return Ok(current.access);
        }
        let refresh = current.refresh.unwrap_or_default();
        let json = self
            .yt_token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh)])
            .await?;
        self.yt_store(&json)?;
        Ok(json["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    async fn yt_load_channel(&self) -> Option<String> {
        let json = self
            .yt_api(
                "/channels",
                &[("part", "snippet".into()), ("mine", "true".into())],
            )
            .await
            .ok()?;
        let name = json["items"][0]["snippet"]["title"].as_str()?.to_string();
        *self.inner.youtube.channel.lock().ok()? = Some(name.clone());
        Some(name)
    }

    pub async fn yt_status(&self) -> Status {
        let has_tokens = self.inner.youtube.tokens.lock().is_ok_and(|t| t.is_some());
        let mut channel = self
            .inner
            .youtube
            .channel
            .lock()
            .ok()
            .and_then(|c| c.clone());
        if has_tokens && channel.is_none() {
            channel = self.yt_load_channel().await;
        }
        Status {
            client_id: self.yt_client_id(),
            has_secret: !self.yt_client_secret().is_empty(),
            connected: has_tokens && channel.is_some(),
            user: channel,
        }
    }

    pub fn yt_disconnect(&self) {
        if let Ok(mut t) = self.inner.youtube.tokens.lock() {
            *t = None;
        }
        if let Ok(mut c) = self.inner.youtube.channel.lock() {
            *c = None;
        }
        let _ = std::fs::remove_file(self.inner.dirs.file(AUTH_FILE));
    }

    /// Runs the browser sign-in. `open` is handed the authorize URL once the callback port is bound.
    pub async fn yt_connect(&self, open: impl FnOnce(&str)) -> Result<Status> {
        if self.yt_client_id().is_empty() {
            bail!("Add your Google client ID first");
        }
        let verifier = random_b64(48);
        let state = random_b64(12);
        let listener = crate::oauth::bind(PORT).await?;
        open(&authorize_url(
            &self.yt_client_id(),
            &challenge_for(&verifier),
            &state,
        ));

        let callback = crate::oauth::wait_for_callback(&listener, "/callback").await?;
        if callback.params.get("state") != Some(&state) {
            callback
                .respond("Something went wrong. Close this tab and try again.")
                .await;
            bail!("State mismatch");
        }
        if callback.params.contains_key("error") {
            callback.respond("Google sign-in was cancelled.").await;
            bail!("Sign-in cancelled");
        }
        let code = callback.params.get("code").cloned().unwrap_or_default();
        let result = self
            .yt_token_request(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("code_verifier", &verifier),
            ])
            .await
            .and_then(|json| self.yt_store(&json));
        match result {
            Ok(()) => {
                self.yt_load_channel().await;
                callback
                    .respond("Connected. You can close this tab and go back to Mosaic.")
                    .await;
                Ok(self.yt_status().await)
            }
            Err(e) => {
                callback
                    .respond("Could not finish signing in. Close this tab and try again.")
                    .await;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_the_rfc_vector() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_is_read_only_and_offline() {
        let url = authorize_url("cid", "chal", "st");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=cid") && url.contains("code_challenge=chal"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube.readonly"));
        assert!(url.contains("access_type=offline") && url.contains("prompt=consent"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8899%2Fcallback"));
    }
}
