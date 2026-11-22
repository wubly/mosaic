# Mosaic (native)

Native Rust rewrite of Mosaic: iced UI, our own audio engine (symphonia, rubato, cpal), librespot for Spotify.
The build spec is `../docs/native-rewrite.md`; deliberate deviations are in `DECISIONS.md`.

## Layout

| Crate | What it is |
|---|---|
| `crates/core` | model, storage, queue, relevance, stats, lyric timing (no I/O except storage) |
| `crates/audio` | playback engine: decode, resample, mixer, EQ, compressor, analyser, network and push sources |
| `crates/services` | SoundCloud, lyrics, local scanner, images, Spotify, Discord, updates |
| `crates/platform` | MPRIS/SMTC/Now Playing, tray, single instance, opening links |
| `app` | the `mosaic` binary: player controller, theme, widgets, views |

## Build dependencies

- **Linux:** a recent stable Rust, `pkg-config`, ALSA headers (`libasound2-dev` / `alsa-lib`), `libxkbcommon-dev`, `libwayland-dev`.
- **Windows:** Rust (MSVC toolchain) and the Visual Studio build tools.
- **macOS:** Rust and the Xcode command line tools.

## Run

```sh
cargo run -p mosaic               # debug build (slow UI; use --release to judge performance)
cargo run --release -p mosaic
```

Flags: `--version`, `--data-dir <path>`, `--reset-window` (restore default panel sizes), `<files>` (added to the library
and played). Environment: `MOSAIC_DECORATIONS=1`, `MOSAIC_REDUCED_MOTION=1`, `MOSAIC_LOG=<filter>`.

Debug helpers (behind the default `debug-cli` feature): `--debug-cmd sc-search|sc-stream|lyrics|scan|play|sc-play|queue-play …`,
`--debug-theme <image>`, `--debug-view <view>`, `--debug-import <spotify url>` and `--debug-screenshot <png>`
(saves only the Mosaic window and exits).

## Checks

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo test -- --ignored net_      # network tests (SoundCloud, lrclib)
cargo build --no-default-features # without librespot
```

## Linux notes

- Audio goes through ALSA via cpal (PipeWire/Pulse provide the shim); `libasound2` / `alsa-lib` is the runtime dependency.
- Wayland has no client-controlled always-on-top; the mini player asks for it anyway and ignores failure.
- The window is undecorated and draws its own title bar and resize borders; `MOSAIC_DECORATIONS=1` uses server-side decorations.
- The tray needs a StatusNotifier host; without one close-to-tray is disabled and Settings says so.
- Media keys and the desktop media widget use MPRIS (`playerctl -p mosaic …`).
