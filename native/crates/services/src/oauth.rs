//! A one-shot loopback HTTP server for OAuth redirects.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub struct Callback {
    pub params: HashMap<String, String>,
    stream: TcpStream,
}

impl Callback {
    pub async fn respond(mut self, message: &str) {
        let body = format!(
            "<body style=\"font:16px system-ui;background:#141218;color:#e6e0e9;display:grid;place-items:center;height:100vh;margin:0\"><p>{message}</p></body>"
        );
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = self.stream.write_all(reply.as_bytes()).await;
        let _ = self.stream.shutdown().await;
    }
}

pub async fn bind(port: u16) -> Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            anyhow!("Port {port} is busy")
        } else {
            anyhow!(e)
        }
    })
}

/// Waits for a request to `path` and returns its query parameters.
pub async fn wait_for_callback(listener: &TcpListener, path: &str) -> Result<Callback> {
    let accept = async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let Some(target) = request.lines().next().and_then(|l| l.split(' ').nth(1)) else {
                continue;
            };
            let (p, query) = target.split_once('?').unwrap_or((target, ""));
            if p != path {
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            return Ok::<_, anyhow::Error>(Callback {
                params: parse_query(query),
                stream,
            });
        }
    };
    tokio::time::timeout(TIMEOUT, accept)
        .await
        .map_err(|_| anyhow!("Sign-in timed out"))?
}

pub fn parse_query(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percent_encoded_queries() {
        let q = parse_query("code=a%2Fb&state=xyz&error=");
        assert_eq!(q["code"], "a/b");
        assert_eq!(q["state"], "xyz");
        assert_eq!(q["error"], "");
    }

    #[tokio::test]
    async fn serves_one_callback() {
        let listener = bind(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(b"GET /callback?code=1&state=2 HTTP/1.1\r\nHost: x\r\n\r\n")
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        });
        let cb = wait_for_callback(&listener, "/callback").await.unwrap();
        assert_eq!(cb.params["code"], "1");
        cb.respond("Connected").await;
        assert!(client.await.unwrap().contains("Connected"));
    }
}
