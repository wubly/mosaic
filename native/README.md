# Mosaic (native)

Native Rust rewrite of Mosaic. See `../docs/native-rewrite.md` for the build spec.

## Build dependencies

- **Linux:** a recent stable Rust, `pkg-config`, ALSA headers (`libasound2-dev` / `alsa-lib`), `libxkbcommon-dev`, `libwayland-dev`.
- **Windows:** Rust (MSVC toolchain) and the Visual Studio build tools.
- **macOS:** Rust and the Xcode command line tools.

## Run

```sh
cargo run -p mosaic
```

`MOSAIC_DECORATIONS=1` uses server-side window decorations.
