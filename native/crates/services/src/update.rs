//! Checking GitHub for a newer release, and (opt-in, driven by the app) downloading and
//! installing it.
//!
//! Flow from the app:
//! 1. `check()` fills `UpdateState`; `state.asset` is the release file for this platform.
//! 2. If `state.available`, `state.asset.is_some()` and `can_self_update()`, offer "Install":
//!    `download(&http, &asset, &dir, |done, total| ..)` then `install_and_restart(&path)`, and
//!    on `Ok(())` exit the app right away (the installer/new binary takes over).
//!    On `Err(InstallError::OpenReleasePage)` (or when `can_self_update()` is false) open
//!    `state.url` in the browser instead.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use futures::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub const REPO: &str = "wubly/mosaic";
pub const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Default)]
pub struct UpdateState {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub url: Option<String>,
    pub notes: String,
    pub checked_at: u64,
    pub error: Option<String>,
    /// This platform's file in the latest release, when there is one.
    pub asset: Option<Asset>,
}

/// A downloadable release file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// The release's `SHA256SUMS.txt`, when it has one.
    pub checksums_url: Option<String>,
}

/// How this copy of Mosaic was installed, which decides the release file to fetch and whether
/// it can be installed from inside the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Windows, run from the per-user installer's directory (it has an `uninstall.exe`).
    WindowsInstalled,
    /// Windows, the bare executable.
    WindowsPortable,
    /// Linux x86_64 AppImage (`APPIMAGE` is set).
    LinuxAppImage,
    MacOs,
    /// deb, Flatpak, Gentoo, tarball, other architectures: update through the package manager.
    Other,
}

impl Target {
    pub fn current() -> Self {
        if cfg!(windows) {
            let installed = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("uninstall.exe").is_file()))
                .unwrap_or(false);
            if installed {
                Self::WindowsInstalled
            } else {
                Self::WindowsPortable
            }
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux")
            && std::env::consts::ARCH == "x86_64"
            && std::env::var_os("APPIMAGE").is_some_and(|v| !v.is_empty())
        {
            Self::LinuxAppImage
        } else {
            Self::Other
        }
    }
}

/// True when `install_and_restart` can update this copy in place (otherwise open the release
/// page).
pub fn can_self_update() -> bool {
    matches!(
        Target::current(),
        Target::WindowsInstalled | Target::LinuxAppImage
    )
}

fn parts(v: &str) -> Vec<u64> {
    v.trim_start_matches(['v', 'V'])
        .split('.')
        .map(leading_int)
        .collect()
}

fn leading_int(s: &str) -> u64 {
    let digits: String = s.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// True when version `a` is newer than `b`.
pub fn newer(a: &str, b: &str) -> bool {
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p > q;
        }
    }
    false
}

#[derive(Deserialize)]
struct Release {
    tag_name: Option<String>,
    html_url: Option<String>,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

const CHECKSUMS_NAME: &str = "sha256sums.txt";

/// The release file for `target`, with the checksum file's URL when the release has one.
/// Names come from release.yml: `Mosaic-<v>-windows-x64-setup.exe`,
/// `Mosaic-<v>-windows-x64-portable.exe`, `Mosaic-<v>-x86_64.AppImage`,
/// `Mosaic-<v>-macos-universal.dmg`.
fn pick_asset(assets: &[ReleaseAsset], target: Target) -> Option<Asset> {
    let suffix = match target {
        Target::WindowsInstalled => "-setup.exe",
        Target::WindowsPortable => "-portable.exe",
        Target::LinuxAppImage => ".appimage",
        Target::MacOs => ".dmg",
        Target::Other => return None,
    };
    let found = assets
        .iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with(suffix))?;
    let checksums_url = assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(CHECKSUMS_NAME))
        .map(|a| a.browser_download_url.clone());
    Some(Asset {
        name: found.name.clone(),
        url: found.browser_download_url.clone(),
        size: found.size,
        checksums_url,
    })
}

/// The hex digest listed for `name` in a `sha256sum`-style file (`<hash>  <name>`, the name
/// optionally prefixed with `*` for binary mode).
fn checksum_for(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        let file = file.strip_prefix("./").unwrap_or(file);
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

pub async fn check(http: &reqwest::Client, previous: &UpdateState, current: &str) -> UpdateState {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    match fetch(http).await {
        Ok(release) => {
            let latest = release
                .tag_name
                .unwrap_or_default()
                .trim_start_matches(['v', 'V'])
                .to_string();
            UpdateState {
                current: current.into(),
                available: !latest.is_empty() && newer(&latest, current),
                asset: pick_asset(&release.assets, Target::current()),
                latest: Some(latest),
                url: release.html_url,
                notes: release.body.unwrap_or_default(),
                checked_at: now,
                error: None,
            }
        }
        Err(e) => UpdateState {
            current: current.into(),
            checked_at: now,
            error: Some(e.to_string()),
            ..previous.clone()
        },
    }
}

async fn fetch(http: &reqwest::Client) -> anyhow::Result<Release> {
    let res = http
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .timeout(Duration::from_secs(10))
        .header("User-Agent", "Mosaic")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        anyhow::bail!(if status.as_u16() == 404 {
            "No releases yet".to_string()
        } else {
            format!("GitHub said {}", status.as_u16())
        });
    }
    Ok(res.json().await?)
}

