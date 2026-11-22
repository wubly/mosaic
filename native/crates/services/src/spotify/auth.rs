//! PKCE sign-in against the user's own Spotify app (port of `electron/spotify/auth.js`).
//! Playback is librespot's job, so the `streaming` and playback-control scopes are not requested.

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

pub const PORT: u16 = 8898;
pub const REDIRECT: &str = "http://127.0.0.1:8898/callback";
const SCOPES: &str = "user-read-email user-read-private user-library-read user-library-modify user-follow-read user-follow-modify playlist-read-private playlist-read-collaborative playlist-modify-public playlist-modify-private user-top-read user-read-recently-played";
/// Scopes added after the first release; a token without them is read-only until reconnected.
const EDIT_SCOPES: [&str; 4] = [
    "user-library-modify",
    "user-follow-modify",
    "playlist-modify-public",
    "playlist-modify-private",
];
const AUTH_FILE: &str = "spotify-auth.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    /// Epoch milliseconds.
    pub expires: i64,
    /// Space-separated scopes the user granted.
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub country: String,
    pub premium: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub client_id: String,
    pub connected: bool,
    pub user: Option<String>,
    pub premium: bool,
    /// The token may change the library and playlists (granted the edit scopes).
    pub can_edit: bool,
}

#[derive(Default)]
pub struct State {
    tokens: Mutex<Option<Tokens>>,
    profile: Mutex<Option<Profile>>,
}

impl State {
    pub fn load(dirs: &mosaic_core::storage::Dirs) -> State {
        let tokens: Option<Tokens> = read_json(&dirs.file(AUTH_FILE));
        State {
            tokens: Mutex::new(tokens.filter(|t| t.refresh.is_some())),
            profile: Mutex::default(),
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
            ("scope", SCOPES),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge),
            ("state", state),
        ])
        .finish();
    format!("https://accounts.spotify.com/authorize?{query}")
}

impl Services {
    fn spotify_client_id(&self) -> String {
        self.config()
            .spotify_client_id
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    pub fn spotify_set_client_id(&self, id: &str) {
        let id = id.trim().to_string();
        self.update_config(|c| c.spotify_client_id = Some(id));
    }

    async fn spotify_token_request(&self, params: &[(&str, &str)]) -> Result<Value> {
        let client_id = self.spotify_client_id();
        let mut form: Vec<(&str, &str)> = vec![("client_id", &client_id)];
        form.extend_from_slice(params);
        let res = self
            .http()
            .post("https://accounts.spotify.com/api/token")
            .form(&form)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let ok = res.status().is_success();
        let status = res.status();
        let json: Value = res.json().await.unwrap_or(Value::Null);
        if !ok {
            let msg = json["error_description"]
                .as_str()
                .or_else(|| json["error"].as_str())
                .map_or_else(
                    || format!("Spotify said {}", status.as_u16()),
                    str::to_string,
                );
            bail!(msg);
        }
        Ok(json)
    }

    fn spotify_store(&self, json: &Value) -> Result<()> {
        let previous = self
            .inner
            .spotify
            .tokens
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .clone();
        let previous_scope = previous.as_ref().and_then(|t| t.scope.clone());
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
            scope: json["scope"]
                .as_str()
                .map(str::to_string)
                .or_else(|| previous_scope.clone()),
        };
        let _ = write_json_atomic(&self.inner.dirs.file(AUTH_FILE), &tokens);
        *self
            .inner
            .spotify
            .tokens
            .lock()
            .map_err(|_| anyhow!("poisoned"))? = Some(tokens);
        Ok(())
    }

