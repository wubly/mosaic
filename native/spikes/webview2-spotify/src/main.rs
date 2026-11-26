//! The WebView2 variant of the spike (docs/spotify-cef-backend.md, section 14 follow-up): the same
//! SDK test page, server and instrumentation, but in the system's Edge WebView2 runtime instead of
//! CEF. Checks whether Spotify's SDK can play here (AAC through Windows, Widevine or PlayReady) and
//! what it costs in memory.
//!
//! Flags (all optional):
//!   --spike-page=sdk|web     which page to open (default sdk)
//!   --spike-hidden           create the window without showing it
//!   --spike-autorun          run the SDK test sequence as soon as the player is ready
//!   --spike-nocheck          skip the page's own Widevine/codec check
//!   --spike-client-id=<id>   the Spotify app client id
//!   --spike-tracks=<uris>    comma-separated track URIs for the sequence
//!   --spike-args=<args>      extra browser arguments (memory levers), space-separated
//!
//! Page logs arrive at the server's /log and are printed as `[spike] {json}` lines.

#[path = "../../cef-spotify/src/server.rs"]
mod server;

use std::sync::OnceLock;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

pub const PORT: u16 = 8898;

/// What `server.rs` reads.
pub struct Opts {
    pub client_id: String,
    pub tracks: String,
    pub autorun: bool,
    pub check: bool,
}

static OPTS: OnceLock<Opts> = OnceLock::new();

pub fn opts() -> &'static Opts {
    OPTS.get().expect("options are set before the server starts")
}

const INSTRUMENT: &str = include_str!("../../cef-spotify/pages/instrument.js");
const DRIVER: &str = include_str!("../../cef-spotify/pages/driver.js");

fn flag(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    args.iter().find_map(|a| a.strip_prefix(&prefix).map(str::to_string))
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == &format!("--{name}"))
}

fn main() -> wry::Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let page = flag(&argv, "spike-page").unwrap_or_else(|| "sdk".into());
    let hidden = has(&argv, "spike-hidden");
    let extra = flag(&argv, "spike-args").unwrap_or_default();
    OPTS.set(Opts {
        client_id: flag(&argv, "spike-client-id").unwrap_or_default(),
        tracks: flag(&argv, "spike-tracks").unwrap_or_default(),
        autorun: has(&argv, "spike-autorun"),
        check: !has(&argv, "spike-nocheck"),
    })
    .ok()
    .expect("set once");
    println!(
        "[spike] {{\"kind\":\"start\",\"engine\":\"webview2\",\"page\":{page:?},\"hidden\":{hidden},\"args\":{extra:?},\"pid\":{}}}",
        std::process::id()
    );
    server::spawn();
    // `--spike-server-only`: just host the page and print logs, for an external browser (the
    // Linux candidate: an installed Chrome/Edge/Brave in a hidden app window).
    if has(&argv, "spike-server-only") {
        println!("[spike] {{\"kind\":\"server_only\",\"port\":{PORT}}}");
        loop {
            std::thread::park();
        }
    }

    let url = match page.as_str() {
        "web" => "https://open.spotify.com/".to_string(),
        _ => format!("http://127.0.0.1:{PORT}/sdk"),
    };
    // wry's own defaults (it replaces them when given arguments), then the SDK's autoplay need,
    // Local Network Access off (open.spotify.com posting to 127.0.0.1), then the extra levers.
    // `--spike-features-off=a,b` adds to the one --disable-features list (Chromium reads only one).
    let features_off = flag(&argv, "spike-features-off")
        .map(|f| format!(",{f}"))
        .unwrap_or_default();
    let browser_args = format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,LocalNetworkAccessChecks,PrivateNetworkAccessSendPreflights,PrivateNetworkAccessRespectPreflightResults,BlockInsecurePrivateNetworkRequests{features_off} \
         --autoplay-policy=no-user-gesture-required {extra}"
    );
    println!("[spike] {{\"kind\":\"browser_args\",\"args\":{browser_args:?}}}");

    let data_dir = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("mosaic-wv2-spike");
    let _ = std::fs::create_dir_all(&data_dir);
    let mut context = WebContext::new(Some(data_dir));

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("WebView2 Spotify spike")
        .with_inner_size(LogicalSize::new(1100.0, 800.0))
        .with_visible(!hidden)
        .build(&event_loop)
        .expect("window");

    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url.clone())
        .with_additional_browser_args(browser_args)
        // Every frame, including the SDK's sdk.scdn.co iframe.
        .with_initialization_script_for_main_only(INSTRUMENT, false);
    if page == "web" {
        builder = builder.with_initialization_script_for_main_only(DRIVER, true);
    }
    let _webview = builder.build(&window)?;
    println!("[spike] {{\"kind\":\"open\",\"url\":{url:?}}}");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}
