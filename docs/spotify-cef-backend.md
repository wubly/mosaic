# Spotify playback through an embedded CEF player

Status: **shipped in 0.6.2 for Windows (WebView2) and macOS (WKWebView, untested)** as
`crates/services/src/spotify/web/` plus `app/src/spotify_host.rs`; `native/DECISIONS.md` lists what
the implementation settled. Linux keeps librespot/Soloist. The Windows spike (section 14) ran first. Its results changed
the engine choice; see "Engine decisions" below. Where the rest of this document says "CEF",
read it as "the platform's web engine" behind one host interface. The process model, bridge,
player modes, tracker and error handling don't depend on the engine.

### Engine decisions (2026-11-26)

| Platform | Engine | Status |
|---|---|---|
| Windows | **Edge WebView2** (system runtime, via wry) | Tested: plays end to end, 171 MB idle / 184 MB playing, nothing to ship (section 2, WebView2 results) |
| macOS | **WKWebView** (system WebKit; FairPlay, which the SDK probes as `com.apple.fps.1_0`) | Untested. Shipped as a best effort, labelled experimental in Settings |
| Linux | **An installed Chromium-based browser** (Chrome, Edge, Brave, Vivaldi; they ship Widevine and AAC) loading the same player page as an app window kept out of sight, with its own profile folder. Fallback: **Soloist** (`spotify::soloist`), which works on Linux | Mechanism tested with Chrome 158 **on Windows** (see "Installed-Chrome results" below). Not yet tested on Linux |

### Installed-Chrome results (2026-11-26, Chrome 158.0.8127.37 on Windows; stand-in for Linux)

Test: `spikes/webview2-spotify/chrome-test.ps1`. The test server serves the SDK page. Chrome runs
with `--user-data-dir=<own folder> --app=http://127.0.0.1:8898/sdk`, plus the trimming switches.
It never touches the user's own Chrome profile.

| Mode | Plays | To sound | Seek | Track change | Processes | Private MB playing |
|---|---|---|---|---|---|---|
| `--headless=new` | yes | 1042 ms | 29–31 ms | 688–930 ms | 9 (ignores the renderer limit) | 253 |
| off-screen window, default | **no**: stuck at `loading:true` | — | — | — | 6 | 174 |
| off-screen window + no occlusion throttling ³ | **yes** | 1315 ms | 30–31 ms | 792–911 ms | 6 | **187** |

³ `--disable-backgrounding-occluded-windows --disable-renderer-backgrounding
--disable-background-timer-throttling`, and `CalculateNativeWinOcclusion` added to
`--disable-features`. Without them, Chrome throttles the window as occluded and the player never
leaves `loading`.

Notes for Linux:
- Headless mode reports `HeadlessChrome` in its user agent, which Spotify could start rejecting.
  Prefer a real window, or override the user agent if headless is ever needed.
- Off-screen positioning works on X11 and XWayland. Native Wayland clients can't place their own
  windows. Options to test there: `--ozone-platform=x11` (XWayland), or a minimized window with
  the same anti-throttling switches.
- Browser search order: Chrome, Chromium (only if its Widevine is installed), Edge, Brave,
  Vivaldi. Settings shows which one is used. If none is found, Soloist (if configured), else
  "install Chrome to play Spotify".
- Memory matches WebView2 (187 vs 184 MB playing). The cost is the same Chromium process model.

### Firefox-family results (2026-11-26, Zen 1.24.1b / Firefox 160 engine, on Windows)

Many Linux users run Firefox derivatives (Firefox, LibreWolf, Zen), so the Linux host must not
depend on Chromium. Test: `spikes/webview2-spotify/firefox-test.ps1`. Zen gets its own profile
whose `user.js` turns on EME, Widevine and autoplay there only. LibreWolf turns DRM off by default
in the user's profile; ours is separate.

- **First run:** Widevine is downloaded through Firefox's GMP updater into the new profile during
  the first run. Playback works only from the second start, as with CEF.
- **Formats:** Widevine is granted for AAC (`mp4a.40.2`, `mp4a.40.5`) and Opus. PlayReady and
  FairPlay aren't available. `addSourceBuffer` for AAC succeeds, the key session sends its
  licence request, and then:
- **Spotify's licence server answers 403** (`widevine-license/v1/audio/license`). WebView2 and
  Chrome got 200 on the same machine and account. **Likely cause (unconfirmed): Verified Media Path.**
  On Windows and macOS, licence servers can demand a VMP-signed browser. Mozilla signs official
  Firefox builds; third-party builds like Zen may not be. **Linux has no VMP**, so this says
  little about Firefox, LibreWolf or Zen on Linux. That needs a Linux test.
- **Headless memory:** with default settings, 913 MB across 15 processes, because Zen loads its
  whole browser UI. Limiting the profile to one content process (`dom.ipc.processCount=1`,
  `fission.autostart=false`, no prelaunch) wasn't measured, because playback never started.
- **Window mode:** with `--no-remote --profile` and a URL, Zen opened a window but never loaded
  the page. Not investigated.
- **Diagnosis tool:** Firefox has no host console hook, and posts from injected scripts to
  `127.0.0.1` were blocked ("CORS request did not succeed"). WebDriver BiDi works instead
  (`--remote-debugging-port`, `script.addPreloadScript` into every frame, `log.entryAdded`;
  see `bidi.mjs`). The Linux host can use the same channel if it needs logs or control.

