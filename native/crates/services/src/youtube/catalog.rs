//! YouTube Music's side of `crate::catalog`. (Stub: songs only, to be filled in.)

use anyhow::{bail, Result};

use crate::catalog::{Card, Collection, Found};
use crate::Services;

impl Services {
    pub async fn yt_search_all(&self, query: &str) -> Result<Found> {
        Ok(Found {
            tracks: self.yt_search(query).await?,
            ..Found::default()
        })
    }

    pub async fn yt_open(&self, _card: &Card) -> Result<Collection> {
        bail!("not yet")
    }
}
