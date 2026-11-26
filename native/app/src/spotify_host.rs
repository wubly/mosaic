//! `mosaic --spotify-host`: the hidden system web view that runs Spotify's Web Playback SDK
//! (WebView2 on Windows, WKWebView on macOS). Started and stopped by
//! `mosaic_services::spotify::web`; see docs/spotify-cef-backend.md.
//!
//! Protocol: the first stdin line is a JSON config `{ "url", "data_dir" }` (the URL carries the
//! bridge secret, so it never goes on the command line). The host prints JSON lines on stdout
//! (`started`, or `error` with a message) and exits when stdin closes, i.e. when Mosaic stops it
//! or exits itself.

use std::io::{BufRead, Write};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{BackgroundThrottlingPolicy, WebContext, WebViewBuilder};

/// Trimmed WebView2 arguments, measured in the WebView2 spike (184 MB private while playing,
/// 7 processes, vs 384 MB / 11 by default). wry's own `--disable-features` defaults come first
/// because Chromium only reads one `--disable-features`.
#[cfg(windows)]
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IsolateOrigins,site-per-process,AudioServiceOutOfProcess \
     --autoplay-policy=no-user-gesture-required --renderer-process-limit=1 --in-process-gpu \
     --disable-gpu --disable-gpu-compositing --disable-site-isolation-trials \
     --enable-low-end-device-mode --js-flags=--lite-mode --disable-extensions \
     --disable-spell-checking --disable-sync --no-first-run";

fn emit(event: &str, message: Option<&str>) {
    let line = match message {
        Some(m) => serde_json::json!({ "event": event, "message": m }),
        None => serde_json::json!({ "event": event }),
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

pub fn run() -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut first = String::new();
    stdin.lock().read_line(&mut first)?;
    let config: serde_json::Value = match serde_json::from_str(first.trim()) {
        Ok(v) => v,
        Err(e) => {
            emit("error", Some(&format!("bad host config: {e}")));
            std::process::exit(2);
        }
    };
    let url = config["url"].as_str().unwrap_or_default().to_string();
    let data_dir = config["data_dir"].as_str().map(std::path::PathBuf::from);
    if !url.starts_with("http://127.0.0.1:") {
        emit("error", Some("refusing a non-loopback player URL"));
        std::process::exit(2);
    }

    // Mosaic closes our stdin to stop us (and it closes by itself if Mosaic dies).
    std::thread::spawn(move || {
        let mut sink = String::new();
        loop {
            sink.clear();
            match std::io::stdin().lock().read_line(&mut sink) {
                Ok(0) | Err(_) => std::process::exit(0),
                Ok(_) => {}
            }
        }
    });

    #[allow(unused_mut)]
    let mut event_loop = EventLoop::new();
    #[cfg(target_os = "macos")]
    {
        // No Dock icon or menu bar for the hidden player.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Prohibited);
    }
    let window = WindowBuilder::new()
        .with_title("Mosaic Spotify player")
        .with_inner_size(LogicalSize::new(400.0, 300.0))
        .with_visible(false)
        .with_focused(false);
    #[cfg(windows)]
    let window = {
        use tao::platform::windows::WindowBuilderExtWindows;
        window.with_skip_taskbar(true)
    };
    let window = match window.build(&event_loop) {
        Ok(w) => w,
        Err(e) => {
            emit(
                "error",
                Some(&format!("could not create the player window: {e}")),
            );
            std::process::exit(1);
        }
    };

    let mut context = WebContext::new(data_dir);
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url)
        .with_autoplay(true)
        .with_background_throttling(BackgroundThrottlingPolicy::Disabled)
        .with_devtools(false);
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_additional_browser_args(BROWSER_ARGS)
    };
    let _webview = match builder.build(&window) {
        Ok(w) => w,
        Err(e) => {
            let hint = if cfg!(windows) {
                " (is the Microsoft Edge WebView2 Runtime installed?)"
            } else {
                ""
            };
            emit(
                "error",
                Some(&format!("could not create the web view: {e}{hint}")),
            );
            std::process::exit(1);
        }
    };
    emit("started", None);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
