# Mosaic native rewrite: implementation spec

This is the build spec for rewriting Mosaic (currently Electron + Next.js/React) as a native Rust desktop app with first-class Linux support, keeping Windows and macOS working.

It is written to be executed end to end by an autonomous coding agent. Read **all of section 0 and section 1** before writing code, then work the milestones in section 12 in order.

---

## 0. Ground rules for the implementing agent

1. **The JS code is the behavioral source of truth.** Whenever this spec says "port `X.js`", open that file and reproduce its logic exactly: same constants, same regexes, same edge cases, same strings shown to the user. This document tells you *where things go and how they fit together*; the JS tells you *what they do*. If the spec and the JS disagree on behavior, the JS wins unless the spec explicitly says "change" or "drop".
2. **Do not modify or delete anything outside `native/`** until milestone M13 (cutover). The Electron app must keep building and running throughout.
3. **Work on a branch named `native`.** Commit at the end of every milestone (and more often if useful) with a message like `native: M3 audio engine core`. End every commit message with:
   ```
   Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
   ```
   Never push and never open PRs.
4. **Every milestone has acceptance checks.** Run them. Do not move on while `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` or `cargo fmt --check` fail inside `native/`.
5. **Don't stop to ask questions.** When something is ambiguous, pick the option closest to the current Electron behavior, write the decision down in `native/DECISIONS.md` (one line per decision, with the milestone), and continue.
6. **Crate versions:** add dependencies with `cargo add <crate>` so you get the current release, then pin the resolved version in `Cargo.toml`. APIs named in this spec (especially iced, librespot, souvlaki, material-colors) may differ from what you get. Check docs.rs for the exact resolved version **before** writing code against a crate. Don't guess APIs from memory.
7. **No `unwrap()`/`expect()` on I/O, network or user data.** Use `anyhow` in binaries and services, and `thiserror` for typed errors in `core`/`audio`. Panics are only acceptable for programmer invariants.
8. **Keep the audio callback real-time safe:** no allocation, no locks that can block, no I/O, no logging inside the cpal callback.
9. **Keep idle CPU low.** No per-frame redraw subscription unless something is animating or playing.
10. **Code style:** small modules, mirroring the JS file layout where practical. No dead code, no commented-out code. Only add comments that explain non-obvious *why*.

---

## 1. Locked decisions

| Topic | Decision |
|---|---|
| UI toolkit | **iced** (latest release, ≥ 0.14), wgpu renderer with tiny-skia fallback. Multi-window via `iced::daemon`. |
| Audio | Our own engine: `symphonia` decode → `rubato` resample → per-deck ring buffers → mixer + DSP → `cpal` output. |
| Async/network | `tokio` (multi-thread runtime) + `reqwest` (rustls, json, stream, gzip). |
| Local tags | `lofty` (tags, embedded pictures, lyrics frames). |
| Images | `image` (decode/resize/blur), on-disk cache plus in-memory LRU of decoded `iced::widget::image::Handle`s. |
| Theme colors | `material-colors` crate (port of material-color-utilities). |
| Media controls | `souvlaki`: MPRIS on Linux (use the zbus backend feature so no libdbus is needed), SMTC on Windows, MPNowPlayingInfoCenter on macOS. This **replaces global media-key shortcuts** on all platforms. |
| Tray | Linux: `ksni` (StatusNotifierItem over D-Bus, pure Rust). Windows/macOS: `tray-icon`. Hidden behind a `platform::tray` trait. |
| Discord | `discord-rich-presence`. |
| Dialogs | `rfd` (uses the xdg-desktop-portal on Linux). |
| Single instance | `interprocess` local socket. A second launch sends `focus` and exits. |
| Spotify playback | **librespot** (behind cargo feature `spotify`, on by default), with a custom `Sink` that feeds our engine. The Web Playback SDK, Widevine and castlabs Electron all go away. |
| Spotify Web API | Keep the existing PKCE flow with the user's own client id (port of `electron/spotify/auth.js`). |
| YouTube | **Dropped** (needs a browser for playback). Stored YouTube tracks are shown as unavailable (see section 3.4). |
| Spotify import web-scrape fallback | **Dropped** (needs a browser). Use the embed endpoint (≤100 tracks). If the user is connected to Spotify Web API, use `/playlists/{id}/tracks` or `/albums/{id}/tracks` with pagination for the full list. |
| "Low memory mode" setting | **Dropped** (Chromium-specific). Ignore the `lowMemory` config key. |
| Live backdrop blur | Replaced by a pre-blurred ambient image plus translucent panel fills (section 9.3). |
| Packaging | `cargo-packager`: AppImage + .deb on Linux, NSIS on Windows, .app/.dmg on macOS. Also a Flatpak manifest and a Gentoo ebuild in `native/packaging/`. |
| App id | `app.mosaic.player` (same as now). Binary name `mosaic`. Version starts at `0.5.0`. |

---

## 2. Workspace layout

```
native/
  Cargo.toml                 # [workspace], shared [workspace.dependencies], release profile (lto = "thin", codegen-units = 1)
  DECISIONS.md
  rust-toolchain.toml        # stable
  assets/
    fonts/                   # Roboto Flex (OFL), Material Symbols Rounded (Apache-2.0) + their LICENSE files
    icon.png icon.svg icon.ico   # copied from ../build/
  crates/
    core/                    # pure logic, no I/O except storage.rs; heavily unit tested
      src/lib.rs
      src/model.rs           # Track, TrackId, Source, Playlist, HistoryEntry, PlayStat, Settings, Session, Store, Config
      src/storage.rs         # data dir resolution, atomic JSON read/write, debounced saver, migration from Electron
      src/library.rs         # in-memory Store ops (port of DataProvider api)
      src/queue.rs           # Queue + ops (port of queue/*.js)
      src/relevance.rs       # port of queue/relevance.js
      src/stats.rs           # port of library/Stats.js summarize()
      src/lyrics_timing.rs   # port of lyrics/timing.js
      src/format.rs          # time formatting (Controls.js fmt, library/time.js hm)
    audio/                   # playback engine, no UI or network knowledge beyond the Source trait
      src/lib.rs             # Engine handle, Command, Event
      src/output.rs          # cpal stream + mixer callback
      src/deck.rs            # per-deck state shared between decoder thread and callback
      src/decode.rs          # symphonia decode loop, opus fallback, ffmpeg fallback
      src/resample.rs        # rubato wrapper
      src/stretch.rs         # time stretch for speed != 1
      src/dsp/{eq.rs, compressor.rs, fade.rs, analyser.rs}
      src/source/{mod.rs, file.rs, http.rs, hls.rs, push.rs}
      tests/                 # offline render tests (see 6.9)
    services/                # all network/IO integrations, async, tokio
      src/lib.rs             # Services struct (cheap Clone, Arc inside)
      src/http.rs            # shared reqwest client, USER_AGENT
      src/soundcloud/{client.rs, api.rs, tracks.rs, silence.rs}
      src/lyrics/{parse.rs, lrclib.rs, genius.rs, service.rs}
      src/local/{scanner.rs, covers.rs}
      src/spotify/{auth.rs, web_api.rs, embed.rs, url.rs, matcher.rs, import.rs}
      src/spotify/playback.rs   # librespot session + sink (feature = "spotify")
      src/discord.rs
      src/update.rs
      src/images.rs          # URL/path -> cached, resized RGBA
      src/oauth.rs           # loopback server helper
    platform/
      src/lib.rs
      src/media_controls.rs  # souvlaki wrapper
      src/tray/{mod.rs, linux.rs, other.rs}
      src/single_instance.rs
      src/dirs.rs            # data/cache/config dirs + Electron legacy dirs
      src/open.rs            # open https URL in browser (only https allowed, like openExternalHttps)
  app/                       # the `mosaic` binary
    src/main.rs              # arg parsing, single instance, runtime, iced::daemon
    src/app.rs               # App state, Message, update, view dispatch, subscriptions
    src/player.rs            # PlayerController: port of PlayerProvider.js logic (UI-agnostic, unit tested)
    src/theme/{mod.rs, palette.rs, dynamic.rs, styles.rs}
    src/anim.rs              # Spring, Tween, AnimationClock
    src/widgets/             # wavy_slider, visualizer, lyrics_view, virtual_list, track_row, track_tile,
                             # rail, switch, chip, search_bar, toast, menu, resizer, icon, glass, skeleton
    src/views/               # discover, library, liked, history, stats, playlists, playlist_detail, local,
                             # queue, settings, spotify, now_playing, mini, onboarding, command_palette,
                             # import_dialog, lyrics_panel, lyrics_picker, player_bar
  packaging/
    linux/app.mosaic.player.desktop
    linux/app.mosaic.player.metainfo.xml
    flatpak/app.mosaic.player.yml
    gentoo/media-sound/mosaic/mosaic-0.5.0.ebuild
```

Dependency direction: `app → services, audio, platform, core`, `services → core`, `audio → (nothing internal)`, `platform → core`. `core` must not depend on tokio.

---

