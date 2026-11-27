//! Launches `mosaic --youtube-signin`, the visible sign-in window (see the app crate's
//! `youtube_signin.rs`), and signs in with the cookies it captures.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::auth::Account;
use crate::Services;

const OVERALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

impl Services {
    /// Opens the YouTube Music sign-in window and waits for the user. `Ok(None)` when they close
    /// it without signing in.
    pub async fn yt_sign_in_window(&self) -> Result<Option<Account>> {
        let data_dir = self.inner.dirs.images().with_file_name("youtube-signin");
        std::fs::create_dir_all(&data_dir)?;
        let exe = std::env::current_exe()?;
        let mut child = Command::new(exe)
            .arg("--youtube-signin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("Could not open the sign-in window: {e}"))?;
        let mut stdin = child.stdin.take().context("no stdin")?;
        let config = json!({ "data_dir": data_dir.to_string_lossy() });
        stdin.write_all(format!("{config}\n").as_bytes()).await?;
        stdin.flush().await?;
        let stdout = child.stdout.take().context("no stdout")?;

        let cookies = tokio::time::timeout(OVERALL_TIMEOUT, async {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) = lines.next_line().await? {
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                match v["event"].as_str() {
                    Some("cookies") => {
                        return Ok(Some(v["cookies"].as_str().unwrap_or_default().to_string()))
                    }
                    Some("cancelled") => return Ok(None),
                    Some("error") => bail!(
                        "{}",
                        v["message"].as_str().unwrap_or("the sign-in window failed")
                    ),
                    _ => {}
                }
            }
            bail!("the sign-in window closed unexpectedly")
        })
        .await
        .map_err(|_| anyhow!("Sign-in timed out"))??;

        // Closing stdin (and the kill on drop) stops the window.
        drop(stdin);
        let _ = child.start_kill();
        match cookies {
            Some(c) => Ok(Some(self.yt_cookie_login(&c).await?)),
            None => Ok(None),
        }
    }
}
