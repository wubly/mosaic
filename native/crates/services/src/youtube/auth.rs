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
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::Services;
use mosaic_core::storage::{read_json, write_json_atomic};

pub const PORT: u16 = 8899;
pub const REDIRECT: &str = "http://127.0.0.1:8899/callback";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";
const AUTH_FILE: &str = "youtube-auth.json";
const SESSION_FILE: &str = "youtube-session.json";
/// Where the sign-in web view starts: Google's login, then on to YouTube Music. The caller
/// watches for the view to land on `music.youtube.com` and then reads that site's cookies
/// (including the HttpOnly ones, so through the web view's cookie manager, not `document.cookie`).
pub const SIGN_IN_URL: &str = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin%3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F";
pub const MUSIC_ORIGIN: &str = "https://music.youtube.com";

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

/// The YouTube Music account a cookie sign-in belongs to.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub name: String,
    /// The email address when YouTube shows one, else the channel handle (`@name`).
    pub email_or_handle: String,
    pub photo: String,
}

/// A cookie sign-in as kept on disk.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Session {
    pub cookies: String,
    pub account: Account,
    /// The web player's visitor id (`VISITOR_DATA`), sent back with each request.
    #[serde(default)]
    pub visitor_data: Option<String>,
    /// Which of the Google account's channels to act as (`DATASYNC_ID`), so a brand account
    /// sees its own library.
    #[serde(default)]
    pub data_sync_id: Option<String>,
}

#[derive(Default)]
pub struct State {
    session: Mutex<Option<Session>>,
    tokens: Mutex<Option<Tokens>>,
    channel: Mutex<Option<String>>,
    /// The anonymous visitor id InnerTube asks `/player` callers to present (see `innertube`).
    pub(super) visitor: Mutex<Option<String>>,
}

impl State {
    pub fn load(dirs: &mosaic_core::storage::Dirs) -> State {
        let tokens: Option<Tokens> = read_json(&dirs.file(AUTH_FILE));
        let session: Option<Session> = read_json(&dirs.file(SESSION_FILE));
        State {
            session: Mutex::new(session.filter(|s| sapisid_of(&s.cookies).is_some())),
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

/// The value of cookie `name` in a `Cookie:` header string.
fn cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k.trim() == name && !v.trim().is_empty()).then(|| v.trim())
    })
}

/// The cookie the request signature is made from. `SAPISID` is what the web player uses;
/// `__Secure-3PAPISID` carries the same value and is what some cookie exports contain.
fn sapisid_of(cookies: &str) -> Option<&str> {
    ["SAPISID", "__Secure-3PAPISID", "__Secure-1PAPISID"]
        .iter()
        .find_map(|n| cookie_value(cookies, n))
}

