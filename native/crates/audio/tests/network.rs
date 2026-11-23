mod common;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use common::*;
use mosaic_audio::{Command, DeckId, Event, SourceSpec};

const SEGMENTS: [&[u8]; 3] = [
    include_bytes!("fixtures/seg0.mp3"),
    include_bytes!("fixtures/seg1.mp3"),
    include_bytes!("fixtures/seg2.mp3"),
];

type RequestLog = Arc<Mutex<Vec<(String, Option<String>)>>>;

struct Server {
    base: String,
    requests: RequestLog,
}

fn serve(routes: HashMap<String, (u16, Vec<u8>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let routes = Arc::new(routes);
    let log = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let routes = Arc::clone(&routes);
            let log = Arc::clone(&log);
            std::thread::spawn(move || handle(stream, &routes, &log));
        }
    });
    Server { base, requests }
}

fn handle(mut stream: TcpStream, routes: &HashMap<String, (u16, Vec<u8>)>, log: &RequestLog) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let url = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut range = None;
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_string());
            }
        }
    }
    log.lock().unwrap().push((url.clone(), range.clone()));
    let Some((status, body)) = routes.get(&url) else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };
    if *status != 200 {
        let head = format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = stream.write_all(head.as_bytes());
        return;
    }
    let start = range
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
    let head = match start {
        Some(start) => {
            let start = start.min(body.len());
            format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n",
                body.len() - start,
                body.len() - 1,
                body.len()
            )
        }
        None => format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ),
    };
    if stream.write_all(head.as_bytes()).is_ok() {
        let _ = stream.write_all(&body[start.unwrap_or(0).min(body.len())..]);
    }
}

fn wav_bytes(segments: &[(f32, f32)]) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::read(write_wav(dir.path(), "x.wav", 48_000, segments, 0.5)).unwrap()
}

fn load_source(rig: &mut Rig, source: SourceSpec, start_at: f64) {
    rig.engine.send(Command::Load {
        deck: DeckId::A,
        gen: 1,
        source,
        start_at,
        autoplay: true,
        fade_in_ms: 0,
        duration_hint: None,
    });
}

#[test]
fn http_source_plays_and_seeks_with_range_requests() {
    let mut routes = HashMap::new();
    routes.insert(
        "/a.wav".to_string(),
        (200, wav_bytes(&[(440.0, 150.0), (880.0, 150.0)])),
    );
    let server = serve(routes);

    let mut rig = Rig::new();
    load_source(
        &mut rig,
        SourceSpec::Http {
            url: format!("{}/a.wav", server.base),
        },
        0.0,
    );
    let Event::Ready { duration, .. } = rig.wait_for("ready", |e| matches!(e, Event::Ready { .. }))
    else {
        unreachable!()
    };
    assert!(duration.is_some_and(|d| (d - 300.0).abs() < 0.5));
    let first = mono(&rig.render_secs(1.0));
    assert!((peak_freq(&first[9600..], 400.0, 900.0) - 440.0).abs() <= 2.0);

    rig.engine.send(Command::Seek {
        deck: DeckId::A,
        secs: 290.0,
    });
    rig.wait_for("seeked", |e| matches!(e, Event::Seeked { .. }));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let after = mono(&rig.render_secs(1.0));
    let f = peak_freq(&after[9600..], 400.0, 900.0);
    assert!((f - 880.0).abs() <= 2.0, "peak {f} rms {}", rms(&after));
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(_, r)| r.is_some()),
        "expected a Range request, saw {:?}",
        server.requests.lock().unwrap()
    );
}

fn hls_routes(extra: &[(&str, u16)]) -> HashMap<String, (u16, Vec<u8>)> {
    let mut routes = HashMap::new();
    let list = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXTINF:1.0,\nseg0.mp3\n#EXTINF:1.0,\nseg1.mp3\n#EXTINF:1.0,\nseg2.mp3\n#EXT-X-ENDLIST\n";
    routes.insert("/list.m3u8".to_string(), (200, list.as_bytes().to_vec()));
    for (i, seg) in SEGMENTS.iter().enumerate() {
        routes.insert(format!("/seg{i}.mp3"), (200, seg.to_vec()));
    }
    for (path, status) in extra {
        routes.insert((*path).to_string(), (*status, Vec::new()));
    }
    routes
}

