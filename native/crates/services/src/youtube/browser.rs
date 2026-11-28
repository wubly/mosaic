//! Google sign-in in the user's own browser. Google refuses sign-in inside embedded web views
//! ("this browser or app may not be secure"), and Linux has no system web view to embed
//! anyway, so Mosaic opens an installed browser on a profile of its own, waits for the user to
//! land on YouTube Music, and reads that profile's cookies over the browser's remote protocol:
//! the DevTools protocol for Chrome, Chromium, Edge, Brave and Vivaldi, WebDriver BiDi for
//! Firefox, Zen, LibreWolf and Floorp. The user's normal profile is never opened or read.
//!
//! Google also turns away browsers under remote control, and a page can tell (Firefox flags it
//! in more ways than `navigator.webdriver`). So the sign-in runs in two steps: the browser opens
//! plainly, with no remote protocol at all, and the user signs in and closes it; then the same
//! profile is opened again headless, with the protocol on, just to read the cookies.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::auth::SIGN_IN_URL;

const MUSIC_URL: &str = "https://music.youtube.com/";

const OVERALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(40);
const POLL: Duration = Duration::from_millis(800);
/// Read from the YouTube Music page once it has loaded, like the web player's own config.
const CONFIG_SCRIPT: &str = "JSON.stringify([(window.ytcfg&&ytcfg.get('VISITOR_DATA'))||'',\
     (window.ytcfg&&ytcfg.get('DATASYNC_ID'))||''])";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Chromium,
    Firefox,
}

/// An installed browser Mosaic can sign in with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Browser {
    pub name: &'static str,
    pub engine: Engine,
    /// The program to run and the arguments before the browser's own (`flatpak run <id>`).
    pub program: PathBuf,
    pub prefix: Vec<String>,
    /// A Flatpak can only write inside its own data folder, so its profile goes there.
    pub flatpak: Option<&'static str>,
}

/// What the sign-in gave: the cookies of the session and the page's account config.
#[derive(Clone, Debug, Default)]
pub struct Captured {
    pub cookies: String,
    pub visitor_data: Option<String>,
    pub data_sync_id: Option<String>,
}

const NATIVE: &[(&str, &str, Engine)] = &[
    ("google-chrome-stable", "Google Chrome", Engine::Chromium),
    ("google-chrome", "Google Chrome", Engine::Chromium),
    ("chromium", "Chromium", Engine::Chromium),
    ("chromium-browser", "Chromium", Engine::Chromium),
    ("brave-browser", "Brave", Engine::Chromium),
    ("brave", "Brave", Engine::Chromium),
    ("microsoft-edge-stable", "Microsoft Edge", Engine::Chromium),
    ("microsoft-edge", "Microsoft Edge", Engine::Chromium),
    ("vivaldi-stable", "Vivaldi", Engine::Chromium),
    ("vivaldi", "Vivaldi", Engine::Chromium),
    ("firefox", "Firefox", Engine::Firefox),
    ("firefox-bin", "Firefox", Engine::Firefox),
    ("zen-browser", "Zen", Engine::Firefox),
    ("zen", "Zen", Engine::Firefox),
    ("librewolf", "LibreWolf", Engine::Firefox),
    ("floorp", "Floorp", Engine::Firefox),
];

const FLATPAKS: &[(&str, &str, Engine)] = &[
    ("com.google.Chrome", "Google Chrome", Engine::Chromium),
    ("org.chromium.Chromium", "Chromium", Engine::Chromium),
    (
        "io.github.ungoogled_software.ungoogled_chromium",
        "Chromium",
        Engine::Chromium,
    ),
    ("com.brave.Browser", "Brave", Engine::Chromium),
    ("com.microsoft.Edge", "Microsoft Edge", Engine::Chromium),
    ("com.vivaldi.Vivaldi", "Vivaldi", Engine::Chromium),
    ("org.mozilla.firefox", "Firefox", Engine::Firefox),
    ("app.zen_browser.zen", "Zen", Engine::Firefox),
    (
        "io.gitlab.librewolf-community",
        "LibreWolf",
        Engine::Firefox,
    ),
    ("one.ablaze.floorp", "Floorp", Engine::Firefox),
];

