//! Throwaway Windows test for docs/spotify-cef-backend.md, section 14.
//!
//! Opens one CEF browser on either the Web Playback SDK test page (served from 127.0.0.1:8898)
//! or open.spotify.com (with a stub driver injected), with the trimming switches from section 8.
//! Everything the pages find is printed to stdout as `[spike] {json}` lines.
//!
//! Flags (all optional):
//!   --spike-page=sdk|web     which page to open (default sdk)
//!   --spike-hidden           create the browser window without showing it
//!   --spike-full             skip the trimming switches (for memory comparisons)
//!   --spike-autorun          run the SDK test sequence as soon as the player is ready
//!   --spike-client-id=<id>   the Spotify app client id (Mosaic's, from config.json)
//!   --spike-tracks=<uris>    comma-separated track URIs for the sequence

mod server;

use cef::*;
use std::sync::OnceLock;

pub const PORT: u16 = 8898;

#[derive(Debug)]
pub struct Opts {
    pub page: String,
    pub hidden: bool,
    pub lean: bool,
    pub autorun: bool,
    pub client_id: String,
    pub tracks: String,
    pub levers: Vec<String>,
    /// Run the page's own Widevine/codec check (it starts CDM processes of its own).
    pub check: bool,
}

static OPTS: OnceLock<Opts> = OnceLock::new();

fn opts() -> &'static Opts {
    OPTS.get().expect("options are set before CEF starts")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    args.iter()
        .find_map(|a| a.strip_prefix(&prefix).map(str::to_string))
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == &format!("--{name}"))
}

/// The memory levers `--spike-levers=` can pick from (default: all). `--spike-full` drops them
/// all, and the base trimming too.
pub const LEVERS: [&str; 6] = ["iso", "gpu", "net", "audio", "lowend", "v8"];

/// Section 8 of the design doc. Each entry says what it is for.
fn switches() -> Vec<(String, Option<String>)> {
    let o = opts();
    let on = |lever: &str| o.lean && o.levers.iter().any(|l| l == lever);
    let mut disable = vec![
        // open.spotify.com (public) posting to 127.0.0.1 (the bridge): Local Network Access.
        "LocalNetworkAccessChecks",
        "PrivateNetworkAccessSendPreflights",
        "PrivateNetworkAccessRespectPreflightResults",
        "BlockInsecurePrivateNetworkRequests",
    ];
    let mut enable: Vec<&str> = Vec::new();
    let mut s: Vec<(&str, Option<String>)> = vec![
        // Diagnostics for F3: what the component updater does about the Widevine CDM.
        ("vmodule", Some("*component*=1,*widevine*=1,*cdm*=1,*update_client*=1".into())),
        // Check for components (the Widevine CDM) seconds after start, not minutes.
        ("component-updater", Some("fast-update".into())),
        // The SDK must start playing without a click in the page.
        ("autoplay-policy", Some("no-user-gesture-required".into())),
    ];
    if o.lean {
        s.extend([
            ("renderer-process-limit", Some("1".into())),
            ("process-per-site", None),
            ("disable-gpu", None),
            ("disable-gpu-compositing", None),
            ("disable-extensions", None),
            ("disable-pdf-extension", None),
            ("disable-print-preview", None),
            ("disable-spell-checking", None),
            ("disable-component-extensions-with-background-pages", None),
            ("disable-sync", None),
            ("disable-default-apps", None),
            ("no-first-run", None),
            ("disable-back-forward-cache", None),
            ("disk-cache-size", Some("67108864".into())),
        ]);
    }
    // iso: the sdk.scdn.co iframe shares the page's renderer instead of getting its own.
    if on("iso") {
        disable.extend(["IsolateOrigins", "site-per-process"]);
        s.push(("disable-site-isolation-trials", None));
    }
    // gpu: no separate GPU process (nothing is drawn that matters).
    if on("gpu") {
        s.push(("in-process-gpu", None));
    }
    // net: the network service runs inside the browser process.
    if on("net") {
        enable.extend(["NetworkServiceInProcess", "NetworkServiceInProcess2"]);
    }
    // audio: the audio service runs inside the browser process.
    if on("audio") {
        disable.push("AudioServiceOutOfProcess");
    }
    // lowend: Chromium's low-memory profile (smaller caches, aggressive discarding).
    if on("lowend") {
        s.push(("enable-low-end-device-mode", None));
    }
    // v8: no optimizing compiler, small heaps; the SDK's JS is tiny.
    s.push((
        "js-flags",
        Some(if on("v8") {
            "--lite-mode --max-semi-space-size=1 --max-old-space-size=96".into()
        } else {
            "--max-old-space-size=192".into()
        }),
    ));
    s.push(("disable-features", Some(disable.join(","))));
    if !enable.is_empty() {
        s.push(("enable-features", Some(enable.join(","))));
    }
    s.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

wrap_app! {
    struct SpikeApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let browser_process = process_type.map_or(true, |p| p.to_string().is_empty());
            let Some(cl) = command_line else { return };
            if !browser_process {
                return;
            }
            for (name, value) in switches() {
                let name = CefString::from(name.as_str());
                match value {
                    Some(v) => cl.append_switch_with_value(Some(&name), Some(&CefString::from(v.as_str()))),
                    None => cl.append_switch(Some(&name)),
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(SpikeBrowserProcess::new())
        }
    }
}

wrap_browser_process_handler! {
    struct SpikeBrowserProcess;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let url = match opts().page.as_str() {
                "web" => "https://open.spotify.com/".to_string(),
                _ => format!("http://127.0.0.1:{PORT}/sdk"),
            };
            let mut info = WindowInfo::default().set_as_popup(Default::default(), "CEF Spotify spike");
            if opts().hidden {
                const WS_VISIBLE: u32 = 0x1000_0000;
                info.style &= !WS_VISIBLE;
            }
            info.bounds = Rect { x: 80, y: 80, width: 1100, height: 800 };
            // Alloy: no browser UI and no session restore (Chrome style reopened old tabs).
            info.runtime_style = RuntimeStyle::ALLOY;
            let mut client = Some(SpikeClient::new());
            let settings = BrowserSettings::default();
            println!("[spike] {{\"kind\":\"open\",\"url\":\"{url}\"}}");
            browser_host_create_browser(
                Some(&info),
                client.as_mut(),
                Some(&CefString::from(url.as_str())),
                Some(&settings),
                None,
                None,
            );
        }
    }
}

