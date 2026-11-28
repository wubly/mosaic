//! Drives the Soloist backend against a stand-in daemon: a shell script plays the `soloist`
//! binary (writes the runtime files, stays alive) and the test serves the WebSocket itself.
// Soloist is only used where Spotify doesn't play through the system web view (Linux and other
// Unixes); on macOS `spotify_playback()` starts the web player instead.
#![cfg(all(unix, not(target_os = "macos"), feature = "spotify"))]

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use mosaic_core::storage::Dirs;
use mosaic_services::spotify::playback::{SpotifyPlayback, Target};
use mosaic_services::spotify::soloist::SoloistSettings;
use mosaic_services::Services;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

fn fake_soloist(dir: &std::path::Path, port: u16) -> std::path::PathBuf {
    let path = dir.join("soloist");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             cmd=\"$1\"; all=\"$*\"\n\
             while [ $# -gt 0 ]; do case \"$1\" in --data-dir) dir=\"$2\"; shift;; esac; shift; done\n\
             if [ \"$cmd\" = setup-api-key ]; then read -r key; printf '%s' \"$key\" > \"$dir/api_key\"; exit 0; fi\n\
             printf '%s' \"$all\" > \"$dir/args\"\n\
             echo $$ > \"$dir/soloist.pid\"\n\
             printf '127.0.0.1\\n' > \"$dir/ws.addr\"\n\
             printf '{port}\\n' > \"$dir/ws.port\"\n\
             echo 'fake soloist up'\n\
             exec sleep 30\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

async fn next_command<S>(ws: &mut S) -> Value
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a command in time")
            .unwrap()
            .unwrap();
        if let Ok(text) = msg.to_text() {
            let v: Value = serde_json::from_str(text).unwrap();
            // The background state poll isn't part of what the test drives.
            if v["command"] != "get_state" {
                return v;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn soloist_plays_a_track_feeds_a_paced_clock_and_finishes_when_it_moves_on() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let binary = fake_soloist(dir.path(), port);

    let services = Services::new(
        Dirs::with_root(dir.path().join("data")),
        Arc::new(Mutex::default()),
    )
    .unwrap();
    services
        .soloist_save(&SoloistSettings {
            path: binary.display().to_string(),
            api_key: "test-key".into(),
        })
        .unwrap();
    let saved = std::fs::metadata(dir.path().join("data/soloist.json")).unwrap();
    assert_eq!(
        saved.permissions().mode() & 0o777,
        0o600,
        "the key file is private"
    );

    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        ws.send(Message::text(
            json!({ "type": "auth_state", "logged_in": true, "is_active": false,
                    "device_name": "Mosaic" })
            .to_string(),
        ))
        .await
        .unwrap();
        ws
    });
    let playback = services.spotify_playback().await.unwrap();
    let mut ws = server.await.unwrap();
    assert!(matches!(playback, SpotifyPlayback::Soloist(_)));
    assert!(playback.describe().contains("paired"));
    let soloist_dir = dir.path().join("data/soloist");
    assert_eq!(
        std::fs::read_to_string(soloist_dir.join("api_key")).unwrap(),
        "test-key",
        "the key goes in through setup-api-key"
    );
    let args = std::fs::read_to_string(soloist_dir.join("args")).unwrap();
    assert!(
        !args.contains("test-key") && !args.contains("--api-key"),
        "the key never appears in the daemon's command line: {args}"
    );

    let chunks = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let moved: Arc<Mutex<Option<String>>> = Arc::default();
    let (c, f, m) = (
        Arc::clone(&chunks),
        Arc::clone(&finished),
        Arc::clone(&moved),
    );
    let target = Target {
        write: Arc::new(move |samples| {
            assert!(samples.iter().all(|s| *s == 0.0));
            c.fetch_add(1, Ordering::SeqCst);
            true
        }),
        finish: Arc::new(move || {
            f.fetch_add(1, Ordering::SeqCst);
        }),
        fail: Arc::new(|m| panic!("unexpected failure: {m}")),
        moved: Arc::new(move |uri| *m.lock().unwrap() = Some(uri)),
    };
    playback.load("spotify:track:a", None, 0, target).unwrap();
    assert_eq!(next_command(&mut ws).await["command"], "activate");
    let play = next_command(&mut ws).await;
    assert_eq!(play["command"], "play");
    assert_eq!(play["uri"], "spotify:track:a");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        chunks.load(Ordering::SeqCst),
        0,
        "nothing is fed before Soloist plays"
    );

    ws.send(Message::text(
        json!({ "type": "playback_state", "status": "playing", "is_active": true,
                "item": { "uri": "spotify:track:a" },
                "position": { "position_ms": 0, "timestamp_ms": 0, "speed": 1.0 } })
        .to_string(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let fed = chunks.load(Ordering::SeqCst);
    // 0.5 s played plus the 0.6 s lead is about 24 chunks of 2048 frames.
    assert!((18..=30).contains(&fed), "fed {fed} chunks");

    ws.send(Message::text(
        json!({ "type": "track_changed", "item": { "uri": "spotify:track:autoplay" } }).to_string(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    assert_eq!(
        moved.lock().unwrap().as_deref(),
        Some("spotify:track:autoplay"),
        "Mosaic hears which track Soloist moved on to"
    );
    let after = chunks.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        chunks.load(Ordering::SeqCst),
        after,
        "the feed stops with the track"
    );

    services.spotify_playback_reset().await;
}