fn on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn flatpak_installed(id: &str) -> bool {
    let system = Path::new("/var/lib/flatpak/app").join(id);
    let user = home().map(|h| h.join(".local/share/flatpak/app").join(id));
    system.is_dir() || user.is_some_and(|u| u.is_dir())
}

/// The browsers that can be used, the default browser first when it is one of them.
pub fn find() -> Vec<Browser> {
    let mut out: Vec<Browser> = Vec::new();
    let mut push = |b: Browser| {
        if !out
            .iter()
            .any(|o| o.program == b.program && o.prefix == b.prefix)
        {
            out.push(b);
        }
    };
    if cfg!(target_os = "windows") {
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        let apps: &[(&str, &str, Engine)] = &[
            (
                r"Microsoft\Edge\Application\msedge.exe",
                "Microsoft Edge",
                Engine::Chromium,
            ),
            (
                r"Google\Chrome\Application\chrome.exe",
                "Google Chrome",
                Engine::Chromium,
            ),
            (
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
                "Brave",
                Engine::Chromium,
            ),
            (
                r"Vivaldi\Application\vivaldi.exe",
                "Vivaldi",
                Engine::Chromium,
            ),
            (r"Mozilla Firefox\firefox.exe", "Firefox", Engine::Firefox),
            (r"Zen Browser\zen.exe", "Zen", Engine::Firefox),
        ];
        for (rel, name, engine) in apps {
            for root in &roots {
                let p = root.join(rel);
                if p.is_file() {
                    push(Browser {
                        name,
                        engine: *engine,
                        program: p,
                        prefix: Vec::new(),
                        flatpak: None,
                    });
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        let apps: &[(&str, &str, Engine)] = &[
            (
                "Google Chrome.app/Contents/MacOS/Google Chrome",
                "Google Chrome",
                Engine::Chromium,
            ),
            (
                "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "Microsoft Edge",
                Engine::Chromium,
            ),
            (
                "Brave Browser.app/Contents/MacOS/Brave Browser",
                "Brave",
                Engine::Chromium,
            ),
            (
                "Chromium.app/Contents/MacOS/Chromium",
                "Chromium",
                Engine::Chromium,
            ),
            (
                "Vivaldi.app/Contents/MacOS/Vivaldi",
                "Vivaldi",
                Engine::Chromium,
            ),
            (
                "Firefox.app/Contents/MacOS/firefox",
                "Firefox",
                Engine::Firefox,
            ),
            ("Zen.app/Contents/MacOS/zen", "Zen", Engine::Firefox),
        ];
        let roots = [
            Some(PathBuf::from("/Applications")),
            home().map(|h| h.join("Applications")),
        ];
        for (rel, name, engine) in apps {
            for root in roots.iter().flatten() {
                let p = root.join(rel);
                if p.is_file() {
                    push(Browser {
                        name,
                        engine: *engine,
                        program: p,
                        prefix: Vec::new(),
                        flatpak: None,
                    });
                }
            }
        }
    } else {
        for (program, name, engine) in NATIVE {
            if let Some(p) = on_path(program) {
                push(Browser {
                    name,
                    engine: *engine,
                    program: p,
                    prefix: Vec::new(),
                    flatpak: None,
                });
            }
        }
        if let Some(flatpak) = on_path("flatpak") {
            for (id, name, engine) in FLATPAKS {
                if flatpak_installed(id) {
                    push(Browser {
                        name,
                        engine: *engine,
                        program: flatpak.clone(),
                        prefix: vec!["run".into(), (*id).into()],
                        flatpak: Some(id),
                    });
                }
            }
        }
        if let Some(default) = default_browser() {
            if let Some(i) = out.iter().position(|b| matches_desktop_id(b, &default)) {
                let b = out.remove(i);
                out.insert(0, b);
            }
        }
    }
    out
}

/// `xdg-settings get default-web-browser`: a desktop file name like `app.zen_browser.zen.desktop`.
fn default_browser() -> Option<String> {
    let out = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    let id = String::from_utf8(out.stdout).ok()?.trim().to_lowercase();
    (!id.is_empty()).then_some(id)
}

fn matches_desktop_id(b: &Browser, desktop: &str) -> bool {
    let stem = desktop.trim_end_matches(".desktop");
    match b.flatpak {
        Some(id) => id.eq_ignore_ascii_case(stem),
        None => b
            .program
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| stem.contains(&f.to_lowercase()) || f.to_lowercase().contains(stem)),
    }
}

/// Where the sign-in profile lives. A Flatpak gets one inside its own data folder.
pub fn profile_dir(browser: &Browser, data_dir: &Path) -> PathBuf {
    let leaf = match browser.engine {
        Engine::Chromium => "youtube-signin-chromium",
        Engine::Firefox => "youtube-signin-firefox",
    };
    match (browser.flatpak, home()) {
        (Some(id), Some(h)) => h
            .join(".var/app")
            .join(id)
            .join("data")
            .join(format!("mosaic-{leaf}")),
        _ => data_dir.join(leaf),
    }
}

/// Firefox settings for the sign-in profile: no first-run pages or default-browser nagging.
pub fn firefox_prefs() -> String {
    [
        ("remote.active-protocols", "1"),
        ("browser.shell.checkDefaultBrowser", "false"),
        ("browser.aboutwelcome.enabled", "false"),
        ("browser.startup.homepage_override.mstone", "\"ignore\""),
        ("startup.homepage_welcome_url", "\"\""),
        ("startup.homepage_welcome_url.additional", "\"\""),
        (
            "datareporting.policy.dataSubmissionPolicyBypassNotification",
            "true",
        ),
        ("toolkit.telemetry.reportingpolicy.firstRun", "false"),
        ("browser.tabs.warnOnClose", "false"),
        ("zen.welcome-screen.seen", "true"),
        // Open tabs are saved every second (not 15), so Mosaic sees the sign-in finish quickly.
        ("browser.sessionstore.interval", "1000"),
    ]
    .iter()
    .map(|(k, v)| format!("user_pref(\"{k}\", {v});\n"))
    .collect()
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// How the browser is started.
enum Launch<'a> {
    /// A plain window for the user, with no remote protocol.
    Visible(&'a str),
    /// No window, the remote protocol on `port`: for reading the profile afterwards.
    Headless { port: u16, url: &'a str },
}

fn launch(browser: &Browser, profile: &Path, how: Launch<'_>) -> Result<Child> {
    std::fs::create_dir_all(profile)
        .with_context(|| format!("Could not create {}", profile.display()))?;
    let mut cmd = Command::new(&browser.program);
    cmd.args(&browser.prefix);
    let url = match how {
        Launch::Visible(url) | Launch::Headless { url, .. } => url,
    };
    match browser.engine {
        Engine::Chromium => {
            // The same cookie key with and without a window: a headless Chromium on Linux
            // otherwise can't read cookies the desktop keyring encrypted.
            cmd.arg(format!("--user-data-dir={}", profile.display()))
                .args([
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--password-store=basic",
                ]);
            if let Launch::Headless { port, .. } = how {
                cmd.arg("--headless=new")
                    .arg(format!("--remote-debugging-port={port}"));
            } else {
                cmd.arg("--new-window");
            }
            cmd.arg(url);
        }
        Engine::Firefox => {
            std::fs::write(profile.join("user.js"), firefox_prefs())?;
            cmd.arg("--profile").arg(profile).arg("--new-instance");
            if let Launch::Headless { port, .. } = how {
                cmd.args(["--headless", "--remote-debugging-port"])
                    .arg(port.to_string());
            }
            cmd.arg(url);
        }
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow!("Could not open {}: {e}", browser.name))
}

/// A JSON-RPC connection to a browser (CDP and BiDi frame their messages the same way).
struct Rpc {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next: u64,
}

impl Rpc {
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws.send(Message::Text(msg.to_string().into())).await?;
        let reply = tokio::time::timeout(Duration::from_secs(15), async {
            while let Some(frame) = self.ws.next().await {
                let Message::Text(text) = frame? else {
                    continue;
                };
                let v: Value = serde_json::from_str(&text)?;
                if v["id"].as_u64() == Some(id) {
                    return Ok(v);
                }
            }
            bail!("the browser closed the connection")
        })
        .await
        .map_err(|_| anyhow!("the browser stopped answering"))??;
        if let Some(e) = reply.get("error").filter(|e| !e.is_null()) {
            let message = e["message"]
                .as_str()
                .or_else(|| reply["message"].as_str())
                .or_else(|| e.as_str())
                .unwrap_or("error");
            bail!("{method}: {message}");
        }
        Ok(reply["result"].clone())
    }
}

fn is_music(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.eq_ignore_ascii_case("music.youtube.com"))
        })
        .unwrap_or(false)
}

