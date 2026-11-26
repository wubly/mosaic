//! A tiny HTTP server on 127.0.0.1:8898 (the redirect port Mosaic's Spotify app already has
//! registered): serves the SDK test page at /sdk and /callback, and prints whatever pages POST
//! to /log. Blocking std I/O on one thread is plenty for a test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use crate::{opts, PORT};

const SDK_PAGE: &str = include_str!("../pages/sdk.html");

pub fn spawn() {
    let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("port {PORT} is busy ({e}); quit Mosaic or anything else using it");
            std::process::exit(1);
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = handle(stream);
            });
        }
    });
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let path = target.split(['?', '#']).next().unwrap_or_default();
    match (method.as_str(), path) {
        ("OPTIONS", "/log") => respond(&mut stream, "204 No Content", "text/plain", ""),
        ("OPTIONS", _) => respond_private(&mut stream, "204 No Content", "text/plain", ""),
        ("GET", "/sdk" | "/callback") => {
            let o = opts();
            let page = SDK_PAGE
                .replace("__CLIENT_ID__", &o.client_id)
                .replace("__TRACKS__", &o.tracks)
                .replace("__AUTORUN__", if o.autorun { "true" } else { "false" })
                .replace("__CHECK__", if o.check { "true" } else { "false" });
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", &page)
        }
        ("GET", "/token") => match std::fs::read_to_string(token_file()) {
            Ok(t) => respond_private(&mut stream, "200 OK", "application/json", &t),
            Err(_) => respond_private(&mut stream, "404 Not Found", "text/plain", "no token"),
        },
        ("PUT", "/token") => {
            let mut body = vec![0u8; length.min(1 << 16)];
            reader.read_exact(&mut body)?;
            std::fs::write(token_file(), &body)?;
            respond_private(&mut stream, "204 No Content", "text/plain", "")
        }
        ("DELETE", "/token") => {
            let _ = std::fs::remove_file(token_file());
            respond_private(&mut stream, "204 No Content", "text/plain", "")
        }
        ("POST", "/log") => {
            let mut body = vec![0u8; length.min(1 << 20)];
            reader.read_exact(&mut body)?;
            println!("[spike] {}", String::from_utf8_lossy(&body));
            respond(&mut stream, "204 No Content", "text/plain", "")
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

/// The test's Spotify token, outside the CEF profile so a killed browser can't lose it.
fn token_file() -> std::path::PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("mosaic-cef-spike")
        .join("token.json")
}

/// Same-origin only (no CORS headers): for the token, which no other site may read.
fn respond_private(stream: &mut TcpStream, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}
Content-Type: {kind}
Content-Length: {}
         Cache-Control: no-store
Connection: close

{body}",
        body.len()
    )
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    // CORS + Private Network Access headers so open.spotify.com's driver may post here.
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: *\r\nAccess-Control-Allow-Private-Network: true\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
