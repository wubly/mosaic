# Changelog

Each release's notes on GitHub are its section below (see `.github/workflows/release.yml`).

## 0.7.0

### Spotify plays through Spotify's own player on Windows and macOS

Spotify tracks no longer depend on librespot or Spotify Soloist on Windows and macOS. Mosaic now
runs Spotify's official Web Playback SDK in the system's web view, hidden in the background:

- **Windows:** the Microsoft Edge WebView2 runtime that comes with Windows 10 and 11. Nothing
  extra is downloaded or bundled.
- **macOS:** WKWebView. **Experimental: not yet tested on a Mac.** If it doesn't play there,
  please open an issue with the message from the player bar.

This works for accounts Spotify stopped giving librespot playback keys to ("Spotify refused this
account a playback key"), with no Soloist download, API key or pairing.

What to know:

- **Reconnect Spotify once.** Playing needs Spotify's `streaming` permission, which earlier
  sign-ins didn't ask for. Mosaic says so in the player bar. Disconnect and connect again in
  Settings → Beta.
- **Premium is required,** as before. Free accounts get "Spotify playback needs Premium".
- **The player starts with the first Spotify track** (about a second on Windows) and stops after
  10 minutes without one, which frees its memory. While playing it uses about 180 MB.
- **Spotify plays straight to your speakers**, so EQ, leveling, crossfade and the visualizer don't
  apply to Spotify tracks. Mosaic's volume slider still controls it.
- Spotify sometimes swaps a track for your region's version. Mosaic follows that instead of
  treating it as a skip.
- **Linux is unchanged:** librespot, or Spotify Soloist when a Soloist key is set.

### Monochrome

- **Monochrome is a new music source:** its public lossless catalog, searchable in Discover next to
  SoundCloud, Spotify and YouTube. Tracks play as FLAC.
- **Autoplay and "Start radio" work for Monochrome tracks.** They follow YouTube Music's radio
  for the same song, matched back to the catalog, and fall back to the artist's top tracks and
  latest releases.

### For developers

- `docs/spotify-cef-backend.md` records the research behind the Spotify change: measured results
  for CEF, WebView2, installed Chrome and Firefox-family browsers, memory per configuration, and
  the decisions. The throwaway tests are in `native/spikes/`.
- New debug command `sp-play <query> [count]` plays Spotify search results through the real
  player and resolver. `--debug-cmd` now logs service messages to stderr (`RUST_LOG` overrides).
- Release notes now come from this file.
