use std::sync::{Arc, Mutex};

use mosaic_core::lyrics_timing::LyricLine;
use mosaic_core::storage::Dirs;
use mosaic_services::lyrics::service::LyricsArgs;
use mosaic_services::Services;

fn services() -> (Services, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Services::new(Dirs::with_root(dir.path()), Arc::new(Mutex::default())).unwrap();
    (s, dir)
}

#[tokio::test]
#[ignore]
async fn net_soundcloud_search_and_stream() {
    let (s, _dir) = services();
    let tracks = s.sc_search("daft punk", 10).await.unwrap();
    assert!(!tracks.is_empty());
    assert!(tracks.iter().all(|t| t.streamable));
    let stream = s.sc_stream(&tracks[0].id).await.unwrap();
    assert!(stream.url.starts_with("http"));
}

#[tokio::test]
#[ignore]
async fn net_lrclib_returns_synced_lyrics() {
    let (s, _dir) = services();
    let args = LyricsArgs {
        id: mosaic_core::model::TrackId::Num(1),
        title: "Bohemian Rhapsody".into(),
        artist: "Queen".into(),
        duration_ms: 354_000,
        lrc_path: None,
    };
    let lyrics = s.get_lyrics(&args).await;
    let synced: &Vec<LyricLine> = &lyrics.synced;
    assert!(!synced.is_empty(), "no synced lyrics: {:?}", lyrics.meta);
}

#[tokio::test]
#[ignore]
async fn net_monochrome_search_and_stream() {
    let (s, _dir) = services();
    let tracks = s.mono_search("daft punk one more time").await.unwrap();
    let first = tracks.first().expect("no results");
    assert!(first.title.to_lowercase().contains("one more time"));
    let url = mosaic_services::monochrome::stream_url(&first.id);
    let res = s
        .http()
        .get(&url)
        .header("Range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert_eq!(&res.bytes().await.unwrap()[..], b"fLaC");
}

#[tokio::test]
#[ignore]
async fn net_monochrome_radio() {
    let (s, _dir) = services();
    let hits = s.mono_search("daft punk one more time").await.unwrap();
    let seed = hits.first().expect("no results");
    let radio = s.mono_related(seed, 20).await.unwrap();
    for t in &radio {
        println!("{} - {} ({})", t.artist, t.title, t.id.key());
    }
    assert!(radio.len() >= 10, "only {} tracks", radio.len());
    assert!(radio.iter().all(|t| t.id != seed.id));
}