/// `name=value; …` for the cookies of YouTube's domains, once the session cookie is among them.
pub fn cookie_header<'a>(
    cookies: impl Iterator<Item = (&'a str, &'a str, &'a str)>,
) -> Option<String> {
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for (name, value, domain) in cookies {
        let domain = domain.trim_start_matches('.');
        if (domain == "youtube.com" || domain.ends_with(".youtube.com"))
            && !pairs.iter().any(|(n, _)| *n == name)
        {
            pairs.push((name, value));
        }
    }
    pairs
        .iter()
        .any(|(n, _)| matches!(*n, "SAPISID" | "__Secure-3PAPISID"))
        .then(|| {
            pairs
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
}

fn page_config(result: &str) -> (Option<String>, Option<String>) {
    let list: Vec<String> = serde_json::from_str(result).unwrap_or_default();
    let get = |i: usize| list.get(i).filter(|s| !s.is_empty()).cloned();
    // `DATASYNC_ID` is `<id>||<id>`; the account to act for is its first part.
    (
        get(0),
        get(1).map(|d| d.split("||").next().unwrap_or(&d).to_string()),
    )
}

async fn connect(url: &str) -> Result<Rpc> {
    let (ws, _) = tokio_tungstenite::connect_async(url).await?;
    Ok(Rpc { ws, next: 0 })
}

/// Waits for the remote protocol to come up, giving up if the browser exits first.
async fn wait_for<T, F, Fut>(child: &mut Child, mut attempt: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let started = Instant::now();
    loop {
        match attempt().await {
            Ok(v) => return Ok(v),
            Err(e) if started.elapsed() > STARTUP_TIMEOUT => {
                bail!("The browser didn't open its sign-in window ({e})")
            }
            Err(_) => {}
        }
        if child.try_wait()?.is_some() {
            bail!("The browser closed right away. If it's already open, close it and try again.");
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// A BiDi session on the headless browser.
async fn firefox_session(child: &mut Child, port: u16) -> Result<Rpc> {
    let url = format!("ws://127.0.0.1:{port}/session");
    let mut rpc = wait_for(child, || connect(&url)).await?;
    rpc.call("session.new", json!({"capabilities": {}}), None)
        .await?;
    Ok(rpc)
}

/// The processes started on the sign-in `profile` (Linux; elsewhere the launched process is
/// the browser). The exact profile path belongs to Mosaic alone.
fn profile_pids(profile: &Path) -> Vec<String> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let needle = profile.as_os_str().as_encoded_bytes();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let me = std::process::id().to_string();
    entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|pid| pid.chars().all(|c| c.is_ascii_digit()) && *pid != me)
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmd| {
                cmd.split(|b| *b == 0).any(|arg| {
                    arg == needle
                        || arg.strip_prefix(b"--user-data-dir=".as_slice()) == Some(needle)
                })
            })
        })
        .collect()
}