/// Download `asset` into `dir` and return the finished file. It streams to `<name>.part`,
/// checks the size against the release metadata, and checks the SHA-256 against the release's
/// `SHA256SUMS.txt` when there is one. `progress(done, total)` is called as bytes arrive.
pub async fn download(
    http: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    progress: impl Fn(u64, u64),
) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        asset.url.starts_with("https://"),
        "refusing a non-HTTPS download"
    );
    let name = Path::new(&asset.name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| *n == asset.name && !n.is_empty())
        .context("bad release file name")?;
    // Fetch the expected hash first so a bad release fails before the big download.
    let expected = match &asset.checksums_url {
        Some(url) => {
            let text = http
                .get(url)
                .timeout(Duration::from_secs(20))
                .header("User-Agent", "Mosaic")
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
                .context("reading SHA256SUMS.txt")?;
            Some(
                checksum_for(&text, name)
                    .with_context(|| format!("{name} is not listed in SHA256SUMS.txt"))?,
            )
        }
        None => None,
    };

    tokio::fs::create_dir_all(dir).await?;
    let part = dir.join(format!("{name}.part"));
    let dest = dir.join(name);
    let (got, hash) = match stream_to(http, asset, &part, progress).await {
        Ok(v) => v,
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(e);
        }
    };
    let verdict = (|| {
        if asset.size > 0 {
            anyhow::ensure!(
                got == asset.size,
                "download is {got} bytes, expected {}",
                asset.size
            );
        }
        if let Some(want) = &expected {
            anyhow::ensure!(&hash == want, "checksum mismatch for {name}");
        }
        Ok(())
    })();
    if let Err(e) = verdict {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    let _ = tokio::fs::remove_file(&dest).await;
    tokio::fs::rename(&part, &dest).await?;
    Ok(dest)
}

/// Streams the asset to `part`; returns the byte count and the lowercase SHA-256 hex.
async fn stream_to(
    http: &reqwest::Client,
    asset: &Asset,
    part: &Path,
    progress: impl Fn(u64, u64),
) -> anyhow::Result<(u64, String)> {
    let res = http
        .get(&asset.url)
        .timeout(Duration::from_secs(60 * 30))
        .header("User-Agent", "Mosaic")
        .send()
        .await?
        .error_for_status()?;
    let total = res.content_length().unwrap_or(asset.size);
    let mut file = tokio::fs::File::create(part).await?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut reported = 0u64;
    let mut stream = res.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), stream.next())
        .await
        .context("download stalled")?
    {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if done - reported >= 64 * 1024 {
            reported = done;
            progress(done, total);
        }
    }
    file.flush().await?;
    progress(done, total.max(done));
    let hash = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((done, hash))
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// This copy can't be updated in place (deb, Flatpak, Gentoo, macOS dmg, portable exe):
    /// send the user to the release page.
    #[error("open the release page to update")]
    OpenReleasePage,
    #[error("{0:#}")]
    Failed(#[from] anyhow::Error),
}

/// Install the downloaded file and start the new version. On `Ok(())` the caller must exit
/// right away: on Windows the silent installer is waiting for this process to end.
///
/// * Windows, installed copy: runs the setup detached with `/S /R /WAIT=<our pid> /D=<our dir>`.
///   `/S` is silent (per-user, no UAC), `/WAIT` makes the installer wait up to 20 s for us to
///   exit on our own instead of killing us, and `/R` makes the installer itself launch Mosaic
///   once it has finished, so no helper process or timing guess is needed on our side. The
///   installer also refreshes the Start menu shortcut and only touches the desktop one if it
///   already exists.
/// * Linux AppImage: writes the new file next to the running one, renames it over it (atomic;
///   the running image keeps its old inode) and starts it.
/// * Everything else: `Err(InstallError::OpenReleasePage)`.
pub fn install_and_restart(path: &Path) -> Result<(), InstallError> {
    match Target::current() {
        #[cfg(windows)]
        Target::WindowsInstalled => Ok(install_windows(path)?),
        #[cfg(unix)]
        Target::LinuxAppImage => Ok(install_appimage(path)?),
        _ => {
            let _ = path;
            Err(InstallError::OpenReleasePage)
        }
    }
}

#[cfg(windows)]
fn install_windows(setup: &Path) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let exe = std::env::current_exe()?;
    let dir = exe.parent().context("no install directory")?;
    let mut cmd = std::process::Command::new(setup);
    cmd.args(["/S", "/R"])
        .arg(format!("/WAIT={}", std::process::id()))
        // NSIS wants /D last and unquoted, even with spaces in the path.
        .raw_arg(format!("/D={}", dir.display()))
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    cmd.spawn().context("starting the installer")?;
    Ok(())
}