Linux plan, given this:
- The host supports **both browser families**: the Chromium family (`--app`, off-screen,
  anti-throttling switches) and the Firefox family (own profile, `user.js`, `--headless` or a
  window). By default it uses the user's default browser if it's in either family. Settings
  can pick another.
- **Soloist stays** as the fallback when no supported browser is installed, or when the licence
  is refused.
- **Open:** whether Spotify licenses Firefox-family Widevine on Linux (expected to, since
  open.spotify.com works in Firefox on Linux), headless versus window on X11 and Wayland, and
  trimmed memory for both families.

Why not one engine: stock CEF can't decode AAC, which is the only format Spotify's SDK streams to
software DRM (section 2, F5). A codec-enabled CEF would mean building Chromium ourselves every
release. On Windows it would also be heavier than WebView2 and ship 405 MB.

Consequences:
- **Soloist stays, on Linux only** (and only as the fallback), instead of being removed in M5.
  On Windows, Soloist has been broken in practice, so Windows doesn't offer it.
- The host (section 3.1) becomes a small per-platform binary with one job: open the bridge page
  in a hidden web view. WebView2 and WKWebView both come through wry, so that's one code path.
  The Linux browser host is a launcher for an external process instead of an embedded view.

## 1. Goal

Play Spotify tracks for every account: Premium, free, and newer accounts that Spotify refuses
librespot keys to (`playback::KEY_REFUSED`, librespot issue #1649). No user-supplied daemon
(Soloist), and no 90-day expiring builds.

The audio is decrypted by a Widevine CDM inside a Chromium Embedded Framework (CEF) browser that
Mosaic starts hidden. That is the same licensed path a desktop browser uses. Mosaic never sees
decrypted audio samples, and this design never asks it to.

What it replaces:

| Today | After |
|---|---|
| Soloist backend (`spotify::soloist`), settings section, API key | Removed once the CEF backend ships (milestone M5) |
| librespot backend | Kept behind the `spotify` feature as an opt-in for accounts that still get keys: in-process, ~20 MB, full EQ and visualizer. Not the default |
| Web API client (`spotify::auth`, `web_api`) | Unchanged, plus the `streaming` scope (section 6.1) |

What it won't do:
- Block, skip or mute ads. Free accounts hear Spotify's ads, as in any browser.
- Give free accounts features Spotify limits to Premium.
- Capture decrypted audio and play it back through Mosaic's engine. EQ, leveling and crossfade
  therefore don't apply to Spotify tracks (section 9).

## 2. Facts this design relies on

✓ = established. † = unverified; the spike (section 14) settles each one.

| # | Claim | Status |
|---|---|---|
| F1 | The Web Playback SDK plays only for Premium accounts. Free accounts get `account_error` | ✓ (Spotify docs) |
| F2 | open.spotify.com plays for free accounts, with ads | ✓ |
| F3 | Stock CEF fetches the Widevine CDM through Chromium's component updater | † (needs CEF built with `enable_widevine`; check the Spotify-hosted CEF builds) |
| F4 | Spotify plays in a CEF browser without VMP signing on Windows/macOS | † **highest risk** |
| F5 | The CEF build can decode the codec Spotify serves to it (AAC in MP4, or what it falls back to) | † |
| F6 | A page at `http://127.0.0.1:<port>` counts as a secure context, so EME and the SDK work there | ✓ (Chromium treats loopback as potentially trustworthy) |
| F7 | Script injected into open.spotify.com can open `ws://127.0.0.1:<port>` once Local Network Access checks are turned off by flag | † |
| F8 | The `cef` crate (tauri-apps/cef-rs) covers what the host needs: off-screen or hidden windows, command-line switches, a visible window for sign-in, frame `execute_java_script`, load events | † (check docs.rs for the resolved version before writing code) |
| F9 | Per-process loopback capture works for CEF's audio service process (Windows `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`, Windows 10 2004+) | † |

### Spike results so far (2026-11-26, Windows 11, cef 156.7.0 / Chromium 156, stock build)

| # | Result |
|---|---|
| F3 | ✓ The component updater downloads and registers Widevine CDM 4.10.3112.0 (23 MB) into `root_cache_path/WidevineCdm`. Without `--component-updater=fast-update`, the first check can take minutes. With it, the check runs about 10 s after start. The CDM only becomes usable after a restart. Mosaic must plan a first-run step: start the host, wait for the CDM, restart the host |
| F4 | partly: `requestMediaKeySystemAccess("com.widevine.alpha")` and `createMediaKeys()` succeed, including with `SW_SECURE_CRYPTO`, on an unsigned build. Signed in with Premium, the SDK connects and registers a device, and the Web API play returns 204. The licence step is never reached, because playback fails first (F5) |
| F5 | ✗ **Stock CEF can't play Spotify. The SDK streams AAC only.** From the instrumented play (`logs/playtest.log`): the SDK asks Widevine for FLAC at `HW_SECURE_DECODE`/`HW_SECURE_CRYPTO` (lossless, hardware-backed only), and for AAC (`mp4a.40.2`, `mp4a.40.5`) and WebM Opus at `SW_SECURE_CRYPTO`. Stock CEF grants only Opus. The device registration (`track-playback/v1/devices`) still comes back with `file_format_filter: 15416` (bits 3–5 and 10–13: MP3 and MP4/AAC formats), storage-resolve hands out format 11 (MP4/AAC), and `addSourceBuffer('audio/mp4; codecs="mp4a.40.2"')` fails. Opus is never offered as a file, and FLAC needs hardware-backed DRM that an unsigned L3 build can't provide. **CEF playback needs AAC: our own CEF build with `proprietary_codecs=true ffmpeg_branding="Chrome"` (11.3), or a different engine** |
| Licence | Spotify's licence server answered the SDK's certificate request (`spclient.wg.spotify.com/widevine-license/v1/application-certificate`, 200). Whether it issues a licence to an unsigned L3 CDM is still unknown, because no AAC segment could be appended. That needs a codec-enabled build |
| SDK | The player reports a different URI from the one requested (`4uLU6hMCjMI75M1A2tKUQC` → `0yNttAVwMr39qyODHNIkrY`): Spotify relinks tracks to the market's version. The tracker (6.3) must treat the requested URI and its relinked URI as the same track (`linked_from`), or it would report "moved on" immediately. The SDK also probes `com.microsoft.playready.recommendation` (PlayReady through Media Foundation), which logs warnings in CEF |
| Memory | See "Memory levers" below. The first signed-in run showed 7 processes and 365 MB private / 427 MB working set. Part of that came from the test page's own Widevine check, which starts a second CDM process |
| Sign-in | Spotify showed a reCAPTCHA challenge in the CEF sign-in window. The user solved it, and that has to stay the user's job (7.2) |
| F6 | ✓ `isSecureContext` is true at `http://127.0.0.1:8898` |
| F7–F9 | pending |
| Other | The runtime is 405 MB on disk with en-US locales only (`libcef.dll` takes most of it), above the 11.2 estimate. Chrome runtime style restores previous tabs from the profile, so the host must use Alloy style (`RuntimeStyle::ALLOY`). Building `cef-dll-sys` needs CMake and Ninja (both ship with VS Build Tools) |

### Memory levers (2026-11-26, SDK page, player connected and idle, Premium account)

Measured with `spikes/cef-spotify/mem-levers.ps1` and `mem-breakdown.ps1`: summed over the host's
process tree, 15 s after the player reports `ready`. Each row adds one lever to the one above.
These runs include the test page's capability check, which starts a second 68 MB CDM process.

| Config | Processes | Private MB | Working set MB |
|---|---|---|---|
| no trimming | 9 | 314 | 548 |
| base trimming (section 8 without the levers below) | 8 | 304 | 528 |
| + `iso` (site isolation off) | 7 | 285 | 474 |
| + `gpu` (`--in-process-gpu`) | 6 | 274 | 454 |
| + `net` (network service in-process) | 5 | 261 | 742 ¹ |
| + `audio` (audio service in-process) | 5 | 262 | 743 |
| + `lowend` (`--enable-low-end-device-mode`) | 5 | 260 | 743 |
| + `v8` (`--lite-mode`, small heaps) | 5 | 256 | 739 |

Per process, every lever except `net`, and no page check (the realistic host):

| Process | Private MB | Working set MB |
|---|---|---|
| browser | 88 | 165 |
| Widevine CDM (utility, `CdmServiceBroker`) | 68 | 86 |
| renderer (page + SDK iframe) | 27 | 65 |
| network service (utility) | 15 | 33 |
| storage service (utility) | 7 | 16 |
| **total** | **205** | **365** |

Conclusions:
- **Recommended set:** `iso`, `gpu`, `audio`, `lowend`, `v8`, plus the base trimming. Leave `net` off.
  ¹ An in-process network service saves 10–15 MB private, but the browser's working set grows by
  about 320 MB (165 → 485 MB, apparently mapped cache files). That looks bad in Task Manager's
  "Memory" column and isn't worth it.
- **The Widevine CDM is the largest fixed cost (68 MB)** and can't be trimmed. Only one MediaKeys
  may exist, so the host must never probe Widevine itself; it should rely on the player's own
  errors.
- The SDK page and iframe in one renderer cost about 27 MB. The page isn't the problem: the
  process model is.
- **Idle target, realistic:** about 200 MB private across 5 processes. Playback adds audio
  buffers (to be measured). Idle shutdown (7.3) frees all of it whenever Spotify isn't playing,
  so it matters most.
- Not pursued: `--single-process`, which CEF supports only for debugging. A custom CEF build
  might shave some of the browser's 88 MB, but no switch does.

### WebView2 results (2026-11-26, Windows 11, Edge WebView2 runtime 158.0.4370.33)

Test: `spikes/webview2-spotify` (wry 0.58 + tao 0.38), with the same SDK page, server and
instrumentation as the CEF test. Premium account, hidden window.

**Spotify plays end to end.**
- WebView2 has AAC (`mp4a.40.2` through Windows' decoders).
- It grants Widevine (AAC, FLAC, Vorbis, Opus) and PlayReady (AAC).
- The SDK picks Widevine and gets format 11 (MP4/AAC). Spotify's licence server issues a licence
  (`widevine-license/v1/audio/license` → 200), the key status is `usable`, and the media
  element plays.
- The runtime ships with Windows 10/11 and updates itself, so Mosaic ships 0 MB of browser.

| Measure | Result |
|---|---|
| Time to sound, cold (Web API play → live position > 200 ms) | 961–992 ms |
| Seek (state at the new position; `waiting` while it buffers) | 32–34 ms |
| Pause / resume | 33 / 34 ms |
| Track change (×3) | 783–810 ms |
| End of a single-URI play | `paused:true, position:0`, same (relinked) URI, `previous_tracks` empty |

Memory, summed over our exe and the `msedgewebview2.exe` processes on our profile folder:

| Config | Processes | Private MB idle | Private MB playing |
|---|---|---|---|
| default arguments | 9 / 11 | 266 | 384 (GPU process grows to 138 MB) |
| trimmed ² | 7 | **171** | **184** |

² `--renderer-process-limit=1 --in-process-gpu --disable-gpu --disable-gpu-compositing
--disable-site-isolation-trials --enable-low-end-device-mode --js-flags=--lite-mode
--disable-extensions --disable-spell-checking --disable-sync --no-first-run` with
`IsolateOrigins,site-per-process,AudioServiceOutOfProcess` added to the one `--disable-features`
list. The arguments only apply when the WebView2 environment is created.

Trimmed, while playing: CDM 70, browser 44, renderer 39, network 15, storage 8, our exe 5,
crashpad 3 (MB private).

Corrections this forces on the design:
- **6.3, position:** the SDK's `state.position` is a snapshot taken at `state.timestamp`. While
  playing, the live position is `position + (now − timestamp)`. The tracker must anchor it, as
  Soloist's `Anchor` does, and never poll it as if it were live.
- **6.3, end of track:** a single-URI play ends as `paused:true, position:0` on the same track,
  with an empty `previous_tracks` list. Detect it as: started, then paused at 0 without a pause
  from Mosaic.
- **6.3, relinking:** match the requested URI against `current_track.uri` *or*
  `current_track.linked_from.uri`.
- **The engine choice (3.1, 11):** on Windows, WebView2 beats stock CEF on every count. It plays
  (CEF can't without our own codec build), it's lighter (171 vs 205 MB idle), and it ships
  nothing (vs 405 MB). The single-engine goal now costs either a custom CEF build on every
  platform, or one engine per platform: WebView2 on Windows, a codec-enabled CEF on Linux (no
  system engine there has Widevine), and WKWebView on macOS (the SDK also probes FairPlay,
  `com.apple.fps.1_0`; untested).

If F4 fails, the fallback is VMP-signing our CEF build with castlabs' EVS signing service (free
for development; licence terms to check before release). If F3 or F5 fails, we need our own CEF
build (section 11.3).

## 3. Architecture

```
┌──────────────────────── mosaic (iced app) ────────────────────────┐
│ App ── Effect::ResolveStream ──► resolve.rs ──► SpotifyPlayback   │
│                                                   │ ::Web(WebPlayer)
│ deck ◄── PushSource ◄── silent clock feeder ◄─────┤               │
│                                                   │               │
│ services::spotify::web                            │               │
│   bridge: 127.0.0.1 HTTP + WebSocket (random port, secret)        │
│   tracker: state machine, end-of-track, errors    │               │
│   host: spawns/kills mosaic-spotify-host, idle timer              │
└──────────────┬──────────────────────────────────────▲─────────────┘
               │ spawn (port, secret, mode, cache dir) │ ws events
┌──────────────▼──────────── mosaic-spotify-host ─────┴─────────────┐
│ CEF browser process (hidden window, trimmed switches)             │
│   page A: http://127.0.0.1:<port>/sdk     (Premium; Web Playback SDK)
│   page B: https://open.spotify.com + injected driver (free)        │
│   sign-in window (visible only when needed)                        │
│ CEF children: renderer, GPU (disabled), utility (audio, CDM)       │
└────────────────────────────────────────────────────────────────────┘
                       │ audio: CEF's own output → system mixer
```

### 3.1 Why a separate host process

- **Main threads:** CEF wants to own the main thread and its message loop. iced/winit wants the
  same. A separate process avoids running two UI loops in one.
- **Crash isolation:** a CDM or renderer crash takes down the host, not Mosaic. Mosaic restarts
  it (section 10).
- **Memory:** killing the host frees all of Chromium's memory. Mosaic starts it on the first
  Spotify play and stops it after `IDLE_SHUTDOWN` (default 10 min) without Spotify playback.
- **Build:** the app crate never links CEF. A build without the `spotify-cef` feature, or a
  machine without the CEF runtime, still builds and runs. Spotify tracks then fall back as
  described in section 6.4.

### 3.2 Why the bridge lives in Mosaic, not the host

The host has as little logic as possible. It opens a URL, injects one script, shows or hides a
window and reports lifecycle events. Every protocol detail sits in the pages' JavaScript and in
`services::spotify::web`: the SDK calls, Web API calls, DOM selectors and state parsing. When
Spotify changes something, the fix is a JavaScript or Rust change in Mosaic, and the CEF
binding code (the most expensive code to maintain) stays put.

## 4. Crates and files

```
native/
  crates/spotify-host/          # new bin crate `mosaic-spotify-host`, depends on `cef`
    src/main.rs                 # CEF init, subprocess dispatch, args
    src/browser.rs              # hidden browser, sign-in window, load handler, script injection
    src/switches.rs             # trimming switches (section 8), one list, documented
  crates/services/src/spotify/web/
    mod.rs                      # WebPlayer: the SpotifyPlayback::Web surface
    bridge.rs                   # HTTP + WebSocket server, auth, message routing
    host.rs                     # spawn, health check, restart, idle shutdown
    tracker.rs                  # state machine (pure, unit-tested)
    protocol.rs                 # serde types for every message (section 5)
    pages/sdk.html              # page A (include_str!)
    pages/sdk.js
    pages/webplayer-driver.js   # injected into open.spotify.com (page B)
  crates/services/src/spotify/clock.rs  # silent push-source feeder, moved out of soloist.rs
```

Feature `spotify-cef` on `mosaic-services` (and forwarded by `app`) gates `spotify::web`.
`services` doesn't depend on `cef`; it only spawns the host binary by path. The host binary sits
next to `mosaic` in builds and installers (section 11).

Dependencies:
- `cef` (host only): the maintained Rust binding. Pin the exact version; its CEF version
  decides the Chromium version.
- `tokio-tungstenite` gains its `handshake` feature (server side). It's already a dependency
  for Soloist.
- No HTTP server crate. The bridge serves two static pages and one WebSocket upgrade, which a
  small hand-written handler on `tokio::net::TcpListener` covers. Pages are `include_str!`.

## 5. Bridge protocol

The bridge listens on `127.0.0.1:0` (a random port). Each run makes a 32-byte random secret. The
host gets both as arguments, and pages get them in the URL fragment (`#s=<secret>`), which never
goes over HTTP. The first WebSocket message must be `hello` with the secret, otherwise the socket
is closed. HTTP requests must carry `?s=<secret>`. Requests whose `Origin` isn't
`http://127.0.0.1:<port>` or `https://open.spotify.com` are refused.

JSON messages, each with a `type`. Types live in `protocol.rs`. The page side mirrors them by
hand. A `v` field (protocol version) in `hello` lets Mosaic refuse a mismatched page.

Mosaic → page:

| type | fields | meaning |
|---|---|---|
| `token` | `access_token`, `expires_at` | reply to `need_token` (SDK mode) |
| `play` | `uri`, `position_ms` | free mode: open and start this track. SDK mode starts tracks over the Web API instead (6.1) |
| `pause` / `resume` | — | |
| `seek` | `position_ms` | |
| `volume` | `value` 0..1 | the player's own volume; Mosaic's deck volume can't reach this audio |
| `next` | — | Spotify's own next track (used when it carries on by itself) |
| `ping` | `id` | health check |

Page → Mosaic:

| type | fields | meaning |
|---|---|---|
| `hello` | `secret`, `v`, `mode` (`sdk`/`web`) | |
| `need_token` | — | SDK's `getOAuthToken` fired |
| `ready` | `device_id` (SDK) | player usable |
| `state` | `uri`, `paused`, `position_ms`, `duration_ms`, `ts`, `loading`, `ad` (bool), `next_uris` | every state change, and every 1 s while playing |
| `ended` | `uri` | page-side end detection (6.3) |
| `signed_out` | — | page landed on a sign-in page or got `authentication_error` |
| `error` | `kind` (`account`, `playback`, `drm`, `init`, `driver`), `message` | |
| `pong` | `id` | |

The host also prints line-delimited JSON on stdout: `started`, `page_loaded {url}`,
`renderer_crashed`, `cdm_missing`, `sign_in_shown`, `sign_in_closed`. `host.rs` reads them. They
cover failures that happen before any page can connect.

## 6. Player modes

`/me` → `product` (already fetched by `spotify_status`) picks the mode: `premium` uses SDK mode,
anything else uses web-player mode. Settings can force either mode for testing.

### 6.1 SDK mode (Premium)

- Page A loads `https://sdk.scdn.co/spotify-player.js` and creates
  `new Spotify.Player({ name: "Mosaic", getOAuthToken, volume })`.
- `getOAuthToken(cb)` sends `need_token`. Mosaic answers with `spotify_token()` (the existing PKCE
  token, refreshed as now). Change needed: add `streaming` to `auth::SCOPES` and turn the
  `!url.contains("streaming")` assertion in `auth.rs` tests into a positive one. Existing users
  are asked to reconnect once, because a refreshed token can't gain a scope. `spotify_status`
  flags a token without `streaming` so the UI can say so.
- After `ready {device_id}`, Mosaic starts tracks with Web API
  `PUT /me/player/play?device_id=<id>` and body `{ "uris": [uri], "position_ms": n }`. Each call
  sends exactly one URI, so Mosaic's own queue stays in charge.
- Pause, resume, seek and volume go to the SDK in the page (`player.pause()` and so on). That
  costs no Web API calls.
- State comes from `player_state_changed` plus a 1 s `getCurrentState()` poll while playing. The
  poll keeps `position_ms` fresh, because the event doesn't fire continuously.
- Errors: `account_error` means not Premium, so Mosaic switches to web-player mode and retries
  once. `authentication_error` means the token was refused, so Mosaic refreshes it once and
  otherwise reports `signed_out`. `playback_error` fails the current track.
- Autoplay: when the single URI ends, Spotify may continue with recommendations. The tracker
  treats a different `uri` after ours as "ended" (6.3) and sends `pause` unless Mosaic adopts it.

This mode uses Spotify's documented SDK and Web API only.

### 6.2 Web-player mode (free, and the fallback)

- The host loads `https://open.spotify.com/` and injects `webplayer-driver.js` on every main-frame
  load (`OnLoadEnd` → `frame.execute_java_script`). The driver connects to the bridge (F7).
- **Starting a track:** the driver navigates in-app to `/track/<id>`, using the History API plus
  a `popstate` so the player isn't reloaded, then clicks the page's play button. It confirms
  that the now-playing URI matches. `position_ms > 0` is applied by a seek once it plays.
- **State:** read from the now-playing bar (track link, progress bar `aria-valuenow`/`max`,
  play/pause button label), observed with a `MutationObserver` and sampled every 500 ms. An ad is
  detected when the now-playing link isn't a `/track/` link or the ad marker is present. The
  state then carries `ad: true`. The ad plays normally: the tracker holds Mosaic's clock, and the
  player bar shows "Advertisement".
- **Controls:** pause, resume and next click the page's buttons. Seek and volume set the
  progress and volume sliders by dispatching pointer events at the right fraction.
- **Selectors:** every selector and text match sits in one `SELECTORS` table at the top of the
  driver, with fallbacks (`data-testid` first, then `aria-label`). When none match within 10 s,
  the driver sends `error {kind: "driver"}` naming the selector. That makes a Spotify frontend
  change show up as one clear error that a one-line edit fixes.
- **Sign-in:** a load that ends on `accounts.spotify.com`, or a page without a logged-in user
  menu, sends `signed_out`. Mosaic then asks the host to show the sign-in window (section 7.2).

This mode drives Spotify's own web interface. It's fragile by nature (any frontend release can
break a selector) and it's automation Spotify's terms don't permit. It's the only way a free
account plays in Mosaic. Mosaic labels it that way in Settings.

### 6.3 End-of-track and "moved on"

This mirrors the Soloist `Tracker` (`soloist.rs`) so the app-side handling (`Target::moved`,
`ExternalEvent`) is unchanged:
- `started` becomes true when a `state` arrives with our `uri`, not paused and not loading.
- **Finish** when, after starting:
  - `uri` changes to another track: Spotify moved on, so call `moved(uri)` then `finish()`. The
    app adopts the new track or loads its own next one, which replaces it.
  - `paused` with `position_ms == 0` and our `uri` in the previous-tracks list (SDK), or
    `ended` from the driver: call `finish()`.
- **Fail** if not started within `LOAD_TIMEOUT` (20 s), or on `error` before starting.
- Ads (web mode) neither finish nor fail. The load timeout pauses while `ad` is true.

### 6.4 Backend selection

`Services::spotify_playback()`:
1. If the `spotify-cef` feature is on and the host binary is found, use `Web`.
2. Otherwise, if the `spotify` feature is on, use `Librespot`. `KEY_REFUSED` then tells the user
   to install the Spotify player component.
3. Otherwise, Spotify tracks show as unavailable, as today without features.

Settings can force librespot ("Play in-process (older accounts only)").

## 7. The host binary

### 7.1 Arguments

```
mosaic-spotify-host --bridge=127.0.0.1:<port> --secret-file=<path> --mode=sdk|web
                    --profile=<data>/spotify-web --log=<logs>/spotify-host.log
```

The secret is written to a file only the user can read and deleted after the host reads it,
because it must not appear on the command line (the same reason Soloist's key went through
`setup-api-key`). CEF subprocesses are the same binary: `main` calls `cef::execute_process` first
and returns for child processes.

### 7.2 Windows

- **Player window:** a real but never-shown top-level window, or windowless (off-screen
  rendering) if F8 shows OSR keeps audio and EME working with less GPU work. The spike decides
  this. It's never focusable and never in the taskbar.
- **Sign-in window:** a normal visible 480×720 window on `accounts.spotify.com/login?continue=…`,
  titled "Sign in to Spotify – Mosaic". It closes itself when the browser reaches
  `open.spotify.com` signed in, then reports `sign_in_closed`. The session cookie stays in
  `--profile` (CEF `root_cache_path`), so sign-in survives restarts. When Spotify expires the
  session, the next play shows the window again. This is the "contract" model: one sign-in,
  reused until Spotify ends it.
- SDK mode needs no browser sign-in. Its token comes from Mosaic.

### 7.3 Lifecycle

- **Start:** lazily, on the first Spotify `load`, or from Settings → "Start player". Health is
  `started` on stdout within 15 s, then `hello` on the socket within 20 s.
- **Idle shutdown:** after `IDLE_SHUTDOWN` without a loaded Spotify track, `host.rs` closes the
  stdin pipe and the host quits cleanly. It's killed after 5 s if it doesn't.
- **Leftovers:** a host left over from a crashed Mosaic is found by its lock file in `--profile`
  and killed before starting a new one, as `soloist::kill_stale` does.
- **Exit:** Mosaic's exit kills the host. On Windows the host goes in a Job Object with
  `KILL_ON_JOB_CLOSE`. On Linux it uses `PR_SET_PDEATHSIG`.

## 8. Cutting CEF down

These are runtime-only changes, using stock CEF builds. Each switch sits in `switches.rs` with a
comment saying why, and the spike's memory runs check each one.

| Area | Switch / setting |
|---|---|
| Processes | `--renderer-process-limit=1`, `--disable-site-isolation-trials`, `--process-per-site` |
| GPU | `--disable-gpu`, `--disable-gpu-compositing`, `--disable-software-rasterizer` (nothing is drawn; OSR uses software) |
| Features | `--disable-extensions`, `--disable-pdf-extension`, `--disable-print-preview`, `--disable-spell-checking`, `--disable-component-extensions-with-background-pages`, `--disable-background-networking` (except the component updater, which the CDM needs; check F3), `--disable-sync`, `--disable-default-apps`, `--no-first-run`, `--mute-audio` **off** |
| Network access | `--disable-features=LocalNetworkAccessChecks,PrivateNetworkAccessSendPreflights,…` (exact names checked against the pinned Chromium; F7) |
| Memory | `--js-flags=--max-old-space-size=192`, `CefSettings.persist_session_cookies=1`, disk cache capped at 64 MB, `--disable-back-forward-cache` |
| Rendering | OSR at 1 fps (`windowless_frame_rate=1`) or a 1×1 hidden window, `--disable-renderer-backgrounding` **off** for page B, but timers must keep running, so check that background throttling doesn't stall the driver |
| Media | `--autoplay-policy=no-user-gesture-required` (the SDK needs it to start without a click) |

Expected memory (to be measured, not promised): page A about 120–180 MB total across processes,
page B about 250–400 MB, because open.spotify.com is a full single-page app. A custom CEF build
(section 11.3) only becomes worth it if those numbers are far off.

## 9. Visualizer, volume, EQ

- **Visualizer:** cava-style, read-only. On Windows, per-process loopback capture of the host's
  process tree (F9) at 48 kHz stereo goes into the existing FFT path in `mosaic-audio`.
  Linux uses a PipeWire monitor stream on the host's node. macOS uses ScreenCaptureKit audio
  capture (13+). The capture is analysed and dropped. It never goes to an output or a file. If
  capture fails or isn't permitted, the visualizer idles, as it does with Soloist today.
- **Volume:** Mosaic's volume slider sends `volume` to the player, as `Soloist::set_volume` does.
  Mute sends 0.
- **Normalization:** Spotify's own setting, shown in Mosaic as a link to the account's playback
  settings. The SDK has no switch for it.
- **EQ, leveling, crossfade:** not available for Spotify tracks. Settings says so. A system-wide
  EQ (Equalizer APO, PipeWire filter-chain) is the user's choice and needs nothing from Mosaic.

## 10. Errors and recovery

| Situation | Detected by | Response |
|---|---|---|
| Host binary or CEF runtime missing | spawn fails | backend falls back to librespot (6.4); Settings shows "Spotify player component not installed" |
| `cdm_missing` / `error{drm}` | host stdout / page | fail the track with "Spotify's protected-content module couldn't load"; Settings shows the detail. Not retried automatically (usually F3/F4) |
| Renderer crashed | `renderer_crashed` | host reloads the page once; a second crash within 60 s restarts the host; a pending track fails |
| Host exited | child wait | mark dead; the next `load` restarts it (same as `Soloist::alive`) |
| Socket closed, host alive | bridge | wait 5 s for a reconnect (a page reload reconnects by itself), then reload the page |
| `signed_out` (web) | page | pause, show the sign-in window, fail the pending track with "Sign in to Spotify to keep playing" |
| `authentication_error` (SDK) | page | refresh the token once; then fail with the reconnect message |
| `account_error` (SDK) | page | switch to web mode and retry the same load once |
| `error{driver}` | page | fail the track with "Spotify's web player changed; Mosaic needs an update", naming the selector in the log |
| Network down | Web API / page errors | fail the track; the next load tries again; no retry loop |
| Ad playing (web) | `state.ad` | hold the clock, show "Advertisement", pause the load timeout |

The host's log has its own file (`spotify-host.log`, rotated at 5 MB). The Settings status line
comes from `WebPlayer::describe()`.

## 11. Packaging

### 11.1 Layout

`mosaic-spotify-host(.exe)` plus the CEF runtime (`libcef`, `chrome_elf`, `icudtl.dat`,
`*.pak`, `locales/en-US.pak` only, `v8_context_snapshot.bin`) go in a `spotify/` folder next to
`mosaic`. The Widevine CDM isn't shipped. The component updater downloads it into the profile on
first use (F3).

### 11.2 Size

The stock CEF minimal distribution is roughly 250–300 MB unpacked on Windows (unverified). To keep
the main installer small, the player is an optional component: installers offer it as a checkbox,
and an in-app "Install Spotify player" downloads a pinned, checksummed archive from the project's
own GitHub release. That needs an explicit click, and the download size is shown first. Linux
AppImage/.deb ship it as a separate package.

### 11.3 Our own CEF build (only if needed)

This is only needed for F3/F5 failures or the memory target. GN args: `proprietary_codecs=true
ffmpeg_branding="Chrome" enable_widevine=true enable_pdf=false enable_printing=false
enable_extensions=false enable_spellcheck=false`. It builds in CI on a schedule tracking CEF's
stable branch. That's several hours per platform per Chromium release, which is the main
maintenance cost this design avoids by default.

### 11.4 VMP (only if F4 fails)

On Windows and macOS: sign the host and CEF binaries with castlabs EVS in the release workflow,
after code signing. Development builds use the development VMP signature.

## 12. Security

- The bridge binds to 127.0.0.1 only, uses a per-run secret and checks `Origin`, so another local
  user or site can't drive playback.
- The OAuth token is sent only over the authenticated socket, never in URLs or logs.
- The CEF profile (cookies) lives in `<data>/spotify-web/`, with user-only permissions on Unix.
  "Disconnect Spotify" in Settings deletes it.
- The CEF sandbox stays on (Windows: `cef_sandbox` with the host as the sandbox target).
- Only open.spotify.com, accounts.spotify.com, sdk.scdn.co, the bridge origin and Spotify's
  CDN/API hosts may load in the main frame. Other navigations open in the user's own browser.
  Popups are blocked.

## 13. App changes

- `resolve.rs`: unchanged. It already calls `spotify_playback()` and `load`.
- `SpotifyPlayback::Web(WebPlayer)`: implements `load`, `pause`, `resume`, `seek`, `stop`,
  `set_volume`, `queue` (from `next_uris`), `skip_next`, and `describe`, matching the Soloist
  semantics.
- `external.rs`: the "Spotify moved on" event loses its Soloist wording and works for both
  backends.
- Settings → Spotify: the Soloist section becomes "Spotify player":
  - status line
  - mode (Automatic / Premium SDK / Web player)
  - Sign in to Spotify (web mode)
  - Install player (when missing)
  - Stop player now
  - a note that EQ, leveling and crossfade don't apply and that free accounts hear ads
- Player bar: shows "Advertisement" while `ad`. Seek is disabled during ads.
- Soloist code, settings and `soloist_fake.rs` are deleted in M5, after the CEF backend has passed
  validation.

## 14. Windows spike (first)

This is a throwaway binary under `native/spikes/cef-spotify/`, not in the workspace members and
not shipped. It answers F3–F9 before any of the above is built.

1. Use `cef` to start CEF with the section 8 switches, then open a visible window.
2. Run `chrome://components` and check whether the Widevine CDM installs (F3).
3. **Page A:** serve an SDK page from 127.0.0.1. Paste a token from the existing Mosaic PKCE flow
   with `streaming` (a one-off build with the scope added). Start one track over the Web API, and
   record whether audio plays or which error comes back (F4, F5, F6).
4. **Page B:** open open.spotify.com, sign in, play a track, and run a stub driver that opens a
   socket to 127.0.0.1 and logs state (F7).
5. Hide the window, or switch to OSR, and repeat 3–4. Audio must continue.
6. Seek, pause/resume, and change tracks three times. Log the state-event timing.
7. **Measure:** private working set summed over the process tree (`Get-Process` by job), CPU %
   over 60 s of steady playback, and time from `play` to the first `state` with
   `paused=false`. Record each with all switches, and with each switch group removed.
8. **Loopback:** capture the host's process tree and print RMS once a second (F9).
9. **Record results** in this document's table (section 2) and in `DECISIONS.md`.

**Stop rule:** if F4 fails with no clear error to act on, stop and decide between VMP signing
(11.4) and dropping the approach before building M1.

## 15. Milestones

| M | Content | Done when |
|---|---|---|
| M0 | Spike (section 14) | F3–F9 answered, numbers recorded |
| M1 | `clock.rs` extracted from Soloist (no behaviour change); `protocol.rs`, `tracker.rs` with unit tests mirroring the Soloist tracker tests | `cargo test` green; Soloist unchanged |
| M2 | `bridge.rs`, `host.rs`, `spotify-host` crate, SDK mode end to end | a Premium account plays, seeks, pauses, crosses three tracks; idle shutdown frees the memory |
| M3 | Web-player mode, sign-in window, driver, ad handling | a free account plays with ads; sign-out → sign-in → resume works |
| M4 | Visualizer loopback, Settings section, error table wired, packaging as optional component | installers build; missing-component path tested |
| M5 | Validation (section 16), Soloist removed, docs and `DECISIONS.md` updated | numbers published; Soloist code gone |

Linux and macOS follow M5. The host and switches are shared, so the remaining work per platform
is packaging, loopback capture and the F4/VMP outcome there.

## 16. Validation

Automated (`cargo test`, no network):
- `tracker.rs`: start, start with seek, moved-on, autoplay, ended (SDK and driver forms), ads,
  load timeout, error before and after start, stale-generation feeds.
- `bridge.rs`: wrong secret, wrong origin, protocol-version mismatch, reconnect.
- `host.rs`: start timeout, crash, idle shutdown, leftover kill (with a fake host binary, like
  `soloist_fake.rs`).

Manual, on a real Premium and a real free account, scripted where the debug CLI allows:

| Measure | Method | Target (initial) |
|---|---|---|
| Time to sound | `load` → first `state` playing, 20 cold and 20 warm runs | warm < 1.5 s |
| Seek latency | `seek` → `state` at the new position | < 500 ms |
| Track change | end → next playing | < 1.5 s (SDK), < 3 s (web) |
| Recovery | kill renderer; kill host; drop network 30 s | resumes or fails with the right message |
| Memory | private working set, host tree, steady playback, 10 min | record; compare below |
| CPU | average % over 10 min steady playback | record; compare below |
| Idle cost | memory 30 s after idle shutdown | host tree gone |

Comparison on the same machine, same track, same measurement:
- the official Spotify desktop app
- open.spotify.com in Chrome/Edge (one tab)
- librespot (Mosaic's in-process backend, on an account that still gets keys)
- the Soloist backend (until removed)

Results go in a table in this file, with the date and versions.

## 17. Open questions

- F3–F9 (section 2).
- Whether Spotify's Developer Terms allow the SDK in a desktop app shell, and the Web API
  development-mode user limits for the user's own client ID.
- Whether the SDK player's `getCurrentState()` gives a usable next-tracks list for `queue()`, or
  `queue()` stays empty in SDK mode.
- How often the open.spotify.com selectors break in practice. Track it from M3 on, and drop web
  mode if it needs fixes more than monthly.
- Whether to keep librespot at all after M5, or drop it to simplify the build.