/// Stops whatever still runs on the sign-in `profile` (a Flatpak outlives `flatpak run`) and
/// waits a little for it to be gone, so the profile isn't locked when it is opened next.
async fn stop_profile_processes(profile: &Path) {
    let pids = profile_pids(profile);
    if pids.is_empty() {
        return;
    }
    let _ = std::process::Command::new("kill").args(&pids).status();
    for _ in 0..30 {
        if profile_pids(profile).is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Decodes Firefox's `mozLz40` files (an LZ4 block behind a magic and the decoded size).
pub fn mozlz4(data: &[u8]) -> Option<Vec<u8>> {
    let rest = data.strip_prefix(b"mozLz40\0")?;
    let size = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let src = &rest[4..];
    let mut out = Vec::with_capacity(size);
    let mut i = 0;
    let length = |i: &mut usize, mut n: usize| -> Option<usize> {
        if n == 15 {
            loop {
                let b = *src.get(*i)?;
                *i += 1;
                n += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        Some(n)
    };
    while i < src.len() {
        let token = src[i];
        i += 1;
        let literals = length(&mut i, usize::from(token >> 4))?;
        out.extend_from_slice(src.get(i..i + literals)?);
        i += literals;
        if i >= src.len() {
            break;
        }
        let offset = usize::from(u16::from_le_bytes([*src.get(i)?, *src.get(i + 1)?]));
        i += 2;
        if offset == 0 || offset > out.len() {
            return None;
        }
        let len = length(&mut i, usize::from(token & 15))? + 4;
        let start = out.len() - offset;
        for k in 0..len {
            out.push(out[start + k]);
        }
        if out.len() > size {
            return None;
        }
    }
    (out.len() == size).then_some(out)
}

/// The page each tab of a Firefox session file is on.
pub fn firefox_tab_urls(session: &Value) -> Vec<String> {
    session["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|w| w["tabs"].as_array().into_iter().flatten())
        .filter_map(|tab| {
            let entries = tab["entries"].as_array()?;
            let at = tab["index"].as_u64().map_or(entries.len(), |i| i as usize);
            entries.get(at.checked_sub(1)?)?["url"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

/// Whether the sign-in window shows YouTube Music, read from what the browser saves of its
/// open tabs (no remote protocol involved).
fn reached_music(browser: &Browser, profile: &Path) -> bool {
    match browser.engine {
        Engine::Firefox => std::fs::read(profile.join("sessionstore-backups/recovery.jsonlz4"))
            .ok()
            .and_then(|d| mozlz4(&d))
            .and_then(|json| serde_json::from_slice::<Value>(&json).ok())
            .is_some_and(|session| firefox_tab_urls(&session).iter().any(|u| is_music(u))),
        Engine::Chromium => {
            // Session files are binary, but URLs sit in them as plain text. Before landing,
            // YouTube Music only appears percent-encoded inside the sign-in URL.
            let newest = std::fs::read_dir(profile.join("Default/Sessions"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("Session_"))
                .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
            newest
                .and_then(|e| std::fs::read(e.path()).ok())
                .is_some_and(|bytes| {
                    bytes
                        .windows(MUSIC_URL.len())
                        .any(|w| w == MUSIC_URL.as_bytes())
                })
        }
    }
}

/// Waits until the sign-in is done: the window reached YouTube Music (then Mosaic closes it),
/// or the user closed it. `false` on timeout.
async fn wait_done(child: &mut Child, browser: &Browser, profile: &Path) -> bool {
    let started = Instant::now();
    // Session files from an earlier sign-in would read as done straight away.
    let _ = std::fs::remove_file(profile.join("sessionstore-backups/recovery.jsonlz4"));
    let _ = std::fs::remove_dir_all(profile.join("Default/Sessions"));
    loop {
        if started.elapsed() > Duration::from_secs(3) && reached_music(browser, profile) {
            // A moment for the browser to store the session cookies, then the window goes.
            tokio::time::sleep(Duration::from_millis(1500)).await;
            stop_profile_processes(profile).await;
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            let _ = child.start_kill();
            return true;
        }
        let exited = child.try_wait().ok().flatten().is_some();
        // A launcher script may exit before the browser it starts shows up.
        let settled = started.elapsed() > Duration::from_secs(3);
        if exited && settled && profile_pids(profile).is_empty() {
            return true;
        }
        if started.elapsed() > OVERALL_TIMEOUT {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
}

/// Opens `profile` headless with the remote protocol on.
async fn open_headless(
    http: &reqwest::Client,
    browser: &Browser,
    profile: &Path,
    url: &str,
) -> Result<(Child, Rpc)> {
    let port = free_port()?;
    let mut child = launch(browser, profile, Launch::Headless { port, url })?;
    let rpc = match browser.engine {
        Engine::Chromium => {
            let version = format!("http://127.0.0.1:{port}/json/version");
            let ws = wait_for(&mut child, || async {
                let v: Value = http.get(&version).send().await?.json().await?;
                v["webSocketDebuggerUrl"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("no debugger address"))
            })
            .await?;
            connect(&ws).await?
        }
        Engine::Firefox => firefox_session(&mut child, port).await?,
    };
    Ok((child, rpc))
}

/// Closes the headless browser and anything left on the profile.
async fn shut_down(mut child: Child, mut rpc: Rpc, engine: Engine, profile: &Path) {
    close_browser(&mut rpc, engine).await;
    if tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .is_err()
    {
        stop_profile_processes(profile).await;
        let _ = child.start_kill();
    }
}

/// Closes the sign-in browser. Firefox can answer `browser.close` and keep running (seen with
/// the Zen Flatpak), so its windows are closed too, which quits it.
async fn close_browser(rpc: &mut Rpc, engine: Engine) {
    match engine {
        Engine::Chromium => {
            let _ = rpc.call("Browser.close", json!({}), None).await;
        }
        Engine::Firefox => {
            if let Ok(tree) = rpc
                .call("browsingContext.getTree", json!({"maxDepth": 0}), None)
                .await
            {
                for c in tree["contexts"].as_array().into_iter().flatten() {
                    let _ = rpc
                        .call(
                            "browsingContext.close",
                            json!({"context": c["context"], "promptUnload": false}),
                            None,
                        )
                        .await;
                }
            }
            let _ = rpc.call("browser.close", json!({}), None).await;
        }
    }
}

/// One look at the Chromium window: the cookies, once the user is on YouTube Music.
async fn chromium_check(rpc: &mut Rpc) -> Result<Option<Captured>> {
    let targets = rpc.call("Target.getTargets", json!({}), None).await?;
    let page = targets["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["type"] == "page" && t["url"].as_str().is_some_and(is_music))
        .cloned();
    let Some(page) = page else { return Ok(None) };
    let all = rpc.call("Storage.getCookies", json!({}), None).await?;
    let list = all["cookies"].as_array().cloned().unwrap_or_default();
    let Some(cookies) = cookie_header(list.iter().map(|c| {
        (
            c["name"].as_str().unwrap_or_default(),
            c["value"].as_str().unwrap_or_default(),
            c["domain"].as_str().unwrap_or_default(),
        )
    })) else {
        return Ok(None);
    };
    let mut captured = Captured {
        cookies,
        ..Captured::default()
    };
    let attached = rpc
        .call(
            "Target.attachToTarget",
            json!({"targetId": page["targetId"], "flatten": true}),
            None,
        )
        .await;
    if let Some(session) = attached
        .ok()
        .and_then(|a| a["sessionId"].as_str().map(str::to_string))
    {
        if let Ok(r) = rpc
            .call(
                "Runtime.evaluate",
                json!({"expression": CONFIG_SCRIPT, "returnByValue": true}),
                Some(&session),
            )
            .await
        {
            (captured.visitor_data, captured.data_sync_id) =
                page_config(r["result"]["value"].as_str().unwrap_or_default());
        }
    }
    Ok(Some(captured))
}

/// One look at the Firefox window over WebDriver BiDi.
async fn firefox_check(rpc: &mut Rpc) -> Result<Option<Captured>> {
    let tree = rpc.call("browsingContext.getTree", json!({}), None).await?;
    let context = tree["contexts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["url"].as_str().is_some_and(is_music))
        .and_then(|c| c["context"].as_str().map(str::to_string));
    let Some(context) = context else {
        return Ok(None);
    };
    let all = rpc.call("storage.getCookies", json!({}), None).await?;
    let list = all["cookies"].as_array().cloned().unwrap_or_default();
    let Some(cookies) = cookie_header(list.iter().map(|c| {
        (
            c["name"].as_str().unwrap_or_default(),
            c["value"]["value"].as_str().unwrap_or_default(),
            c["domain"].as_str().unwrap_or_default(),
        )
    })) else {
        return Ok(None);
    };
    let mut captured = Captured {
        cookies,
        ..Captured::default()
    };
    if let Ok(r) = rpc
        .call(
            "script.evaluate",
            json!({"expression": CONFIG_SCRIPT, "target": {"context": context},
                   "awaitPromise": false, "resultOwnership": "none"}),
            None,
        )
        .await
    {
        (captured.visitor_data, captured.data_sync_id) =
            page_config(r["result"]["value"].as_str().unwrap_or_default());
    }
    Ok(Some(captured))
}

/// How long the headless read waits for YouTube Music to load and show the session.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs the sign-in with `browser`: a plain window the user signs in with and then closes,
/// then a headless read of the profile. `Ok(None)` when they closed it without signing in.
pub async fn sign_in(
    http: &reqwest::Client,
    browser: &Browser,
    data_dir: &Path,
) -> Result<Option<Captured>> {
    let profile = profile_dir(browser, data_dir);
    // A window left over from an earlier attempt would hold the profile.
    stop_profile_processes(&profile).await;
    let mut window = launch(browser, &profile, Launch::Visible(SIGN_IN_URL))?;
    if !wait_done(&mut window, browser, &profile).await {
        stop_profile_processes(&profile).await;
        let _ = window.start_kill();
        bail!("Sign-in timed out");
    }
    read_session(http, browser, &profile).await
}

/// Reads the signed-in session from the sign-in profile, headless. `Ok(None)` when the profile
/// isn't signed in to YouTube Music.
async fn read_session(
    http: &reqwest::Client,
    browser: &Browser,
    profile: &Path,
) -> Result<Option<Captured>> {
    let (child, mut rpc) = open_headless(http, browser, profile, MUSIC_URL).await?;
    let started = Instant::now();
    let mut last_error = None;
    let found = loop {
        let check = match browser.engine {
            Engine::Chromium => chromium_check(&mut rpc).await,
            Engine::Firefox => firefox_check(&mut rpc).await,
        };
        match check {
            Ok(Some(c)) => break Some(c),
            Ok(None) => {}
            Err(e) => last_error = Some(e),
        }
        if started.elapsed() > READ_TIMEOUT {
            if let Some(e) = last_error {
                tracing::warn!("reading the sign-in profile: {e}");
            }
            break None;
        }
        tokio::time::sleep(POLL).await;
    };
    shut_down(child, rpc, browser.engine, profile).await;
    Ok(found)
}

/// For checking a browser works (`--debug-cmd yt-browser-probe`): opens the sign-in profile
/// headless on YouTube Music, reports what the page sees, and closes it. Needs no account.
pub async fn probe(http: &reqwest::Client, browser: &Browser, data_dir: &Path) -> Result<String> {
    let profile = profile_dir(browser, data_dir);
    let (child, mut rpc) = open_headless(http, browser, &profile, MUSIC_URL).await?;
    tokio::time::sleep(Duration::from_secs(6)).await;
    let script = "JSON.stringify([location.href, String(navigator.webdriver)])";
    let (page, cookies) = match browser.engine {
        Engine::Firefox => {
            let tree = rpc.call("browsingContext.getTree", json!({}), None).await?;
            let context = tree["contexts"][0]["context"].clone();
            let page = rpc
                .call(
                    "script.evaluate",
                    json!({"expression": script, "target": {"context": context},
                           "awaitPromise": false, "resultOwnership": "none"}),
                    None,
                )
                .await?;
            let cookies = rpc.call("storage.getCookies", json!({}), None).await?;
            (page["result"]["value"].clone(), cookies)
        }
        Engine::Chromium => {
            let targets = rpc.call("Target.getTargets", json!({}), None).await?;
            let page = targets["targetInfos"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|t| t["type"] == "page")
                .cloned()
                .unwrap_or_default();
            let a = rpc
                .call(
                    "Target.attachToTarget",
                    json!({"targetId": page["targetId"], "flatten": true}),
                    None,
                )
                .await?;
            let session = a["sessionId"].as_str().unwrap_or_default().to_string();
            let r = rpc
                .call(
                    "Runtime.evaluate",
                    json!({"expression": script, "returnByValue": true}),
                    Some(&session),
                )
                .await?;
            let cookies = rpc.call("Storage.getCookies", json!({}), None).await?;
            (r["result"]["value"].clone(), cookies)
        }
    };
    let n = cookies["cookies"].as_array().map_or(0, Vec::len);
    shut_down(child, rpc, browser.engine, &profile).await;
    let left = profile_pids(&profile).len();
    Ok(format!("page {page}\ncookies {n}\nprocesses left: {left}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_youtube_cookies_make_the_header_and_only_when_signed_in() {
        let jar = [
            ("SID", "1", ".youtube.com"),
            ("SAPISID", "abc", ".youtube.com"),
            ("SAPISID", "other", ".google.com"),
            ("LOGIN_INFO", "x", "music.youtube.com"),
            ("NID", "n", ".google.com"),
        ];
        assert_eq!(
            cookie_header(jar.iter().copied()).as_deref(),
            Some("SID=1; SAPISID=abc; LOGIN_INFO=x")
        );
        let signed_out = [
            ("YSC", "y", ".youtube.com"),
            ("SAPISID", "g", ".google.com"),
        ];
        assert_eq!(cookie_header(signed_out.iter().copied()), None);
    }

    #[test]
    fn the_page_config_gives_the_visitor_and_the_account() {
        let (visitor, account) = page_config(r#"["CgtWSVNJVE9S","112233||445566"]"#);
        assert_eq!(visitor.as_deref(), Some("CgtWSVNJVE9S"));
        assert_eq!(account.as_deref(), Some("112233"));
        assert_eq!(page_config(r#"["",""]"#), (None, None));
        assert_eq!(page_config("garbage"), (None, None));
    }

    #[test]
    fn firefox_session_files_decode_to_their_tabs() {
        // "mozLz40\0", size 37, then one LZ4 sequence of 37 literals (token 0xF0, 37-15=22).
        let json = br#"{"windows":[{"tabs":[{"entries":[]}]}]}"#;
        assert_eq!(json.len(), 39);
        let mut file = b"mozLz40\0".to_vec();
        file.extend_from_slice(&(json.len() as u32).to_le_bytes());
        file.push(0xF0);
        file.push((json.len() - 15) as u8);
        file.extend_from_slice(json);
        assert_eq!(mozlz4(&file).as_deref(), Some(&json[..]));
        // A back-reference: "abcabcabc" as 3 literals then a 6-byte match at offset 3.
        let mut rep = b"mozLz40\0".to_vec();
        rep.extend_from_slice(&9u32.to_le_bytes());
        rep.extend_from_slice(&[0x32, b'a', b'b', b'c', 3, 0]);
        assert_eq!(mozlz4(&rep).as_deref(), Some(&b"abcabcabc"[..]));
        assert!(mozlz4(b"not a session").is_none());

        let session = json!({"windows": [{"tabs": [
            {"index": 2, "entries": [{"url": "https://accounts.google.com/"}, {"url": "https://music.youtube.com/"}]},
            {"entries": [{"url": "about:blank"}]},
        ]}]});
        assert_eq!(
            firefox_tab_urls(&session),
            ["https://music.youtube.com/", "about:blank"]
        );
    }

    #[test]
    fn only_music_youtube_counts_as_arrived() {
        assert!(is_music("https://music.youtube.com/"));
        assert!(is_music("https://MUSIC.youtube.com/library"));
        assert!(!is_music(
            "https://accounts.google.com/?continue=https://music.youtube.com"
        ));
        assert!(!is_music("https://music.youtube.com.evil.example/"));
    }

    #[test]
    fn firefox_profiles_skip_first_run_pages() {
        let prefs = firefox_prefs();
        assert!(prefs.contains("user_pref(\"browser.aboutwelcome.enabled\", false);"));
        assert!(prefs
            .lines()
            .all(|l| l.starts_with("user_pref(") && l.ends_with(");")));
    }

    #[test]
    fn flatpak_profiles_live_in_the_apps_own_folder() {
        let b = Browser {
            name: "Zen",
            engine: Engine::Firefox,
            program: "/usr/bin/flatpak".into(),
            prefix: vec!["run".into(), "app.zen_browser.zen".into()],
            flatpak: Some("app.zen_browser.zen"),
        };
        let dir = profile_dir(&b, Path::new("/data"));
        assert!(dir.ends_with(".var/app/app.zen_browser.zen/data/mosaic-youtube-signin-firefox"));
        let native = Browser {
            flatpak: None,
            prefix: Vec::new(),
            ..b
        };
        assert_eq!(
            profile_dir(&native, Path::new("/data")),
            Path::new("/data/youtube-signin-firefox")
        );
    }
}