#[cfg(unix)]
fn install_appimage(new: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;

    let current = PathBuf::from(std::env::var_os("APPIMAGE").context("APPIMAGE is not set")?);
    let staged = current.with_extension("AppImage.new");
    // Copy (the download may be on another filesystem), then rename for an atomic swap.
    std::fs::copy(new, &staged).context("copying the new AppImage next to the old one")?;
    let swap = (|| {
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&staged, &current)
    })();
    if let Err(e) = swap {
        let _ = std::fs::remove_file(&staged);
        return Err(e).context("replacing the AppImage");
    }
    let _ = std::fs::remove_file(new);
    std::process::Command::new(&current)
        .args(std::env::args_os().skip(1))
        .env_remove("APPDIR")
        .env_remove("ARGV0")
        .process_group(0)
        .spawn()
        .context("restarting")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(newer("0.5.1", "0.5.0"));
        assert!(newer("v1.0", "0.9.9"));
        assert!(!newer("0.5.0", "0.5.0"));
        assert!(!newer("0.4.9", "0.5.0"));
        assert!(newer("0.5.0.1", "0.5.0"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(!newer("0.5.0-beta", "0.5.0"));
    }

    fn assets() -> Vec<ReleaseAsset> {
        [
            "Mosaic-0.7.0-x86_64.AppImage",
            "mosaic_0.7.0_amd64.deb",
            "mosaic-0.7.0-linux-x86_64.tar.gz",
            "Mosaic-0.7.0-windows-x64-setup.exe",
            "Mosaic-0.7.0-windows-x64-portable.exe",
            "Mosaic-0.7.0-macos-universal.dmg",
            "SHA256SUMS.txt",
        ]
        .into_iter()
        .map(|n| ReleaseAsset {
            name: n.into(),
            browser_download_url: format!("https://example.com/dl/{n}"),
            size: 42,
        })
        .collect()
    }

    #[test]
    fn picks_the_platform_asset() {
        let a = assets();
        let pick = |t| pick_asset(&a, t).map(|x| x.name);
        assert_eq!(
            pick(Target::WindowsInstalled).as_deref(),
            Some("Mosaic-0.7.0-windows-x64-setup.exe")
        );
        assert_eq!(
            pick(Target::WindowsPortable).as_deref(),
            Some("Mosaic-0.7.0-windows-x64-portable.exe")
        );
        assert_eq!(
            pick(Target::LinuxAppImage).as_deref(),
            Some("Mosaic-0.7.0-x86_64.AppImage")
        );
        assert_eq!(
            pick(Target::MacOs).as_deref(),
            Some("Mosaic-0.7.0-macos-universal.dmg")
        );
        assert_eq!(pick(Target::Other), None);
        let picked = pick_asset(&a, Target::MacOs).unwrap();
        assert_eq!(picked.size, 42);
        assert_eq!(
            picked.checksums_url.as_deref(),
            Some("https://example.com/dl/SHA256SUMS.txt")
        );
        // A release without the platform's file, or without checksums.
        assert_eq!(pick_asset(&a[1..3], Target::WindowsInstalled), None);
        assert_eq!(
            pick_asset(&a[..1], Target::LinuxAppImage)
                .unwrap()
                .checksums_url,
            None
        );
    }

    #[test]
    fn parses_checksums() {
        let h1 = "a".repeat(64);
        let h2 = "B".repeat(64);
        let text = format!(
            "{h1}  Mosaic-0.7.0-windows-x64-setup.exe\r\n{h2} *mosaic_0.7.0_amd64.deb\n\
             {h1}  ./Mosaic-0.7.0-x86_64.AppImage\nnot a line\nabc  short.bin\n"
        );
        assert_eq!(
            checksum_for(&text, "Mosaic-0.7.0-windows-x64-setup.exe"),
            Some(h1.clone())
        );
        assert_eq!(
            checksum_for(&text, "mosaic_0.7.0_amd64.deb"),
            Some("b".repeat(64))
        );
        assert_eq!(
            checksum_for(&text, "Mosaic-0.7.0-x86_64.AppImage"),
            Some(h1)
        );
        assert_eq!(checksum_for(&text, "short.bin"), None);
        assert_eq!(checksum_for(&text, "missing.exe"), None);
    }

    #[test]
    fn parses_release_json() {
        let json = r#"{"tag_name":"v0.7.0","html_url":"u","body":"b","assets":[
            {"name":"Mosaic-0.7.0-windows-x64-setup.exe","browser_download_url":"https://x/s","size":7,"extra":1}]}"#;
        let r: Release = serde_json::from_str(json).unwrap();
        let a = pick_asset(&r.assets, Target::WindowsInstalled).unwrap();
        assert_eq!((a.url.as_str(), a.size), ("https://x/s", 7));
        // JSON without assets still parses.
        let r: Release = serde_json::from_str(r#"{"tag_name":"v1"}"#).unwrap();
        assert!(r.assets.is_empty());
    }
}
