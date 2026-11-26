//! The loopback server between Mosaic and the player page: serves the page at `/player` and one
//! WebSocket at `/ws`. Bound to 127.0.0.1 on a random port. The socket must come from the page's
//! own origin and open with `hello` carrying the per-run secret (handed to the page in the URL
//! fragment, which never travels over HTTP). See docs/spotify-cef-backend.md, section 5.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub const PAGE: &str = include_str!("player.html");
pub const PROTOCOL: u64 = 1;

/// Where page messages go, and how Mosaic reaches the page.
pub struct Wiring {
    pub on_message: Box<dyn Fn(Value) + Send + Sync>,
    pub on_disconnect: Box<dyn Fn() + Send + Sync>,
    /// The connected page's outbox; replaced when the page reconnects.
    pub outbox: Mutex<Option<mpsc::UnboundedSender<String>>>,
}

pub struct Bridge {
    pub port: u16,
}

impl Bridge {
    pub fn page_url(&self, secret: &str) -> String {
        format!("http://127.0.0.1:{}/player#s={secret}", self.port)
    }
}

/// Starts listening; serves until the runtime shuts down or `stop` fires.
pub async fn start(
    secret: String,
    wiring: Arc<Wiring>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> Result<Bridge> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let origin = format!("http://127.0.0.1:{port}");
    tokio::spawn(async move {
        loop {
            let accepted = tokio::select! {
                a = listener.accept() => a,
                _ = &mut stop => break,
            };
            let Ok((stream, _)) = accepted else { continue };
            let (secret, origin, wiring) = (secret.clone(), origin.clone(), Arc::clone(&wiring));
            tokio::spawn(async move {
                if let Err(e) = serve(stream, &secret, &origin, wiring).await {
                    tracing::debug!("spotify web bridge: {e}");
                }
            });
        }
    });
    Ok(Bridge { port })
}

/// The request path, read without consuming the request (the WebSocket handshake needs it).
async fn peek_path(stream: &TcpStream) -> Result<String> {
    let mut buf = [0u8; 1024];
    for _ in 0..50 {
        let n = stream.peek(&mut buf).await?;
        let head = String::from_utf8_lossy(&buf[..n]);
        if let Some(line) = head.split("\r\n").next().filter(|_| head.contains("\r\n")) {
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default();
            let target = parts.next().unwrap_or_default();
            if method != "GET" {
                return Err(anyhow!("unexpected method {method}"));
            }
            return Ok(target
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .to_string());
        }
        if n == buf.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(anyhow!("no request line"))
}

/// Reads the request head (closing with unread data would reset the connection on Windows).
async fn drain_head(stream: &mut TcpStream) -> Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
        if stream.read(&mut byte).await? == 0 {
            break;
        }
        head.push(byte[0]);
    }
    Ok(())
}

async fn serve(
    mut stream: TcpStream,
    secret: &str,
    origin: &str,
    wiring: Arc<Wiring>,
) -> Result<()> {
    match peek_path(&stream).await?.as_str() {
        "/player" => {
            drain_head(&mut stream).await?;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
                 Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
            stream.write_all(response.as_bytes()).await?;
            Ok(())
        }
        "/ws" => socket(stream, secret, origin, wiring).await,
        _ => {
            drain_head(&mut stream).await?;
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            Ok(())
        }
    }
}

async fn socket(stream: TcpStream, secret: &str, origin: &str, wiring: Arc<Wiring>) -> Result<()> {
    let expected = origin.to_string();
    // The callback type is tungstenite's; its error response is large by design.
    #[allow(clippy::result_large_err)]
    let check_origin = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let from = req.headers().get("origin").and_then(|o| o.to_str().ok());
        if from == Some(expected.as_str()) {
            Ok(resp)
        } else {
            let mut refused = ErrorResponse::new(Some("forbidden".into()));
            *refused.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
            Err(refused)
        }
    };
    let ws = tokio_tungstenite::accept_hdr_async(stream, check_origin).await?;
    let (mut sink, mut source) = ws.split();

    // The first message must be hello with the secret and our protocol version.
    let hello = tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .map_err(|_| anyhow!("no hello"))?
        .ok_or_else(|| anyhow!("closed before hello"))??;
    let hello: Value = serde_json::from_str(hello.to_text()?)?;
    if hello["type"] != "hello" || hello["secret"].as_str() != Some(secret) {
        return Err(anyhow!("bad hello"));
    }
    if hello["v"].as_u64() != Some(PROTOCOL) {
        (wiring.on_message)(serde_json::json!({
            "type": "error", "kind": "init",
            "message": "Mosaic's Spotify player page is out of date",
        }));
        return Err(anyhow!("protocol mismatch"));
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Ok(mut slot) = wiring.outbox.lock() {
        *slot = Some(tx.clone());
    }
    tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(WsMessage::text(text)).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(msg)) = source.next().await {
        let Ok(text) = msg.to_text() else { continue };
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            (wiring.on_message)(v);
        }
    }
    // Only report a disconnect if no newer page connection replaced this one.
    let current = wiring
        .outbox
        .lock()
        .map(|slot| slot.as_ref().is_some_and(|s| s.same_channel(&tx)))
        .unwrap_or(false);
    if current {
        if let Ok(mut slot) = wiring.outbox.lock() {
            *slot = None;
        }
        (wiring.on_disconnect)();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    fn wiring(count: Arc<AtomicUsize>) -> Arc<Wiring> {
        Arc::new(Wiring {
            on_message: Box::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
            }),
            on_disconnect: Box::new(|| {}),
            outbox: Mutex::new(None),
        })
    }

    async fn connect(
        port: u16,
        origin: &str,
    ) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>>
    {
        let mut req = format!("ws://127.0.0.1:{port}/ws").into_client_request()?;
        req.headers_mut().insert("origin", origin.parse()?);
        Ok(tokio_tungstenite::connect_async(req).await?.0)
    }

    #[tokio::test]
    async fn serves_the_page_and_refuses_other_paths() {
        let (_tx, rx) = tokio::sync::oneshot::channel();
        let bridge = start("s".into(), wiring(Arc::default()), rx).await.unwrap();
        let get = |path: &'static str| async move {
            let mut s = TcpStream::connect(("127.0.0.1", bridge.port))
                .await
                .unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        let page = get("/player").await;
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("Spotify.Player"));
        assert!(get("/token").await.starts_with("HTTP/1.1 404"));
    }

    #[tokio::test]
    async fn the_socket_needs_our_origin_and_the_secret() {
        let count = Arc::new(AtomicUsize::new(0));
        let (_tx, rx) = tokio::sync::oneshot::channel();
        let bridge = start("right".into(), wiring(Arc::clone(&count)), rx)
            .await
            .unwrap();
        let origin = format!("http://127.0.0.1:{}", bridge.port);

        assert!(
            connect(bridge.port, "https://evil.example").await.is_err(),
            "foreign origin"
        );

        let mut wrong = connect(bridge.port, &origin).await.unwrap();
        wrong
            .send(WsMessage::text(
                r#"{"type":"hello","secret":"wrong","v":1}"#,
            ))
            .await
            .unwrap();
        wrong
            .send(WsMessage::text(r#"{"type":"ready","device_id":"x"}"#))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "nothing gets through without the secret"
        );

        let mut ok = connect(bridge.port, &origin).await.unwrap();
        ok.send(WsMessage::text(
            r#"{"type":"hello","secret":"right","v":1}"#,
        ))
        .await
        .unwrap();
        ok.send(WsMessage::text(r#"{"type":"ready","device_id":"x"}"#))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