/// `SAPISIDHASH <ts>_<sha1("<ts> <sapisid> <origin>")>`, the `Authorization` value Google's
/// web clients send together with the cookies.
pub fn sapisid_hash(timestamp: u64, sapisid: &str, origin: &str) -> String {
    let digest: String = Sha1::digest(format!("{timestamp} {sapisid} {origin}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("SAPISIDHASH {timestamp}_{digest}")
}

/// Tidies pasted or captured cookies: drops a `Cookie:` prefix and line breaks, and adds the
/// consent cookie that keeps EU requests from being bounced to a consent page.
fn normalize_cookies(raw: &str) -> String {
    let flat = raw
        .trim()
        .trim_start_matches("Cookie:")
        .trim_start_matches("cookie:")
        .replace(['\r', '\n'], ";");
    let parts: Vec<&str> = flat
        .split(';')
        .map(str::trim)
        .filter(|p| p.contains('='))
        .collect();
    let mut out = parts.join("; ");
    if cookie_value(&out, "SOCS").is_none() {
        out.push_str("; SOCS=CAI");
    }
    out
}

/// Account details out of an `account/account_menu` response; `None` when the cookies are not
/// signed in (the response then has no active-account header).
pub fn parse_account(response: &Value) -> Option<Account> {
    fn find<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
        match node {
            Value::Object(map) => map
                .get(key)
                .or_else(|| map.values().find_map(|v| find(v, key))),
            Value::Array(list) => list.iter().find_map(|v| find(v, key)),
            _ => None,
        }
    }
    fn text(node: &Value) -> String {
        node["runs"]
            .as_array()
            .map(|r| r.iter().filter_map(|r| r["text"].as_str()).collect())
            .or_else(|| node["simpleText"].as_str().map(str::to_string))
            .unwrap_or_default()
    }
    let header = find(response, "activeAccountHeaderRenderer")?;
    let name = text(&header["accountName"]);
    if name.is_empty() {
        return None;
    }
    let email = text(&header["email"]);
    let handle = text(&header["channelHandle"]);
    Some(Account {
        name,
        email_or_handle: if email.is_empty() { handle } else { email },
        photo: header["accountPhoto"]["thumbnails"]
            .as_array()
            .and_then(|t| t.last())
            .and_then(|t| t["url"].as_str())
            .unwrap_or_default()
            .to_string(),
    })
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
    /// The signed-in YouTube Music account (cookie sign-in), if any. No network.
    pub fn yt_account(&self) -> Option<Account> {
        self.inner
            .youtube
            .session
            .lock()
            .ok()?
            .as_ref()
            .map(|s| s.account.clone())
    }

    /// Signs in with the cookies a Google sign-in left behind (see [`SIGN_IN_URL`]). The cookies
    /// are checked against YouTube Music first and only kept when they are a live session.
    pub async fn yt_cookie_login(&self, cookies: &str) -> Result<Account> {
        self.yt_session_login(cookies, None, None).await
    }

    /// `yt_cookie_login` with the page config the sign-in browser read.
    pub async fn yt_session_login(
        &self,
        cookies: &str,
        visitor_data: Option<String>,
        data_sync_id: Option<String>,
    ) -> Result<Account> {
        let cookies = normalize_cookies(cookies);
        if sapisid_of(&cookies).is_none() {
            bail!("Those cookies have no Google sign-in (SAPISID is missing)");
        }
        let response = self
            .yt_music_post("account/account_menu", json!({}), &cookies)
            .await?;
        let account = parse_account(&response)
            .ok_or_else(|| anyhow!("YouTube Music doesn't see a signed-in account"))?;
        let session = Session {
            cookies,
            account: account.clone(),
            visitor_data,
            data_sync_id,
        };
        let _ = write_json_atomic(&self.inner.dirs.file(SESSION_FILE), &session);
        *self
            .inner
            .youtube
            .session
            .lock()
            .map_err(|_| anyhow!("poisoned"))? = Some(session);
        Ok(account)
    }

    /// Forgets the cookie sign-in.
    pub fn yt_sign_out(&self) {
        if let Ok(mut s) = self.inner.youtube.session.lock() {
            *s = None;
        }
        let _ = std::fs::remove_file(self.inner.dirs.file(SESSION_FILE));
    }

    /// The signed-in session's (visitor id, account id), as the sign-in found them.
    pub(crate) fn yt_session_ids(&self) -> (Option<String>, Option<String>) {
        self.inner
            .youtube
            .session
            .lock()
            .ok()
            .and_then(|s| {
                s.as_ref()
                    .map(|s| (s.visitor_data.clone(), s.data_sync_id.clone()))
            })
            .unwrap_or_default()
    }

    pub(crate) fn yt_cookies(&self) -> Option<String> {
        self.inner
            .youtube
            .session
            .lock()
            .ok()?
            .as_ref()
            .map(|s| s.cookies.clone())
    }

    /// The headers that authenticate a request to YouTube Music as the signed-in user.
    pub(crate) fn yt_auth_headers(cookies: &str) -> Result<Vec<(&'static str, String)>> {
        let sapisid = sapisid_of(cookies).ok_or_else(|| anyhow!("Not signed in to YouTube"))?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Ok(vec![
            ("Authorization", sapisid_hash(ts, sapisid, MUSIC_ORIGIN)),
            ("Cookie", cookies.to_string()),
            ("X-Goog-AuthUser", "0".into()),
            ("X-Origin", MUSIC_ORIGIN.into()),
        ])
    }

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
    fn sapisid_hash_matches_a_known_vector() {
        // sha1("1700000000 abc123 https://music.youtube.com"), checked with sha1sum.
        assert_eq!(
            sapisid_hash(1_700_000_000, "abc123", MUSIC_ORIGIN),
            "SAPISIDHASH 1700000000_597a8411c1c01f9c14cea84b6ec377b76d93be6e"
        );
    }

    #[test]
    fn cookies_are_normalized_and_the_sapisid_found() {
        let c =
            normalize_cookies("Cookie: SID=1; SAPISID=abc/def;\r\n__Secure-3PAPISID=zzz ; junk");
        assert_eq!(c, "SID=1; SAPISID=abc/def; __Secure-3PAPISID=zzz; SOCS=CAI");
        assert_eq!(sapisid_of(&c), Some("abc/def"));
        let only3p = normalize_cookies("__Secure-3PAPISID=zzz; SOCS=X");
        assert_eq!(sapisid_of(&only3p), Some("zzz"));
        assert!(only3p.ends_with("SOCS=X"));
        assert_eq!(sapisid_of("SID=1"), None);
        assert_eq!(sapisid_of("SAPISID="), None);
    }

    #[test]
    fn auth_headers_carry_the_signature_and_cookies() {
        let h = Services::yt_auth_headers("SID=1; SAPISID=abc").unwrap();
        let get = |n: &str| h.iter().find(|(k, _)| *k == n).unwrap().1.clone();
        assert!(get("Authorization").starts_with("SAPISIDHASH "));
        assert_eq!(get("Cookie"), "SID=1; SAPISID=abc");
        assert_eq!(get("X-Origin"), "https://music.youtube.com");
        assert!(Services::yt_auth_headers("SID=1").is_err());
    }

    #[test]
    fn account_menu_parses_name_handle_and_photo() {
        let fixture = include_str!("../../tests/fixtures/yt_library_account_menu.json");
        let account = parse_account(&serde_json::from_str(fixture).unwrap()).unwrap();
        assert_eq!(account.name, "Ada Lovelace");
        assert_eq!(account.email_or_handle, "@adalovelace");
        assert!(account.photo.ends_with("=s88-c-k-c0x00ffffff-no-rj"));
        assert_eq!(parse_account(&json!({"actions": []})), None);
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