    pub async fn spotify_token(&self) -> Result<String> {
        let current = self
            .inner
            .spotify
            .tokens
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .clone()
            .filter(|t| t.refresh.is_some())
            .ok_or_else(|| anyhow!("Not connected to Spotify"))?;
        if now_ms() < current.expires {
            return Ok(current.access);
        }
        let refresh = current.refresh.unwrap_or_default();
        let json = self
            .spotify_token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh)])
            .await?;
        self.spotify_store(&json)?;
        Ok(json["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// GET against the Web API; `Value::Null` for 204.
    pub async fn spotify_api(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.spotify_send(reqwest::Method::GET, path, query, None)
            .await
    }

    /// Any Web API call; `Value::Null` for an empty body.
    pub async fn spotify_send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("https://api.spotify.com/v1{path}")
        };
        let mut req = self
            .http()
            .request(method, url)
            .bearer_auth(self.spotify_token().await?)
            .query(query)
            .timeout(Duration::from_secs(15));
        req = match body {
            Some(b) => req.json(&b),
            None => req.header("content-length", "0"),
        };
        let res = req.send().await?;
        let status = res.status();
        let bytes = res.bytes().await.unwrap_or_default();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if !status.is_success() {
            if status.as_u16() == 403 && !self.spotify_can_edit() {
                bail!("Reconnect Spotify in Settings to allow changes from Mosaic");
            }
            bail!(json["error"]["message"].as_str().map_or_else(
                || format!("Spotify said {}", status.as_u16()),
                str::to_string
            ));
        }
        Ok(json)
    }

    /// Whether the stored token carries the library and playlist edit scopes.
    pub fn spotify_can_edit(&self) -> bool {
        self.inner
            .spotify
            .tokens
            .lock()
            .ok()
            .and_then(|t| t.as_ref().and_then(|t| t.scope.clone()))
            .is_some_and(|s| {
                let granted: Vec<&str> = s.split_whitespace().collect();
                EDIT_SCOPES.iter().all(|e| granted.contains(e))
            })
    }

    /// The signed-in user's Spotify id.
    pub fn spotify_user_id(&self) -> String {
        self.inner
            .spotify
            .profile
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.id.clone()))
            .unwrap_or_default()
    }

    async fn spotify_load_profile(&self) -> Option<Profile> {
        let me = self.spotify_api("/me", &[]).await.ok()?;
        let profile = Profile {
            id: me["id"].as_str().unwrap_or_default().to_string(),
            name: me["display_name"]
                .as_str()
                .or_else(|| me["id"].as_str())
                .unwrap_or_default()
                .to_string(),
            country: me["country"].as_str().unwrap_or("US").to_string(),
            premium: me["product"].as_str() == Some("premium"),
        };
        *self.inner.spotify.profile.lock().ok()? = Some(profile.clone());
        Some(profile)
    }

    pub fn spotify_country(&self) -> String {
        self.inner
            .spotify
            .profile
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.country.clone()))
            .unwrap_or_else(|| "US".into())
    }

    pub async fn spotify_status(&self) -> Status {
        let has_tokens = self.inner.spotify.tokens.lock().is_ok_and(|t| t.is_some());
        let mut profile = self
            .inner
            .spotify
            .profile
            .lock()
            .ok()
            .and_then(|p| p.clone());
        if has_tokens && profile.is_none() {
            profile = self.spotify_load_profile().await;
        }
        Status {
            client_id: self.spotify_client_id(),
            connected: has_tokens && profile.is_some(),
            user: profile.as_ref().map(|p| p.name.clone()),
            premium: profile.is_some_and(|p| p.premium),
            can_edit: self.spotify_can_edit(),
        }
    }

    pub fn spotify_disconnect(&self) {
        if let Ok(mut t) = self.inner.spotify.tokens.lock() {
            *t = None;
        }
        if let Ok(mut p) = self.inner.spotify.profile.lock() {
            *p = None;
        }
        let _ = std::fs::remove_file(self.inner.dirs.file(AUTH_FILE));
    }

    /// Runs the browser sign-in. `open` is handed the authorize URL once the callback port is bound.
    pub async fn spotify_connect(&self, open: impl FnOnce(&str)) -> Result<Status> {
        if self.spotify_client_id().is_empty() {
            bail!("Add your Spotify client ID first");
        }
        let verifier = random_b64(48);
        let state = random_b64(12);
        let listener = crate::oauth::bind(PORT).await?;
        open(&authorize_url(
            &self.spotify_client_id(),
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
            callback.respond("Spotify sign-in was cancelled.").await;
            bail!("Sign-in cancelled");
        }
        let code = callback.params.get("code").cloned().unwrap_or_default();
        let result = self
            .spotify_token_request(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("code_verifier", &verifier),
            ])
            .await
            .and_then(|json| self.spotify_store(&json));
        match result {
            Ok(()) => {
                self.spotify_load_profile().await;
                callback
                    .respond("Connected. You can close this tab and go back to Mosaic.")
                    .await;
                Ok(self.spotify_status().await)
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
    fn authorize_url_carries_the_pkce_parameters_without_playback_scopes() {
        let url = authorize_url("cid", "chal", "st");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(
            url.contains("client_id=cid")
                && url.contains("code_challenge=chal")
                && url.contains("state=st")
        );
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8898%2Fcallback"));
        assert!(!url.contains("streaming") && !url.contains("modify-playback"));
        assert!(url.contains("playlist-read-private"));
    }

    #[test]
    fn random_values_are_urlsafe_and_distinct() {
        let (a, b) = (random_b64(48), random_b64(48));
        assert_ne!(a, b);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
