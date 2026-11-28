//! `mosaic --youtube-signin`: a visible system web view window (WebView2 on Windows, WKWebView
//! on macOS) showing Google's sign-in page for YouTube Music. Once the user lands on
//! `music.youtube.com`, it reads the account cookies (HttpOnly ones included) and prints them.
//!
//! Protocol: the first stdin line is a JSON config `{ "data_dir" }`. The process prints one JSON
//! line on stdout: `{"event":"cookies","cookies":"a=b; c=d"}`, `{"event":"cancelled"}` (window
//! closed) or `{"event":"error","message":...}`, then exits. It also exits when stdin closes.

#[cfg(all(feature = "spotify", any(windows, target_os = "macos")))]
pub fn run() -> anyhow::Result<()> {
    imp::run()
}

#[cfg(not(all(feature = "spotify", any(windows, target_os = "macos"))))]
pub fn run() -> anyhow::Result<()> {
    anyhow::bail!("YouTube Music sign-in window is not supported on Linux yet")
}

#[cfg(all(feature = "spotify", any(windows, target_os = "macos")))]
mod imp {
    use std::io::{BufRead, Write};

    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;
    use wry::{PageLoadEvent, WebContext, WebViewBuilder};

    /// Google refuses sign-in in web views that identify as embedded; present as plain Edge.
    const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";

    fn emit(value: serde_json::Value) {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }

    fn fail(message: &str) -> ! {
        emit(serde_json::json!({ "event": "error", "message": message }));
        std::process::exit(1);
    }

    fn is_music_host(url: &str) -> bool {
        url.strip_prefix("https://")
            .and_then(|r| r.split(['/', '?', '#']).next())
            .is_some_and(|h| h.eq_ignore_ascii_case("music.youtube.com"))
    }

    pub fn run() -> anyhow::Result<()> {
        let mut first = String::new();
        std::io::stdin().lock().read_line(&mut first)?;
        let config: serde_json::Value = match serde_json::from_str(first.trim()) {
            Ok(v) => v,
            Err(e) => fail(&format!("bad sign-in config: {e}")),
        };
        let data_dir = config["data_dir"].as_str().map(std::path::PathBuf::from);

        // Mosaic closes our stdin to stop us (and it closes by itself if Mosaic dies).
        std::thread::spawn(|| {
            let mut sink = String::new();
            loop {
                sink.clear();
                match std::io::stdin().lock().read_line(&mut sink) {
                    Ok(0) | Err(_) => std::process::exit(0),
                    Ok(_) => {}
                }
            }
        });

        #[derive(Debug)]
        enum Ev {
            MusicLoaded,
        }
        #[allow(unused_mut)]
        let mut event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
        let proxy = event_loop.create_proxy();

        let window = WindowBuilder::new()
            .with_title("Sign in to YouTube Music")
            .with_inner_size(LogicalSize::new(480.0, 720.0))
            .with_visible(false);
        let window = match window.build(&event_loop) {
            Ok(w) => w,
            Err(e) => fail(&format!("could not create the sign-in window: {e}")),
        };
        // Centre on the primary monitor, then show.
        if let Some(m) = window
            .current_monitor()
            .or_else(|| window.primary_monitor())
        {
            let ms = m.size();
            let ws = window.outer_size();
            let mp = m.position();
            window.set_outer_position(tao::dpi::PhysicalPosition::new(
                mp.x + (ms.width.saturating_sub(ws.width) / 2) as i32,
                mp.y + (ms.height.saturating_sub(ws.height) / 2) as i32,
            ));
        }
        window.set_visible(true);
        window.set_focus();

        let mut context = WebContext::new(data_dir);
        let builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(mosaic_services::youtube::auth::SIGN_IN_URL)
            .with_user_agent(USER_AGENT)
            .with_devtools(false)
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, PageLoadEvent::Finished) && is_music_host(&url) {
                    let _ = proxy.send_event(Ev::MusicLoaded);
                }
            });
        let webview = match builder.build(&window) {
            Ok(w) => w,
            Err(e) => {
                let hint = if cfg!(windows) {
                    " (is the Microsoft Edge WebView2 Runtime installed?)"
                } else {
                    ""
                };
                fail(&format!("could not create the web view: {e}{hint}"))
            }
        };

        // Google can write the session cookies a moment after the page reports it has loaded,
        // so a load without them is checked again a few times before waiting for the next one.
        let mut checks_left = 0u32;
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            let check = match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    emit(serde_json::json!({ "event": "cancelled" }));
                    *control_flow = ControlFlow::Exit;
                    false
                }
                Event::UserEvent(Ev::MusicLoaded) => {
                    checks_left = 10;
                    true
                }
                Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. }) => {
                    checks_left > 0
                }
                _ => false,
            };
            if !check {
                return;
            }
            checks_left = checks_left.saturating_sub(1);
            let mut pairs: Vec<(String, String)> = Vec::new();
            for url in ["https://music.youtube.com", "https://www.youtube.com"] {
                if let Ok(cookies) = webview.cookies_for_url(url) {
                    for c in cookies {
                        let pair = (c.name().to_string(), c.value().to_string());
                        if !pairs.iter().any(|p| p.0 == pair.0) {
                            pairs.push(pair);
                        }
                    }
                }
            }
            // Only a signed-in session carries SAPISID; otherwise keep waiting.
            if pairs
                .iter()
                .any(|(n, _)| n == "SAPISID" || n == "__Secure-3PAPISID")
            {
                let cookies = pairs
                    .iter()
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                emit(serde_json::json!({ "event": "cookies", "cookies": cookies }));
                *control_flow = ControlFlow::Exit;
            } else if checks_left > 0 {
                *control_flow = ControlFlow::WaitUntil(
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                );
            }
        });
    }
}