## 3. Data model and storage (crate `core`)

### 3.1 Files and directories

The native app stores its data in its **own** directories so it never fights a running Electron install:

| Kind | Linux | Windows | macOS |
|---|---|---|---|
| data (JSON) | `$XDG_DATA_HOME/mosaic` (`~/.local/share/mosaic`) | `%APPDATA%\mosaic\data` | `~/Library/Application Support/app.mosaic.player` |
| cache (covers, images) | `$XDG_CACHE_HOME/mosaic` | `%LOCALAPPDATA%\mosaic\cache` | `~/Library/Caches/app.mosaic.player` |

Use `directories::ProjectDirs::from("app", "mosaic", "mosaic")` and adjust only if the result differs from the table in a way that matters. Write the final mapping into DECISIONS.md.

Files in the data dir use **the same names and formats as Electron**:

| File | Content | Electron source |
|---|---|---|
| `mosaic-data.json` | the Store (3.3) | `electron/ipc/app.js` store:load/save |
| `mosaic-config.json` | the Config (3.2) | `electron/core/storage.js` |
| `local-cache.json` | scanner metadata cache | `electron/library/scanner.js` |
| `lyrics-choices.json` | `{ [trackId]: lrclibRecord }` | `electron/lyrics/service.js` |
| `spotify-auth.json` | `{ access, refresh, expires }` | `electron/spotify/auth.js` |
| `librespot/` | librespot credential + audio cache | new |

Covers go in `<cache>/covers/<key>.jpg` and `<key>-s.jpg`. Remote images go in `<cache>/images/<sha1(url)>.<ext>`.

Writes are **atomic**: write `<file>.tmp`, fsync, rename. Reads tolerate a missing or corrupt file by returning the default (same as `readJson` fallback).

### 3.2 Config (`mosaic-config.json`)

```rust
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub name: Option<String>,
    pub onboarded: Option<bool>,
    pub client_id: Option<String>,          // SoundCloud client_id cache
    pub close_to_tray: Option<bool>,        // default true
    pub genius: Option<bool>,
    pub beta: Option<bool>,
    pub spotify_client_id: Option<String>,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>, // preserve unknown keys (lowMemory, youtube*)
}
```

`config:reset` semantics: port `electron/ipc/app.js` `config:reset`. Keep `clientId`, `closeToTray === false`, `genius`, `beta` and `spotifyClientId`, and drop everything else.

### 3.3 Store (`mosaic-data.json`)

Port the `DEFAULTS` object in `components/data/DataProvider.js` exactly. Shape:

```jsonc
{
  "likes":     [Track],                           // newest first
  "playlists": [{ "id": "k3j2h1x9", "name": "…", "tracks": [Track] }],
  "history":   [{ "track": Track, "at": 1732000000000 }],   // newest first, cap 100
  "plays":     { "<trackId as string>": { "track": Track, "count": 3, "seconds": 615 } },
  "days":      { "2026-11-22": 1830 },            // local-date key → seconds listened
  "sources":   ["/home/u/Music"],
  "settings":  { … see below … },
  "session":   { "queue": [Track], "index": 0, "position": 42.3 }
}
```

Settings defaults (from DataProvider.js), with `#[serde(default)]` per field and unknown keys preserved through a flatten map:

```
autoplay=true crossfade=0 fadePause=true skipSilence=true ytVideo=false leveling=false
eqOn=false eqGains=[0,0,0,0,0] eqPreset="Flat" speed=1 volume=0.8 shuffle=false
repeat="off"|"all"|"one" lyricsOpen=true presence=true barHeight=100 sideWidth=360
```

Merging on load works like DataProvider: top-level defaults, then file values, with `settings` merged field by field.

### 3.4 Track