wrap_client! {
    struct SpikeClient;

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(SpikeDisplay::new())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(SpikeLoad::new())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(SpikeLifeSpan::new())
        }
    }
}

wrap_display_handler! {
    struct SpikeDisplay;

    impl DisplayHandler {
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let message = message.map(CefString::to_string).unwrap_or_default();
            if message.starts_with("[spike]") {
                println!("{message}");
            } else {
                println!("[console] {}", message.chars().take(300).collect::<String>());
            }
            1
        }
    }
}

const DRIVER: &str = include_str!("../pages/driver.js");
const INSTRUMENT: &str = include_str!("../pages/instrument.js");

// Renderer side: runs `instrument.js` in every frame as soon as its script context exists,
// before the page's (or the SDK iframe's) own scripts.
wrap_app! {
    struct RendererApp;

    impl App {
        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(SpikeRenderer::new())
        }
    }
}

wrap_render_process_handler! {
    struct SpikeRenderer;

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let Some(context) = context else { return };
            let mut retval = None;
            let mut exception = None;
            context.eval(
                Some(&CefString::from(INSTRUMENT)),
                Some(&CefString::from("mosaic-spike-instrument.js")),
                1,
                Some(&mut retval),
                Some(&mut exception),
            );
        }
    }
}

wrap_load_handler! {
    struct SpikeLoad;

    impl LoadHandler {
        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            http_status_code: ::std::os::raw::c_int,
        ) {
            let Some(frame) = frame else { return };
            if frame.is_main() == 0 {
                return;
            }
            let url = CefString::from(&frame.url()).to_string();
            println!("[spike] {{\"kind\":\"load_end\",\"status\":{http_status_code},\"url\":{url:?}}}");
            if url.starts_with("https://open.spotify.com") {
                frame.execute_java_script(
                    Some(&CefString::from(DRIVER)),
                    Some(&CefString::from("mosaic-spike-driver.js")),
                    1,
                );
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let code = sys::cef_errorcode_t::from(error_code) as i32;
            let text = error_text.map(CefString::to_string).unwrap_or_default();
            let url = failed_url.map(CefString::to_string).unwrap_or_default();
            println!("[spike] {{\"kind\":\"load_error\",\"code\":{code},\"text\":{text:?},\"url\":{url:?}}}");
        }
    }
}

wrap_life_span_handler! {
    struct SpikeLifeSpan;

    impl LifeSpanHandler {
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            quit_message_loop();
        }
    }
}

fn main() {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = args::Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        eprintln!("could not read the command line");
        std::process::exit(1);
    };
    let is_browser = cmd_line.has_switch(Some(&CefString::from("type"))) != 1;

    // Child processes (renderer, GPU, utility) run here and never return to us.
    let mut renderer = RendererApp::new();
    let code = execute_process(Some(args.as_main_args()), Some(&mut renderer), std::ptr::null_mut());
    if !is_browser {
        std::process::exit(code);
    }

    let argv: Vec<String> = std::env::args().collect();
    let o = Opts {
        page: flag(&argv, "spike-page").unwrap_or_else(|| "sdk".into()),
        hidden: has(&argv, "spike-hidden"),
        lean: !has(&argv, "spike-full"),
        autorun: has(&argv, "spike-autorun"),
        client_id: flag(&argv, "spike-client-id").unwrap_or_default(),
        tracks: flag(&argv, "spike-tracks").unwrap_or_default(),
        check: !has(&argv, "spike-nocheck"),
        levers: flag(&argv, "spike-levers")
            .map(|l| l.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect())
            .unwrap_or_else(|| LEVERS.iter().map(|l| l.to_string()).collect()),
    };
    println!(
        "[spike] {{\"kind\":\"start\",\"page\":{:?},\"hidden\":{},\"lean\":{},\"autorun\":{},\"levers\":{:?},\"pid\":{}}}",
        o.page,
        o.hidden,
        o.lean,
        o.autorun,
        o.levers.join(","),
        std::process::id()
    );
    OPTS.set(o).expect("set once");
    server::spawn();

    let root = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("mosaic-cef-spike");
    let profile = root.join("profile");
    let _ = std::fs::create_dir_all(&profile);
    let settings = Settings {
        no_sandbox: 1,
        root_cache_path: CefString::from(root.to_string_lossy().as_ref()),
        cache_path: CefString::from(profile.to_string_lossy().as_ref()),
        persist_session_cookies: 1,
        log_file: CefString::from(root.join("cef.log").to_string_lossy().as_ref()),
        ..Default::default()
    };
    let mut app = SpikeApp::new();
    if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        eprintln!("CEF failed to initialize (another spike already running with this profile?)");
        std::process::exit(1);
    }
    run_message_loop();
    shutdown();
}
