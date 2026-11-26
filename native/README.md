# Mosaic (native)

Native Rust rewrite of Mosaic: iced UI, our own audio engine (symphonia, rubato, cpal). Spotify plays through Spotify's Web Playback SDK in the system web view on Windows and macOS (`services::spotify::web`), and through librespot or Spotify Soloist on Linux.
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

Debug helpers (behind the default `debug-cli` feature): `--debug-cmd sc-search|mono-search|sc-stream|lyrics|scan|play|sc-play|queue-play …`,
`--debug-theme <image>`, `--debug-view <view>`, `--debug-import <spotify url>` and `--debug-screenshot <png>`
(saves only the Mosaic window and exits).

## Checks

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo test -- --ignored net_      # network tests (SoundCloud, lrclib)
cargo build --no-default-features # without librespot
```

## Releases

Bump `version` in `[workspace.package]` (`Cargo.toml`), commit, then push a matching tag:

```sh
git tag v0.6.2 && git push origin v0.6.2
```

`.github/workflows/release.yml` builds the Linux AppImage, `.deb` and tarball (on Ubuntu 22.04 for glibc reach), the
Windows NSIS installer and portable `.exe`, and a universal (arm64 + x86_64) macOS `.dmg`, then attaches them all,
plus `SHA256SUMS.txt`, to a GitHub release. Tags with a `-` (`v0.6.0-beta.1`) become prereleases. Running the
workflow by hand builds everything as workflow artifacts without publishing.

The macOS app is ad-hoc signed unless the repository has `APPLE_CERTIFICATE` (base64 `.p12`),
`APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY` secrets; add `APPLE_ID`, `APPLE_PASSWORD` (app-specific)
and `APPLE_TEAM_ID` to notarize too. Ad-hoc builds open only after "Open Anyway" in System Settings > Privacy &
Security. Windows builds are unsigned, so SmartScreen warns on first run.

## macOS and Windows notes

- macOS uses native window decorations (traffic lights over a transparent title bar) since winit cannot drag-resize
  an undecorated window there; `MOSAIC_DECORATIONS=0` turns them off.
- The tray icon and media controls (Now Playing, SMTC) start once the main window has opened: macOS needs the event
  loop running and SMTC needs the window's HWND.
- Windows release builds use the GUI subsystem, so `--version` and the debug CLI print nothing there.

## Linux notes

- Audio goes through ALSA via cpal (PipeWire/Pulse provide the shim); `libasound2` / `alsa-lib` is the runtime dependency.
- Wayland has no client-controlled always-on-top; the mini player asks for it anyway and ignores failure.
- The window is undecorated and draws its own title bar and resize borders; `MOSAIC_DECORATIONS=1` uses server-side decorations.
- The tray needs a StatusNotifier host; without one close-to-tray is disabled and Settings says so.
- Media keys and the desktop media widget use MPRIS (`playerctl -p mosaic …`).