Tracks are persisted inside likes, playlists, history, plays and session. **Round-tripping must be lossless.**

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(untagged)]
pub enum TrackId { Num(u64), Str(String) }   // SoundCloud ids are JSON numbers; local:/sp:/yt: ids are strings
impl TrackId { pub fn key(&self) -> String }  // string form, used as map key in `plays` and `lyrics-choices`

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: TrackId,
    pub source: Source,                 // "soundcloud" | "local" | "spotify" | "youtube"; unknown → Source::Other(String)
    pub title: String,
    pub artist: String,
    #[serde(default)] pub album: String,
    #[serde(default)] pub artwork: String,   // URL, or for local tracks a cover path (see migration)
    #[serde(default)] pub thumb: String,
    #[serde(default)] pub duration: u64,     // milliseconds
    #[serde(default)] pub permalink: Option<String>,
    #[serde(default = "yes")] pub streamable: bool,
    #[serde(default)] pub folder: Option<String>,     // local only
    #[serde(default)] pub lrc_path: Option<String>,   // local only, see strip()
    #[serde(default)] pub uri: Option<String>,        // spotify
    #[serde(default)] pub artist_id: Option<String>,  // spotify
    #[serde(default)] pub album_id: Option<String>,   // spotify
    #[serde(default)] pub video_id: Option<String>,   // youtube (kept for round-trip only)
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
}
```

- **`strip()`** (DataProvider.js): when persisting a track, drop `lrcPath` unless `source == local`.
- **Local id:** `"local:" + hex(sha1(path.to_lowercase()))[..16]` where `path` is the absolute path string as the OS reports it. This must match `idForPath` in `electron/library/scanner.js` bit for bit so likes and plays carry over. Add a unit test with a known vector computed by running the JS: `node -e "console.log('local:'+require('crypto').createHash('sha1').update('/home/u/Music/a.mp3'.toLowerCase()).digest('hex').slice(0,16))"`.
- **Unavailable tracks:** `source == youtube` or unknown sources, and `source == spotify` when the `spotify` feature is off or the user isn't connected. These are rendered greyed out with tooltip "Not available in this version" (YouTube) or "Connect Spotify in Settings → Beta" (Spotify). They are skipped when stepping through the queue (step over them in the same direction; if everything is unavailable, stop).

### 3.5 Library operations

Port every method of the `api` object in `DataProvider.js` into `impl Store`: `set_setting`, `save_session`, `is_liked`, `toggle_like` (returns the new liked state), `create_playlist` (id = 8 random `[a-z0-9]` chars, name trimmed, empty → `"Untitled playlist"`), `rename_playlist`, `delete_playlist`, `add_to_playlist` (returns `false` if already present), `remove_from_playlist`, `reorder_playlist`, `record_play` (history: dedupe only the head entry, cap 100), `add_seconds` (also bumps `days[today]`), `clear_history`, `add_sources`, `remove_source`.

Persistence: `Saver` collects dirty flags and writes 600 ms after the last change (debounce), and also on app exit (flush). Session saving follows `useSessionPersistence.js`: queue window `[index-20, index-20+300)`, position at most every 4 s.

### 3.6 Migration from Electron (`storage::migrate_legacy`)

Runs once at startup if `<data>/mosaic-data.json` doesn't exist.

1. Candidate legacy dirs, in order (first containing `mosaic-data.json` wins):
   - Linux: `~/.config/Mosaic`, `~/.config/mosaic`
   - Windows: `%APPDATA%\Mosaic`, `%APPDATA%\mosaic`
   - macOS: `~/Library/Application Support/Mosaic`, `~/Library/Application Support/mosaic`
2. Copy `mosaic-data.json`, `mosaic-config.json`, `lyrics-choices.json` and `spotify-auth.json` (if present) into the data dir.
3. Copy `covers/*` into `<cache>/covers/`.
4. Rewrite every `artwork`/`thumb` string of the form `media://art/<file>` to the absolute path `<cache>/covers/<file>`, everywhere a Track appears.
5. Don't copy `local-cache.json`. The scanner rebuilds it (cheap, and the format differs; see 7.3).
6. Write `<data>/.migrated-from` containing the legacy path. Show a toast once: "Imported your library from the previous version".

Unit test this with a fixture directory.

---

## 4. Queue and player logic

### 4.1 `core::queue`

```rust
pub struct Queue { pub list: Vec<Track>, pub index: isize }   // index = -1 when empty
```

Port `useQueueOps.js` as methods that return a `QueueOutcome` enum (e.g. `Added`, `AlreadyQueued`, `PlayNow(Track, Vec<Track>)`, `PlayingNext`, …). The controller maps outcomes to toasts using **the exact JS toast strings**: "Already in your queue", "Added to queue", "Added N track(s) to the queue", "Playing next", "Starting radio from “…”", "No similar tracks found", "Could not start radio".

`play(target, list)`: dedupe the list by id (keep first), insert the target at the front if missing, and set the index to the target.

`next_index(queue, direction, prefs, rng)`: port `nextIndex` exactly (shuffle picks a random index ≠ current; repeat-all wraps; returns `None` at the end).

### 4.2 `core::relevance`

Port `relevance.js` (`fits_along`, `filter_along`). Use the `regex` crate with case-insensitive flags. The `\b` semantics are the same.

### 4.3 `app::player::PlayerController`

The UI-agnostic port of **`components/player/PlayerProvider.js`** plus `useSleepTimer.js`, `useListeningStats.js` and `usePresenceSync.js`. It owns the `Queue`, the current `Track`, `playing/loading/error`, a `Time { current, duration, offset, tail }`, sleep state and the active deck id. It talks to the engine only through `audio::Command`/`audio::Event`, and to services only by returning `Effect`s:

```rust
pub enum Effect {
    Engine(audio::Command),
    ResolveStream { track: Track, gen: u64 },          // services → Message::StreamResolved
    FetchRelated { seed: Track, purpose: RelatedPurpose },
    Toast(String),
    RecordPlay(Track), AddSeconds(Track, u32),
    SaveSession(SessionPatch),
    Presence(Option<PresencePayload>),
    MediaControls(MediaState),
    SpotifyLoad { track: Track, position_ms: u64, gen: u64 },
}
impl PlayerController {
    pub fn handle(&mut self, input: PlayerInput, now: Instant) -> Vec<Effect>;
}
```

`PlayerInput` covers user intents (`Play(track, list)`, `PlayIndex`, `Toggle`, `Next`, `Prev`, `Seek`, `SetVolume`, `SetSpeed`, `ToggleShuffle`, `CycleRepeat`, `SetSleep`, queue ops), engine events, resolved streams, related results, a `Tick` (1 Hz, for the sleep timer and stats) and settings changes.

Behaviors to preserve (all from PlayerProvider.js):

- **Load generation token** (`loadSeq`): every load bumps `gen`. Results and events carrying an old `gen` are ignored.
- **Load:** set track, clear error, `loading = true`, time = `{seekTo, duration/1000}`. Resolve the stream, then call engine `Load`. On ready: seek to `seekTo`, or to `silence.start` when skipSilence is on. If `fadeIn > 0`, start at volume 0 and fade to the volume over `fadeIn` s with curve `In`.
- **Step** (`step(direction, auto)`): sleep end-of-track, then repeat-one (restart), then "prev restarts if > 3 s", then `next_index`. At the end of the queue with `auto && autoplay && current.source == soundcloud`: continue with similar tracks (`related(id, 40)`, `filter_along`, minus tracks already queued). Toast "Autoplay: similar tracks added".
- **Toggle with fadePause:** resume = fade 0 → volume over 300 ms. Pause = fade current → 0 over 250 ms, then pause and restore the volume. A toggle before anything has loaded (restored session) means load with `seekTo = resumeAt`.
- **Crossfade trigger** (`maybeCrossfade`), checked on every position event of the active deck. Conditions as in JS (`LEAD = 1`, `left < 0.4` bail, `length < seconds*2` bail, not repeat-one, not sleep-eot, the next index exists, the next track isn't Spotify, once per `track.id+gen`). Then hand over: swap the active deck, fade the outgoing deck to 0 over `max(0.5, left-0.1)` s with curve `Out` then unload, and `step(1, auto=true)` with fadeIn = crossfade seconds.
- **Silence tail skip** (`maybeSkipTail`): when skipSilence is on and `position >= silence.end`, `step(1, true)` once per `track.id+gen`.
- **Time shown:** `offset = silence.start`, `tail = duration - silence.end` (only when skipSilence is on). The seek bar works in shown time (see `Controls.js`).
- **Sleep timer:** port `useSleepTimer.js`. The final fade is 25 steps × 100 ms, implemented as one engine `Fade` over 2500 ms linear then `Pause`, then restore the volume. Toasts are identical, including "Sleep timer: goodnight 🌙".
- **Listening stats:** `RecordPlay` 20 s after a track starts (cancelled if the track changes first). `AddSeconds(15)` every 15 s while playing.
- **Presence:** port `usePresenceSync.js`. Send `None` when not playing or loading. Debounce seeks by 800 ms.
- **Session restore:** port `useSessionPersistence.js` restore (set the queue and track, `resumeAt = position`, don't autoplay).
- **Errors:** "Could not play this track", "Playback failed", or the service's message. Shown in the player bar artist line in error color (see PlayerBar.js).

Unit-test the controller heavily with a fake clock and fake engine (assert on emitted `Effect`s): crossfade trigger window, repeat modes, shuffle never repeats the current index, prev-restart, autoplay continuation, sleep eot, stale gen ignored, skip over unavailable tracks.

---

## 5. Services (crate `services`)

All async on tokio, with one shared `reqwest::Client` built with:
- `USER_AGENT` from `electron/core/constants.js`. On Linux use `Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36`.
- Gzip and a 20 s connect timeout. Per-request timeouts as in the JS (`AbortSignal.timeout`).

Return `anyhow::Result<T>`. User-facing error strings must match the JS ones.

| Rust module | Port of | Public API (shape) |
|---|---|---|
| `soundcloud::client` | `electron/soundcloud/client.js` | `sc_get(url, params) -> Value`. Discovers and caches client_id into Config, retries once on 401/403 after rediscovery. Error "Could not reach SoundCloud. Check your connection." |
| `soundcloud::api` | `soundcloud/ipc.js` | `search(q, limit=30)`, `charts(genre, limit=30)`, `related(id, limit=20)`, `stream(id) -> StreamInfo { kind: Progressive\|Hls{codec}, url, silence: Option<Silence> }` |
| `soundcloud::tracks` | `soundcloud/tracks.js` | `full_streams`, `slim_track`, `playable`. **Change:** stream preference order is progressive mp3 → hls mp3 (`audio/mpeg`) → hls aac → hls opus, because progressive is cheapest for us. Keep the `snipped` filter. |
| `soundcloud::silence` | `soundcloud/silence.js` | `waveform_silence(track_json) -> Option<Silence { start: f64, end: Option<f64> }>` |
| `lyrics::parse` | `lyrics/parse.js` | `parse_title`, `parse_lrc`, `has_word_stamps`, `kind_of`, `shape_lyrics`, `empty_lyrics`, `norm`. `norm` uses NFKD + strip combining marks (`unicode-normalization`). |
| `lyrics::lrclib` | `lyrics/lrclib.js` | Semaphore(4), 3 attempts with 500·(i+1) ms backoff, 404 → None, failure counter. Headers `User-Agent: Mosaic/0.5 (desktop player)`, `Lrclib-Client: Mosaic/0.5`. |
| `lyrics::genius` | `lyrics/genius.js` | `genius_lyrics(title, artist) -> Option<String>`. Port the string surgery literally (it's regex/HTML-depth based, not a DOM parser). |
| `lyrics::service` | `lyrics/service.js` | `get_lyrics(args)`, `get_candidates(args)`, `select_lyrics(args)`. Same precedence: custom choice → sidecar .lrc → in-memory auto cache → embedded tags (via lofty: SYLT/USLT/`LYRICS` Vorbis comment) → lrclib exact/fuzzy (+1 retry after 1.5 s when server failures increased) → Genius if `beta && genius`. |
| `local::scanner` | `library/scanner.js` | `scan_paths(paths) -> Vec<Track>`. Walk dirs (follow the JS: recursive, extension filter `AUDIO_EXT`), 4 concurrent workers (`spawn_blocking`), mtime cache, `fallback_track` filename parsing ("Artist - Title"), lofty tags, duration in ms, `lrc_path` if a sibling `.lrc` exists. Sort artist then title. Keeps `local_paths: HashMap<TrackId, PathBuf>` for playback and lyrics. |
| `local::covers` | `saveCover` in scanner.js | Decode the embedded picture, write a 400 px wide JPEG q82 and a 120 px wide q80 to `<cache>/covers/<key>.jpg` and `<key>-s.jpg`, where `key` = id without `local:`. `artwork`/`thumb` = absolute file paths. |
| `spotify::auth` | `spotify/auth.js` | PKCE loopback on `127.0.0.1:8898/callback`, same scopes minus `streaming`/`user-modify-playback-state`/`user-read-playback-state` (librespot handles playback). Token refresh, `country()`, `status()`. |
| `spotify::web_api` | `spotify/sdk.js` (non-playback parts) | `search`, `library`, `artist`, `album`, `playlist_tracks(id)` (new: paginated `/playlists/{id}/tracks`), `album_tracks(id)`. The `slim`/`albumCard`/`artistCard` mappers are copied exactly. |
| `spotify::embed`, `url`, `matcher` | `spotify/embed.js`, `url.js`, `match.js` | Literal ports. `matcher::find_match` uses `soundcloud::client`. |
| `spotify::import` | `spotify/ipc.js` `importPlaylist` | Embed first. If embed returned ≥100 tracks **and** web api is connected, fetch the full list via the API and prefer the longer list. If embed fails and the API is connected, use the API. Else `{ error: "unavailable" }`. Concurrency 4 for matching (from `useSpotifyImport.js`). |
| `spotify::playback` | new (section 8) | librespot session, `load(uri, position_ms)`, `pause`, `resume`, `seek`, events. |
| `discord` | `discord/presence.js`, `activity.js` | Client id `1562187683592147045`. Retry every 15 s, min 4 s between pushes, `clip()` rules, Listening type, SoundCloud button. Exposes a `status()` watch channel. Runs on its own std thread because the crate's IPC is blocking. |
| `update` | `update.js` | Repo `wubly/mosaic`, recheck every 6 h, `newer()` semver-ish compare, current = `CARGO_PKG_VERSION`. |
| `images` | new | `load(src, max_px) -> Arc<RgbaImage>`. `src` is an http(s) URL (download to disk cache, then decode) or an absolute path. Resize to fit `max_px` with `Lanczos3`. In-memory LRU (≈200 entries). Dedupes concurrent requests for the same key. |
| `oauth` | shared by auth.js | Tiny one-shot HTTP server (`tiny_http`) on a fixed port that returns a "You can close this window" page and yields the query string. Times out after 5 min. |

Network integration tests go in `services/tests/net_*.rs`, marked `#[ignore]`. Offline unit tests use recorded JSON/HTML fixtures under `services/tests/fixtures/`. Create the fixtures by running the JS once, or by fetching once and saving.

---

## 6. Audio engine (crate `audio`)

### 6.1 Overview

```
            ┌──────── decoder thread A ────────┐
Source A ──►│ demux → decode → resample → (stretch) │──► ring A (f32 stereo, ~0.5 s) + marker ring A
            └──────────────────────────────────┘
            ┌──────── decoder thread B ────────┐
Source B ──►│              (same)              │──► ring B + marker ring B
            └──────────────────────────────────┘
cpal callback:  pull A·gainA + B·gainB → EQ(5 biquads/ch) → compressor → master volume → out
                                                         └→ analyser ring (mono)
control thread: owns decks, receives Command (crossbeam channel), emits Event (crossbeam channel)
```

- The output is always **stereo f32 at the device default sample rate** (usually 48000). Mono sources are duplicated and >2 channels are downmixed.
- **Gains** (per-deck fade gain, master volume) are atomics read by the callback. Fades are computed **in the callback per sample** from parameters set atomically (`start_gain`, `target_gain`, `start_frame`, `len_frames`, `curve`). When a fade completes, the callback sets a flag and the control thread performs the follow-up (`Pause`/`Unload`) and emits an event.

### 6.2 Public API

```rust
pub struct Engine { /* cmd tx, event rx, analyser handle */ }
impl Engine {
    pub fn start() -> anyhow::Result<Engine>;               // opens default output device
    pub fn send(&self, cmd: Command);
    pub fn events(&self) -> crossbeam_channel::Receiver<Event>;
    pub fn analyser(&self) -> Analyser;                     // clonable, lock-free read
    pub fn start_offline(sample_rate: u32) -> (Engine, OfflineRenderer); // for tests: render(&mut [f32]) pulls the mixer
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum DeckId { A, B }
pub enum Curve { Linear, In /* sin(t·π/2) */, Out /* 1-cos(t·π/2) */ }   // exactly fade.js EASE
pub enum AfterFade { Nothing, Pause, Unload }

pub enum SourceSpec {
    File(PathBuf),
    Http { url: String },                     // progressive, range-seekable
    Hls { playlist_url: String },
    Push(PushSource),                         // librespot sink feeds this (section 8)
}

pub enum Command {
    Load { deck: DeckId, gen: u64, source: SourceSpec, start_at: f64, autoplay: bool, fade_in_ms: u32, duration_hint: Option<f64> },
    Play { deck: DeckId }, Pause { deck: DeckId }, Unload { deck: DeckId },
    Seek { deck: DeckId, secs: f64 },
    Fade { deck: DeckId, to: f32, ms: u32, curve: Curve, then: AfterFade },
    SetVolume(f32),                       // linear 0..1, same as HTMLMediaElement.volume
    SetEq { enabled: bool, gains_db: [f32; 5] },
    SetLeveling(bool),
    SetSpeed(f32),                        // 0.5..2, pitch preserved
    Shutdown,
}

pub enum Event {
    Ready     { deck: DeckId, gen: u64, duration: Option<f64> },  // ≈ loadedmetadata / MANIFEST_PARSED
    Playing   { deck: DeckId, gen: u64 },
    Paused    { deck: DeckId, gen: u64 },
    Buffering { deck: DeckId, gen: u64, on: bool },              // ≈ waiting / playing
    Position  { deck: DeckId, gen: u64, secs: f64 },             // every 100 ms while playing, plus after a seek
    Seeked    { deck: DeckId, gen: u64 },
    FadeDone  { deck: DeckId, gen: u64 },
    Ended     { deck: DeckId, gen: u64 },
    Error     { deck: DeckId, gen: u64, message: String },
    DeviceChanged,                                                // output device lost/reopened
}
```

### 6.3 Position tracking (RT-safe)

The decoder thread pushes samples to an SPSC ring (`rtrb`) and, for each decoded packet, a marker `(cumulative_output_frame_index_at_packet_start, source_position_secs)` into a small marker ring. The callback counts output frames consumed per deck. It pops markers whose frame index ≤ consumed, and stores `source_pos + (consumed - marker_frame) / out_rate * speed` into an `AtomicU64` (f64 bits). The control thread reads that atomic every 100 ms and emits `Position`.

**Seek/flush:** the control thread bumps the deck's `flush_gen` (AtomicU32) and tells the decoder to seek. When the callback sees a new `flush_gen`, it discards everything in the sample and marker rings. The decoder only pushes post-seek samples after acknowledging the new flush_gen.

### 6.4 Sources

- **File:** `symphonia` with all default codecs plus `symphonia` features `mp3`, `aac`, `isomp4`, `flac`, `vorbis`, `wav`, `alac`, `pcm`.
  - **Opus:** symphonia demuxes Ogg Opus but has no decoder. Decode packets with the `opus` crate (libopus; enable its static/bundled build so Windows/macOS don't need a system lib) behind a small `Decoder` adapter.
  - **WMA and anything else unsupported:** if `ffmpeg` is on PATH, spawn `ffmpeg -v error -ss <start> -i <file> -f f32le -ac 2 -ar <out_rate> -` and read stdout. Seeking respawns it. If ffmpeg is missing: `Error { message: "This format needs ffmpeg installed" }`.
- **Http (progressive):** a `MediaSource` implementation backed by a background download task. It holds a sparse buffer, satisfies reads from what's downloaded, and on a seek beyond the downloaded range issues a new `Range:` request. `byte_len` comes from `Content-Length`. `is_seekable = true`. Emits `Buffering` when reads block more than 150 ms.
- **Hls:** parse the media playlist with `m3u8-rs`. Segment list with durations → `seek(secs)` picks the segment and skips decoded frames inside it. A background prefetch keeps ~20 s ahead and ~10 s behind (mirrors the hls.js config `maxBufferLength 20, backBufferLength 10`).
  - mp3 segments: feed the concatenated bytes to symphonia's mp3 reader.
  - aac (fMP4: init + `.m4s`): concatenate init + segments for symphonia's isomp4. If that proves unreliable, fall back to the next transcoding.
  - opus HLS: lowest priority, implement last.
  - If a segment returns 403/410 (expired signed URL): emit `Error { message: "stream expired" }`. The controller re-resolves the stream and reloads at the current position **once** before surfacing "Playback failed".
- **Push:** a producer handle the Spotify sink writes interleaved f32 into (section 8). Seeking is delegated to the producer (librespot), so the deck just flushes.

### 6.5 Resampling and speed

- `rubato` `SincFixedIn` (or the FFT resampler) per deck from the source rate to the output rate. Only build it when the rates differ.
- Speed ≠ 1: time-stretch with pitch preservation using `signalsmith-stretch` (Rust bindings). If unavailable, use `ssstretch`, and failing that implement WSOLA in `stretch.rs`. Apply it after resampling in the decoder thread. Changing speed takes effect within the ring latency (≤0.5 s); that's acceptable. Markers record source time, so position stays correct.

### 6.6 DSP (port of `components/player/audio/audioGraph.js`)

- **EQ:** 5 biquads per channel using RBJ cookbook formulas, matching WebAudio's BiquadFilterNode definitions:
  - band 0 `lowshelf` 60 Hz
  - bands 1–3 `peaking` 230 / 910 / 3600 Hz, Q = 1
  - band 4 `highshelf` 14000 Hz

  Gains in dB. When `enabled == false`, all gains are 0 (flat, still in the chain; or bypassed if all are 0). Coefficient updates are smoothed over 20 ms to avoid zipper noise.
- **Leveler:** feed-forward compressor matching DynamicsCompressorNode settings:
  - leveling on: threshold −24 dB, ratio 4, knee 0
  - attack 10 ms, release 250 ms
  - leveling off: bypass
  - no makeup gain (WebAudio adds some; skip it and note this in DECISIONS.md)
- **Master volume:** linear multiply (parity with `audio.volume`), smoothed over 10 ms.
- **Analyser:** mono downmix → lock-free ring of the last 512 samples. `Analyser::byte_frequency_data(&self, out: &mut [u8; 256])` reproduces `AnalyserNode.getByteFrequencyData`:
  - fftSize 512 with a Blackman window
  - smoothingTimeConstant 0.82 applied to magnitudes
  - minDecibels −100, maxDecibels −30, linearly mapped to 0..255

  The smoothing state lives in the `Analyser` (UI side), not the callback.

### 6.7 Device handling

On a cpal stream error (device unplugged, PipeWire restart), reopen the default device after 500 ms with the same rate if possible, emit `DeviceChanged`, and resume. If the new rate differs, rebuild the resamplers (flushing).

### 6.8 Gapless

When a deck reaches EOF and the controller has already issued a `Load` on the other deck with `autoplay` (crossfade = 0 case), there must be no audible gap beyond decoder priming. For the non-crossfade "next track" path, the controller loads the next track on the **other** deck and swaps (same as crossfade with 0 s). This is an improvement over the JS, which reused the same `<audio>`.

### 6.9 Engine tests (`audio/tests/`)

These use `Engine::start_offline(48000)` and generated WAV fixtures (write them in the test with `hound`):
- A 1 kHz sine at 44.1 kHz plays back at 48 kHz with the correct duration (±10 ms) and pitch (FFT peak at 1 kHz ± 1 bin).
- Seek to 2.0 s, then the next `Position` is 2.0 ± 0.02 s.
- An equal-power crossfade between two **different-frequency** sines (440 Hz → 660 Hz, same amplitude; uncorrelated, so powers add) keeps the summed RMS within ±1.5 dB of a single sine across the fade.
- EQ +6 dB at band 2: a sine at 910 Hz comes out +6 dB ± 0.5 dB; a sine at 60 Hz is unchanged ± 0.5 dB.
- Fade `Out` then `AfterFade::Pause` emits `FadeDone` and `Paused` and outputs silence afterwards.
- Analyser: a full-scale 1 kHz sine at 48 kHz puts its peak in bin round(1000/(48000/512)) ≈ 11.
- Speed 1.5 on a 10 s file ends after ≈ 6.67 s of output, with the position reaching ≈ 10 s.

---

## 7. Platform integration (crate `platform`)

### 7.1 Media controls (`souvlaki`)

- Create `MediaControls` with `dbus_name: "mosaic"`, `display_name: "Mosaic"`. On Windows pass the main window HWND (get it from iced via `window::run_with_handle` or the raw-window-handle API of the resolved iced version). Record how you obtained it in DECISIONS.md.
- Metadata: title, artist, album, cover URL (http URL, or `file://` for local covers), duration.
- Playback state and position updates on play/pause/seek, and every 5 s otherwise.
- Events: Play/Pause/Toggle → `Toggle`, Next, Previous, Stop, SetPosition/Seek → `Seek`.

This replaces `electron/media-keys.js` and `navigator.mediaSession` (`useSystemMedia.js`). Window title = "`title — artist`" while playing, else "Mosaic" (same as useSystemMedia).

### 7.2 Tray

Menu items as in `electron/tray.js`: Open Mosaic, Play / pause, Next, Previous, Quit. Left click opens the window.

Close-to-tray (`closeToTray`, default true) hides the window instead of quitting. **But** if the tray failed to register (e.g. GNOME without the AppIndicator extension, or no StatusNotifierWatcher on the bus), treat close-to-tray as off and show a note in Settings: "No system tray found, so closing the window quits Mosaic".

### 7.3 Single instance

On startup, try to connect to the local socket `mosaic-<uid>` (Linux: `$XDG_RUNTIME_DIR/mosaic.sock`). If that succeeds, send `focus\n` and exit 0. Otherwise bind it and listen; on `focus`, show, unminimize and focus the main window.

### 7.4 Opening links

`open::that` only for `https://` URLs (port of `openExternalHttps`).

### 7.5 Linux specifics (document these in README under "Linux notes")

- Wayland has no client-controlled always-on-top. The mini player calls `window::change_level(AlwaysOnTop)` anyway (it works on X11 and some compositors) and silently ignores failure.
- Custom title bar: the window is created with `decorations: false`. Drag via `window::drag`, and resize via `window::drag_resize` on a 6 px invisible border.
- If the env var `MOSAIC_DECORATIONS=1` is set, use server-side decorations and hide our title bar (escape hatch for tiling WMs).
- Audio goes through ALSA via cpal (PipeWire/Pulse provide ALSA shims). Document `libasound2`/`alsa-lib` as a runtime dependency.

---

## 8. Spotify playback via librespot (`services::spotify::playback`, feature `spotify`)

- Crates: `librespot-core`, `librespot-playback`, `librespot-metadata` and `librespot-oauth` (all the same version).
- **Login:** use `librespot-oauth` with its default (librespot) client id and the redirect/port that crate documents. Note this is a *different* OAuth flow from the Web API one.
  - Cache credentials with `librespot_core::cache::Cache` in `<data>/librespot/` (credentials) and `<cache>/librespot-audio/` (audio files, size limit 1 GiB).
  - Settings → Beta → Spotify gets a second button, "Enable playback (Premium)", that triggers this login. The playback login is otherwise triggered lazily on the first Spotify play, with a toast.
- **Sink:** implement `librespot_playback::audio_backend::Sink`.
  - `write(packet, converter)` converts samples to f32 and pushes them into the deck's `PushSource` producer, blocking (with a 2 s timeout) when the ring is full. That gives librespot natural back-pressure.
  - `start`/`stop` map to nothing.
  - librespot's output is 44.1 kHz stereo; our resampler handles it.
  - Use `PlayerConfig { normalisation: false, gapless: true, .. }` with the bitrate set to 320.
- **Control:** `load(uri, position_ms)` → `player.load(SpotifyId::from_uri(uri), true, position_ms)`. Pause/resume call librespot **and** the engine deck. Seek → `player.seek(ms)` + engine flush.
- **Position:** the deck reports position as `load_position + frames_consumed / 44100 * speed`. Reset it on a seek.
- **End:** librespot `PlayerEvent::EndOfTrack` → the push source marks EOF → the deck drains → `Ended`.
- **Errors:** `PlayerEvent::Unavailable` → "This track isn't available on Spotify in your region". Session failure with a non-Premium account → "Spotify playback needs Premium".
- The controller rule "don't crossfade *into* a Spotify track" stays (parity). Crossfading *out of* Spotify now works.
- When the feature is compiled out, Spotify tracks are "unavailable" (3.4) and the Spotify Web API browsing UI stays (useful for import/matching).

---

## 9. UI (crate `app`)

### 9.1 Architecture

- `iced::daemon(App::title, App::update, App::view)` with `.subscription(App::subscription)`, `.theme(App::theme)` and `.style(...)`. There is only one window, `main`. OAuth flows open the system browser, so no helper windows are needed.
- The mini player is **the same window** switching layout (parity with `win:mini`):
  - mini on: store the normal size, `resize(380×660)`, set `min_size(340×440)`, `change_level(AlwaysOnTop)`.
  - mini off: restore the size and the 980×640 minimum, level normal.
- `App` state holds `Store`, `Config`, `PlayerController`, `ui: UiState` (current view, overlays, per-view state), `theme: DynamicTheme`, `toasts`, `images: ImageCache` (handles + pending set), `services: Services` and `engine: Engine`.
- `Message` is one enum with nested per-view enums (`Message::Discover(discover::Msg)`, …). Async work is `Task::perform(services.x(..), Message::XLoaded)`. Results carry a request seq so stale results are dropped (mirrors `seq.current` in Discover.js).
- **Subscriptions:**
  - engine events: a `Subscription::run` that drains the crossbeam receiver on a blocking thread and forwards into a channel
  - media-controls events, tray events, single-instance focus events
  - `time::every(1s)` → `PlayerInput::Tick`
  - `window::frames()` **only while** `anim.is_active() || (playing && (visualizer_visible || lyrics_visible || wavy_visible))`
  - keyboard: `keyboard::on_key_press` for shortcuts (9.6)
  - file drop: `window::Event::FileDropped/FileHovered/FilesHoveredLeft` → DropVeil plus `add_sources`
  - window close request → close-to-tray logic

### 9.2 Theme (`theme/`)

- `Palette` holds exactly the CSS custom properties in `app/globals.css` `:root`: `primary, on_primary, primary_container, on_primary_container, surface, on_surface, on_surface_variant, surface_container, surface_container_high, surface_container_highest, outline, outline_variant, error, error_container, on_error_container`. Defaults come from the `:root` hex values.
- `dynamic.rs` is a port of `components/theme/useDynamicTheme.js`. Seed = `source_color_from_image` on the track's `thumb || artwork` (decode it via the image service at ≤128 px). Tones are exactly the ones in `applyScheme`, cached per URL. Fallback seed `#6750a4`.
- **Transition:** palettes animate over 0.9 s (the CSS `transition: --md-* .9s`) by lerping in linear sRGB. `App::theme()` returns an `iced::Theme::custom` built from the *current interpolated* palette. All widget styles read from our `Palette` (a global `Arc<ArcSwap<Palette>>` or passed through `Theme`; pick whichever the resolved iced version makes ergonomic and record it).
- `styles.rs`: one function per CSS class used for buttons and containers: `primary`, `tonal`, `ghost`, `icon` (+ `on`, `like`), `chip` (+ `on`), `glass`, `error`, `muted` text, `kbd`, plus scrollbar style (10 px, thumb `#ffffff1f`, radius 5). Copy sizes, radii, paddings and colors (including `color-mix` results) from `globals.css`; compute `color-mix(in srgb, A p%, B)` with a helper `mix(a, b, p)`.
- **Fonts:** bundle Roboto Flex (UI text, 400/500 weights) as the default font, and Material Symbols Rounded for icons. Base text is 14/20. Headings follow globals.css (`.step h1` 36/44, `h2`, `h3` sizes from CSS).
- **Icons:** `widgets/icon.rs` maps a `Icon` enum (one variant per `react-icons/md` glyph used; grep `from 'react-icons/md'` across `components/` for the full list) to Material Symbols codepoints. Filled vs outline variants: Material Symbols uses the `FILL` axis. If the resolved iced can't set variable font axes, bundle the static "Material Symbols Rounded Filled" and "Outlined" fonts and pick per variant.

### 9.3 Ambient background and glass

- **Ambient** (port of `components/app/Ambient.js` plus the `.ambient*` CSS): when the track's art changes, downscale it to 48×48 (drawn at −6,−6 60×60 as in JS), apply `fast_blur(3)`, saturate ×1.6 (in HSL or via a saturation matrix), then upscale to 256×256 with a Gaussian blur σ≈12 to approximate the CSS `blur(90px)` at window scale. Store it as an image handle. It's drawn full-window behind everything with `inset: -15%` (scaled 130 %, centered, `ContentFit::Cover`, linear filtering). Crossfade between old and new over 1.2 s. No art → plain `surface`.
- **Glass panels** (`.glass`): fill `mix(surface, transparent, 58%)`, 1 px border `#ffffff0d`, radius 28. Because the ambient behind them is already blurred, no backdrop blur is needed.
- **Now Playing overlay** (`.nowplaying`): fill `mix(surface, transparent, 62%)` over the ambient. Use the ambient image again at higher opacity to imitate the stronger blur.

### 9.4 Animation (`anim.rs`)

- `Spring { value, velocity, target, stiffness, damping, mass=1 }` uses semi-implicit Euler with fixed 1/240 s substeps. It's "settled" when |v| < 0.01 and |x − target| < 0.001. Use the framer-motion parameters from the JS verbatim (e.g. lyrics aside `stiffness 260 damping 30`, rail pill `500/36`, play button radius `420/28`, player bar entrance `260/26`, stats bars `140/20`).
- `Tween { from, to, start, dur, ease }` with eases `linear`, `ease_out` (cubic-bezier(.3,0,0,1) for the wavy amplitude), `sine_in`, `sine_out`.
- `AnimationClock` registry: widgets and views ask it for values keyed by a stable id. `is_active()` drives the frames subscription.
- `AnimatePresence` equivalents (enter/exit): keep exiting elements in state with an exit animation until it settles, then drop them.

### 9.5 Layout (main window)

Port the structure in `components/app/Shell.js` plus the layout CSS (`.app`, `.titlebar`, `.rail`, `.main`, `.side`, `.bar`, `.rz-v`, `.rz-h`). Read those CSS rules for exact gaps and paddings.

```
┌ titlebar 40px: logo "mosaic" left, window buttons right (min/max/close, drawn by us, symbol color #cac4d0) ┐
│ rail │            main (glass)                     │rz│  lyrics aside (glass, width = sideWidth) │
│      │  current view                               │  │  LyricsPanel                              │
├──────┴─────────────────────────────────────────────┴──┴──────────────────────────────────────────┤
│ rz-h                                                                                               │
│ player bar (glass, height = barHeight): now | controls (transport + seek) | tools                   │
└───────────────────────────────────────────────────────────────────────────────────────────────────┘
overlays (z-order): NowPlaying, ImportDialog, CommandPalette, TrackMenu (context menu), Toasts, DropVeil
```

- On macOS, draw no window buttons (native traffic lights via `titlebar_transparent`/`fullsize_content_view` platform settings) and pad the titlebar left by 92 px (`macInset`).
- Resizers: port `usePanelSizes.js` constants (`BAR {80,200,100}`, `SIDE {280,560,360}`). Drag updates live; commit on release; double-click resets.
- Lyrics aside enter/exit: opacity + x offset 32 px, spring 260/30.

### 9.6 Keyboard shortcuts

Port `components/app/useShortcuts.js` exactly:

| Key | Action |
|---|---|
| Ctrl/Cmd+K | toggle the command palette (works even while typing) |
| Space | toggle play |
| ← / → | seek ∓5 s |
| Shift+← / Shift+→ | prev / next |
| ↑ / ↓ | volume ±0.05 |
| M | mute toggle (0 ↔ 0.8) |
| L | like (with toast) |
| S | shuffle |
| R | cycle repeat |
| Y | lyrics panel |
| I | mini player |
| F | Now Playing |
| / | focus search on Discover |

None of these fire while a text input is focused, the palette is open, or Ctrl/Meta/Alt is held (except Ctrl+K). Track focus state yourself (`text_input` ids + focus messages).

### 9.7 Widgets (`widgets/`)

Each is a custom `iced::advanced::Widget` or a composition. "Port" means: read the JS component and its CSS rules, and match look and behavior.

| Widget | Port of | Notes |
|---|---|---|
| `wavy_slider` | `components/player/WavySlider.js` + `.ws*` CSS | Canvas-style draw with `Frame`/`Path`. Constants WAVELENGTH 28, AMPLITUDE 3.5, HEIGHT 28, GAP 7. Wave phase scrolls continuously while `playing` (read the `ws-wave` keyframes in CSS for speed). Amplitude scaleY animates 1 ↔ 0.02 over 450 ms with ease-out. Thumb 4 px (3 px while held) × 22 px. Thumb/fill position eases over 250 ms linear unless held. Keyboard arrows step `0.05` if max ≤ 1, else `step`. Pointer capture while dragging. `wavy=false` gives a flat line. |
| `visualizer` | `Visualizer.js` | 40 bars, gap 4, bin mapping `floor((|i-half+0.5|/half)^1.4 * 110) + 1`, attack 0.5 / release 0.12 smoothing, alpha 0.45 + level·0.55, rounded bars, color = primary. Redraw at ≤30 fps. Stops requesting frames when paused and all levels are < 0.01. |
| `lyrics_view` | `LyricsPanel.js` (line list) + `.lyrics .line .w` CSS | One `Paragraph` per line with one span per word. Word rects come from `Paragraph::span_bounds` (or the equivalent in the resolved iced). Per word, draw the dim text, then the same text clipped to `p * word_width` in the bright color (`p` from `lyrics_timing`). Line emphasis: opacity 1 / 0.55 / 0.28 by distance, scale 0.96 for non-active lines, animated over 350 ms. **Replace the CSS blur with an extra opacity step** (d≥3 → 0.22). Auto-scroll centers the active line using a spring. Clicking a line seeks to `line.t`. Lookahead `+0.04 s`. `large` variant for Now Playing. |
| `virtual_list` | `components/ui/VirtualList.js` | Fixed row height (read VirtualList.js for it). Renders only visible rows + overscan inside `scrollable` using top/bottom spacers. Needed for local libraries of 10k+ tracks. Acceptance: 20k rows scroll at 60 fps on integrated graphics. |
| `track_row`, `track_tile` | `components/tracks/TrackRow.js`, `TrackTile.js`, `Bars.js`, `useTrackState.js`, `format.js` | Playing indicator bars animate while this track plays. Hover shows the play button. Right click opens the track menu. Double click (row) or click (tile) plays the track with its list. Unavailable tracks are greyed out (3.4). |
| `menu` | `components/tracks/TrackMenu.js` | Context menu overlay with the same items and order as the JS (read the file), including the playlist submenu. Positioned at the cursor and clamped to the window. |
| `rail` | `components/app/Rail.js` | Items, icons (outline ↔ filled when active), badge `99+` cap, active pill slides between items with spring 500/36, label below. |
| `switch`, `chip`, `search_bar`, `skeleton`, `empty`, `page_header`, `toast`, `resizer`, `glass`, `collage`, `logo` | `components/ui/*`, `components/playlists/Collage.js`, `components/onboarding/MosaicArt.js` | Toasts: read `Toast.js` for duration/stacking. Skeleton shimmer from the `.skel` CSS. Collage: 2×2 cover grid. |
| `image` helper | n/a | `art(src, size)` returns a cached handle or a placeholder (surface_container_high + music note icon) and triggers the load. Fade in once loaded (150 ms). |

### 9.8 Views (`views/`)

For each: read the JS file(s), reproduce the structure, copy all strings, and wire actions to `PlayerInput`/`Store`/`Services`.

| View | Port of | Notes |
|---|---|---|
| `onboarding` | `components/onboarding/Onboarding.js` + `steps/*` + `MosaicArt.js` | Steps Welcome → Name → Music (add folders) → Options → Spotify (only if `beta`; otherwise skip). Saves Config `{name, onboarded: true}`. Dots indicator. Shown when `!config.onboarded`. |
| `discover` | `library/Discover.js` | 400 ms debounced search with a seq guard. Device hits (top 5 local matches). Spotify artists/albums/songs when connected. SoundCloud results grid. Home state: Jump back in, Your Spotify likes, genre chips + charts shelf, "Because you played …". **Remove all YouTube sections.** The view stays alive when hidden (keep its state), like `hidden={view !== 'discover'}`. |
| `library` | `library/LibraryView.js`, `LikedList.js`, `HistoryList.js`, `Stats.js`, `PlayAll.js`, `time.js` | Tabs Liked / Playlists / History / Stats with a sliding indicator. Stats uses `core::stats` and an animated activity bars chart. |
| `playlists`, `playlist_detail` | `playlists/PlaylistsTab.js`, `PlaylistDetail.js`, `PlaylistItem.js`, `Collage.js` | Create, rename inline, delete, drag reorder (implement drag reorder in the list: press-hold 150 ms then drag; drop updates via `reorder_playlist`). |
| `import_dialog` | `playlists/ImportDialog.js`, `useSpotifyImport.js` | URL input, progress while matching (concurrency 4), create playlist, navigate to it. Remove the "needs login → open web login" branch; replace it with "Connect Spotify in Settings → Beta to import playlists over 100 tracks" when relevant. |
| `local` | `library/LocalView.js` | Sources chips, Add folder/files (rfd), rescan spinner, filter, sort chips with grouping headers, Play / Shuffle, virtual list. |
| `queue` | `queue/Queue.js` | Current + up next, remove, drag reorder, clear. |
| `spotify` | `spotify/SpotifyView.js`, `SpotifyProvider.js` | Library home, artist page, album page (navigation stack). Only reachable when connected (rail item is shown only then). |
| `settings` | `settings/SettingsView.js` | Sections: Updates, Playback (autoplay, crossfade slider 0–12 s, fade on pause, skip silence, volume leveling, lyrics panel, close to tray), Discord, Beta (beta toggle, Genius, Spotify client id + connect/disconnect + playback login), Library, Keyboard shortcuts, About. **Remove the YouTube and Performance sections.** Reset button calls `onReset` → config reset + onboarding. |
| `player_bar` | `player/PlayerBar.js`, `Controls.js`, `TunePopover.js`, `tune/*` | Idle variant when there's no track. Thumb swaps with scale/opacity 200 ms. The play button morphs radius 18 ↔ 28 (spring 420/28) and icon swaps with scale+rotate. Spinner while loading. Tune popover has tabs EQ (5 sliders −12..12 dB + presets from `presets.js`), Speed (`SPEEDS`), Sleep (`TIMERS` + end of track + off), Gain (`GainSlider.js`). **Remove the YouTube video button.** |
| `now_playing` | `player/NowPlaying.js` | Full-window overlay: big art, title/artist, controls, visualizer, large lyrics. Close on Esc or button. Enter/exit animation from the JS. |
| `mini` | `player/MiniPlayer.js` | Compact layout used in mini mode. |
| `lyrics_panel`, `lyrics_picker` | `lyrics/LyricsPanel.js`, `LyricsPicker.js` | Header art + title + source pill (`KIND_LABEL`). Picker: candidate list from `get_candidates`, typed query search, select → `select_lyrics`, "Automatic" option resets. |
| `command_palette` | `app/CommandPalette.js`, `app/paletteActions.js` | Actions list exactly as in paletteActions.js. Track search over queue + likes + history + local. Enter on free text → Discover search. Arrow navigation. |
| `drop_veil` | `app/DropVeil.js` | Overlay while files hover the window. Drop → `add_sources` + go to Local + toast "Adding N item(s) to your library". |

### 9.9 Accessibility and polish checklist

- Every icon button has a tooltip with the same `title` text as the JS (iced `tooltip`).
- HiDPI: test at scale 1, 1.5 and 2. Images are requested at `size × scale_factor`.
- Text selection isn't required (iced limitation); don't work around it.
- Respect `prefers-reduced-motion`-like opt-out: env var `MOSAIC_REDUCED_MOTION=1` snaps all springs and tweens.

---

## 10. App startup sequence (`main.rs`)

1. Init logging (`tracing` + `tracing-subscriber`, env filter `MOSAIC_LOG`). Write to stderr and `<cache>/mosaic.log` (rotate at 5 MB).
2. Single-instance check (7.3).
3. Resolve dirs, then `migrate_legacy()`.
4. Load Config and Store.
5. Start the tokio runtime (multi-thread, 4 workers) and stash its `Handle` in `Services`.
6. `Engine::start()`. On failure, continue with a disabled engine and show the error banner "No audio output device found".
7. Start media controls, tray, the discord thread (if `settings.presence`) and an update check (after 5 s).
8. Kick off `scan_paths(store.sources)`.
9. Run `iced::daemon`, open the main window (1280×820, min 980×640, `decorations: false`, background `#141218`, icon from `assets/icon.png`).
10. On exit: flush the saver, clear discord presence, engine `Shutdown`, then drop the tray.

CLI flags: `--version`, `--data-dir <path>` (override, used by tests), `--reset-window`.

---

## 11. Packaging and CI

- `native/packaging/linux/app.mosaic.player.desktop`: `Categories=AudioVideo;Audio;Player;`, `StartupWMClass=mosaic`, `MimeType=audio/mpeg;audio/flac;audio/ogg;audio/x-wav;audio/mp4;` (opening files adds them as sources and plays the first). Handle file args on startup and via the single-instance socket (`open <path>\n`).
- AppStream metainfo with screenshots pointing to `docs/screenshots/*.webp` in the repo.
- `cargo-packager` config in `native/app/Cargo.toml` `[package.metadata.packager]`: product name Mosaic, identifier `app.mosaic.player`, icons, formats `appimage, deb, nsis, dmg`. The .deb depends on `libasound2`.
- Flatpak manifest: runtime `org.freedesktop.Platform` (current stable branch), SDK extension `rust-stable`, offline cargo sources via `flatpak-cargo-generator.py` (document the command). finish-args:
  - `--share=network --share=ipc`
  - `--socket=wayland --socket=fallback-x11 --device=dri`
  - `--socket=pulseaudio`
  - `--filesystem=xdg-music:ro`
  - `--own-name=org.mpris.MediaPlayer2.mosaic`
  - `--talk-name=org.kde.StatusNotifierWatcher`
  - `--filesystem=xdg-run/discord-ipc-0` (and also `xdg-run/app/com.discordapp.Discord` for Discord-as-Flatpak)
- Gentoo ebuild using `cargo.eclass`, `CRATES` generated with `pycargoebuild`. `RDEPEND="media-libs/alsa-lib"`.
- `.github/workflows/native.yml` (add the file, but don't push): matrix ubuntu-latest / windows-latest / macos-latest. Steps: install Linux deps (`libasound2-dev pkg-config libxkbcommon-dev libwayland-dev`), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo packager --release` on tags.

---

## 12. Milestones

Each milestone lists its tasks, then its **acceptance checks**. Run all checks from `native/` unless noted. Commit after each milestone.

### M0: Scaffold
- Create the workspace and crates exactly as in section 2 (empty modules with `todo!()`-free stubs that compile). Copy the icons and download the fonts (with LICENSE files) into `assets/`.
- Add `native/README.md` covering build deps per OS and how to run (`cargo run -p mosaic`).
- **Accept:** `cargo build` and `cargo test` succeed. `cargo run -p mosaic` opens an empty 1280×820 undecorated window titled "Mosaic" on Wayland and X11 (`WINIT_UNIX_BACKEND`/`WAYLAND_DISPLAY=` to force X11). Closing it exits.

### M1: Core model, storage, migration
- Sections 3 and 4.1–4.2, plus `stats.rs`, `lyrics_timing.rs` and `format.rs`.
- **Accept:** these unit tests pass:
  - round-trip a realistic `mosaic-data.json` fixture (create one with every track kind and numeric + string ids) byte-equivalently after parse → serialize → parse (compare as `serde_json::Value`)
  - local id vector
  - every queue op
  - `next_index` matrix
  - relevance cases (talk regex, duration bounds, same-title dedupe)
  - stats summarize (streak logic, including "today empty doesn't break the streak")
  - `build_timings` for both word-stamped and estimated lines
  - migration fixture rewrites `media://art/` URLs

### M2: Services, part 1 (no UI)
- soundcloud client/api/tracks/silence, lyrics (all), local scanner + covers, images, update, http.
- Add a debug CLI `cargo run -p mosaic -- --debug-cmd <cmd> <args>`, behind a `debug-cli` feature, so you can test services without a UI:
  - `sc-search "q"`
  - `sc-stream <id>`
  - `lyrics "<title>" "<artist>" <duration_ms>`
  - `scan <dir>`
- **Accept:** offline fixture tests for `parse_title`, `parse_lrc`, `kind_of`, `shape_lyrics`, genius `clean` (use a saved HTML fixture), `waveform_silence`, `slim_track`/`playable`, `score_candidate`, and the `fallback_track` filename parser. `cargo test -- --ignored net_` passes with network (SoundCloud search returns >0 streamable tracks, a stream URL resolves, lrclib returns synced lyrics for a well-known song). `scan` on a folder of mixed mp3/flac/m4a/ogg/opus returns correct tags and writes covers.

### M3: Audio engine core
- Engine with the File source, resampling, two decks, mixer, fades, master volume, position markers, seek/flush, Ended, device reopen.
- `--debug-cmd play <file>` plays a file to the speakers and prints positions; `Ctrl+C` stops.
- **Accept:** engine tests in 6.9 except speed/EQ/analyser pass. Manual: a FLAC and an MP3 play correctly on PipeWire, and seeking is instant (<100 ms audible latency).

### M4: Engine DSP, speed, network sources
- EQ, compressor, analyser, stretch, Http source, Hls source (mp3 first, then aac, then opus), opus files, ffmpeg fallback, buffering events, expired-URL error.
- `--debug-cmd sc-play <id>` resolves and plays a SoundCloud track.
- **Accept:** all 6.9 tests pass. Manual: a SoundCloud HLS track plays and seeks to 80 % within 1 s; EQ bass boost is audible; speed 1.5 keeps the pitch.

### M5: PlayerController
- Section 4.3 in `app/src/player.rs`, fully unit tested with fake engine and clock.
- **Accept:** all controller tests listed in 4.3 pass. `--debug-cmd queue-play <id1> <id2> …` plays a SoundCloud queue with crossfade 6 s and you hear an equal-power crossfade.

### M6: App shell and theme
- Theme palette + styles, fonts, icons, the anim module, the titlebar with drag/resize and window buttons, rail, main/aside/bar layout with resizers, ambient + glass, toasts, keyboard shortcuts plumbing, the frames subscription gate.
- Dynamic theme from a hard-coded test image URL, behind `--debug-theme <url>`.
- **Accept:** the layout matches the Electron app side by side at 1280×820 (take screenshots of both; check spacing and radii by eye). Resizers persist to settings. Idle CPU is <1 % with nothing playing (`top`). The theme transition animates smoothly over 0.9 s.

### M7: Playback UI
- Player bar (all states), wavy slider, controls, tune popover (EQ/speed/sleep/gain), volume, media controls (souvlaki), window title, session restore, listening stats, Discord presence.
- **Accept:** play a SoundCloud track from a debug button. Then:
  - `playerctl -p mosaic play-pause|next|previous|metadata` all work, and the GNOME/KDE media widget shows the cover
  - closing and reopening restores the queue and position without autoplay
  - Discord shows "Listening to" with the correct progress (if Discord is running)

### M8: Discover, search, track components
- Track row/tile/menu, Discover (search + home shelves + charts + related), command palette, image loading with cache and fade.
- **Accept:** typing a search shows results within ~1 s with skeletons while loading. Clicking a tile plays it with the list as the queue. The context menu works. Scrolling a 30-tile grid shows no hitching. Ctrl+K palette actions all work.

### M9: Library, playlists, local, queue
- LibraryView tabs, Stats, playlists + detail + drag reorder, LocalView with virtual list, sorting/grouping, sources, file dialogs, drag-and-drop from the file manager, Queue view.
- **Accept:** adding a 5k-track folder scans in reasonable time (<60 s cold, <3 s warm), the list scrolls smoothly, and the filter is instant. Likes/playlists/history persist across restarts. Data migrated from an Electron install shows up correctly, local covers included.

### M10: Lyrics
- Lyrics panel with the word-fill widget, picker, Now Playing (large lyrics + visualizer), mini player.
- **Accept:**
  - Word-by-word highlighting matches the Electron app on a track with word-stamped lrclib lyrics.
  - An `.lrc` sidecar is picked up for local files.
  - Choosing another version persists across restarts.
  - The visualizer reacts to audio.
  - Mini mode resizes correctly and goes on top on X11.

### M11: Spotify
- Web API auth/browse (search sections in Discover, Spotify view, artist/album pages), import via embed/API, matching, librespot playback with the sink into the engine.
- **Accept:** with a Premium account, a Spotify track plays through the engine. EQ affects it, seek works, end of track advances the queue. A 250-track playlist imports fully when connected and the first 100 when not. Without the `spotify` feature (`cargo build --no-default-features`), it builds and Spotify tracks show as unavailable.

### M12: Settings, onboarding, tray, single instance, updates, packaging
- Settings view (all sections), onboarding flow, tray with the GNOME fallback note, single-instance focus + file open, update banner, `.desktop`/metainfo, cargo-packager config, Flatpak manifest, ebuild, CI workflow.
- **Accept:**
  - a fresh `--data-dir` shows onboarding
  - reset works
  - launching a second instance focuses the first
  - `cargo packager --release -f appimage` produces an AppImage that runs on the dev machine
  - `.deb` and Flatpak builds succeed locally (if `flatpak-builder` is installed; otherwise note it in DECISIONS.md)

### M13: Parity pass and cutover prep
- Walk the **parity checklist** below in the native app, fix gaps, and record anything intentionally different in `native/DECISIONS.md`.
- Update the root `README.md`:
  - native build instructions
  - Linux notes (7.5)
  - YouTube removed
  - Spotify now via librespot (Premium, unofficial client)
- **Do not delete the Electron code**; leave a short `MIGRATION.md` at the repo root listing exactly which paths can be removed (`electron/`, `app/`, `components/`, `next.config.mjs`, `jsconfig.json`, `package.json`, `package-lock.json`, `build/` except icons) and the command sequence to promote `native/` to the root. The human decides when to run it.

### Parity checklist (M13)
- [ ] Onboarding (all steps), name greeting on Discover
- [ ] SoundCloud search, charts by genre, related, autoplay continuation, radio
- [ ] Local folders/files, drag-drop, rescan, sort/group/filter, covers, .lrc sidecars, embedded lyrics
- [ ] Queue: add, play next, enqueue many, remove, reorder, clear
- [ ] Shuffle, repeat off/all/one, prev-restarts-after-3 s
- [ ] Crossfade 1–12 s, fade on pause, skip silence (start + tail), gapless next
- [ ] EQ + presets, volume leveling, speed 0.5–2 with pitch preserved, sleep timer (minutes + end of track)
- [ ] Likes, playlists (create/rename/delete/reorder/add/remove), history, stats (streak, 14-day chart, top tracks/artists)
- [ ] Spotify import (embed ≤100, full when connected), SoundCloud matching
- [ ] Lyrics: auto (lrclib exact → fuzzy → retry), Genius fallback (beta), picker + persistence, word fill, click to seek
- [ ] Now Playing, mini player, lyrics panel toggle, panel resize persistence
- [ ] Dynamic theme from art with 0.9 s transition, ambient background
- [ ] Command palette, every keyboard shortcut in 9.6
- [ ] Media keys / MPRIS / SMTC / macOS Now Playing, tray menu, close to tray
- [ ] Discord rich presence with progress and the SoundCloud button
- [ ] Update checker
- [ ] Spotify (beta): search, library, artist/album pages, playback via librespot
- [ ] Session restore on launch
- [ ] Migration from the Electron data dir

---

## 13. Known pitfalls

- **iced API drift:** widget/advanced APIs change between minor versions. Before writing each custom widget, open the resolved version's docs for `advanced::Widget`, `text::Paragraph` and `canvas`. Copy patterns from the iced repo examples at the *same tag* (`git clone --branch <tag> https://github.com/iced-rs/iced` into the scratch dir, read `examples/`).
- **Numeric SoundCloud ids:** never stringify ids when persisting (see `TrackId`). Comparisons use `TrackId` equality: `Num(5) != Str("5")`, which is the same as JS `===`.
- **Expired SoundCloud stream URLs:** stream URLs are signed and expire (~minutes to hours). A restored session must re-resolve the stream on play; never persist stream URLs.
- **lrclib etiquette:** keep the 4-concurrent cap and the user agent header.
- **librespot API churn:** OAuth and the `Sink` signature changed across 0.5 → 0.6+. Use the resolved version's docs.
- **Wayland focus stealing:** `window::gain_focus` may be ignored. For single-instance focus, also call `request_user_attention`.
- **cpal on Linux** picks the ALSA `default` device, which PipeWire/Pulse route correctly. Don't enumerate and pick hw devices.
- **Opus HLS from SoundCloud** uses Ogg pages split across segments. Concatenating segment bytes into one Ogg stream works. Don't decode segments independently.
- **Decoder priming/padding:** use symphonia's `gapless` option (`FormatOptions { enable_gapless: true }`) for mp3/aac so crossfades and gapless transitions don't click.
- **Equal-power crossfade:** the JS uses `sin` for fade-in and `1-cos` for fade-out (see fade.js). Keep those exact curves. Don't "fix" them.
