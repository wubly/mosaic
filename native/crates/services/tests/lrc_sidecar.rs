use std::sync::{Arc, Mutex};

use mosaic_core::storage::Dirs;
use mosaic_services::lyrics::service::LyricsArgs;
use mosaic_services::Services;

fn silent_wav(path: &std::path::Path) {
    let mut bytes = Vec::new();
    let data = vec![0u8; 8000 * 2];
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&data);
    std::fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn a_sibling_lrc_file_becomes_the_lyrics_of_a_scanned_local_track() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    silent_wav(&music.join("Artist - Song.wav"));
    std::fs::write(music.join("Artist - Song.lrc"), "[00:01.00]first line\n[00:03.50]second line\n").unwrap();

    let s = Services::new(Dirs::with_root(dir.path().join("data")), Arc::new(Mutex::default())).unwrap();
    let tracks = s.scan_paths(&[music.to_string_lossy().into_owned()]).await.unwrap();
    assert_eq!(tracks.len(), 1);
    let lrc = tracks[0].lrc_path.clone().expect("lrc_path is set");
    assert!(lrc.ends_with("Artist - Song.lrc"));

    let lyrics = s
        .get_lyrics(&LyricsArgs {
            id: tracks[0].id.clone(),
            title: tracks[0].title.clone(),
            artist: tracks[0].artist.clone(),
            duration_ms: tracks[0].duration,
            lrc_path: Some(lrc),
        })
        .await;
    let lines: Vec<_> = lyrics.synced.iter().map(|l| (l.t, l.text.as_str())).collect();
    assert_eq!(lines, [(1.0, "first line"), (3.5, "second line")]);
    assert_eq!(lyrics.meta.origin, "file");
}