#[test]
fn hls_plays_segments_in_order_and_seeks() {
    let server = serve(hls_routes(&[]));
    let mut rig = Rig::new();
    load_source(
        &mut rig,
        SourceSpec::Hls {
            playlist_url: format!("{}/list.m3u8", server.base),
        },
        0.0,
    );
    let Event::Ready { duration, .. } = rig.wait_for("ready", |e| matches!(e, Event::Ready { .. }))
    else {
        unreachable!()
    };
    assert_eq!(duration, Some(3.0));

    let out = mono(&rig.render_secs(3.4));
    let tone = |from: f32, to: f32| {
        peak_freq(
            &out[(from * 48_000.0) as usize..(to * 48_000.0) as usize],
            400.0,
            900.0,
        )
    };
    assert!((tone(0.2, 0.8) - 440.0).abs() <= 3.0);
    assert!((tone(1.2, 1.8) - 660.0).abs() <= 3.0);
    assert!((tone(2.2, 2.8) - 880.0).abs() <= 3.0);
}

#[test]
fn hls_seek_lands_in_the_right_segment() {
    let server = serve(hls_routes(&[]));
    let mut rig = Rig::new();
    load_source(
        &mut rig,
        SourceSpec::Hls {
            playlist_url: format!("{}/list.m3u8", server.base),
        },
        0.0,
    );
    rig.wait_for("ready", |e| matches!(e, Event::Ready { .. }));
    rig.render_secs(0.2);
    rig.engine.send(Command::Seek {
        deck: DeckId::A,
        secs: 1.5,
    });
    rig.wait_for("seeked", |e| matches!(e, Event::Seeked { .. }));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let after = mono(&rig.render_secs(0.6));
    let f = peak_freq(&after[4800..], 400.0, 900.0);
    assert!((f - 660.0).abs() <= 3.0, "peak {f}");
}

#[test]
fn hls_expired_segment_reports_expiry() {
    let server = serve(hls_routes(&[("/seg1.mp3", 410)]));
    let mut rig = Rig::new();
    load_source(
        &mut rig,
        SourceSpec::Hls {
            playlist_url: format!("{}/list.m3u8", server.base),
        },
        0.0,
    );
    rig.wait_for("ready", |e| matches!(e, Event::Ready { .. }));
    let Event::Error { message, .. } =
        rig.wait_for_rendering("error", |e| matches!(e, Event::Error { .. }))
    else {
        unreachable!()
    };
    assert_eq!(message, "stream expired");
}

#[test]
fn opus_files_decode() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.opus");
    let mut rig = Rig::new();
    load_source(&mut rig, SourceSpec::File(path), 0.0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    let out = mono(&rig.render_secs(2.5));
    assert!((peak_freq(&out[12_000..60_000], 900.0, 1100.0) - 1000.0).abs() <= 2.0);
    let active = out.iter().rposition(|s| s.abs() > 0.02).unwrap() as f32 / OUT_RATE as f32;
    assert!((active - 2.0).abs() < 0.1, "length {active}");
}

#[test]
fn ffmpeg_fallback_handles_unsupported_formats() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("ffmpeg not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.ac3");
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=2:sample_rate=48000",
            "-ac",
            "2",
            "-c:a",
            "ac3",
        ])
        .arg(&path)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("ffmpeg lacks an ac3 encoder; skipping");
        return;
    }
    let mut rig = Rig::new();
    load_source(&mut rig, SourceSpec::File(path), 0.0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    let out = mono(&rig.render_secs(1.0));
    assert!((peak_freq(&out[9600..], 800.0, 950.0) - 880.0).abs() <= 3.0);
}

#[test]
fn unreachable_http_reports_error() {
    let mut rig = Rig::new();
    load_source(
        &mut rig,
        SourceSpec::Http {
            url: "http://127.0.0.1:1/none.mp3".into(),
        },
        0.0,
    );
    rig.wait_for("error", |e| matches!(e, Event::Error { .. }));
}
