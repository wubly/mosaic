use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

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
}
