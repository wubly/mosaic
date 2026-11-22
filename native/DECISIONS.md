# Decisions

One line per decision: `Mn: decision`.

- M0: Commit attribution uses `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` (the model actually running), not the Opus 5.5 line in the spec.
- M0: Material Symbols Rounded variable TTF is 15 MB; kept whole for now, subsetting deferred to packaging.
- M2: Local scanner cache uses its own `{v, entries}` layout (version 1) and never deletes the cover dir on a version change, so migrated covers survive.
- M2: Local track sorting uses a case/accent-insensitive key (NFKD, lowercase) instead of JS `localeCompare`.
- M2: Spotify/YouTube-free `Services` takes a shared `Arc<Mutex<Config>>`; the app mutates it via `Services::update_config`, which persists.
- M2: Embedded lyrics: SYLT (ms timestamps) parsed via lofty's MpegFile; other lyrics frames and `LYRICS` comments via lofty `ItemKey::{Lyrics,UnsyncLyrics}`.
- M3: Engine uses symphonia 0.6 / rubato 5 / cpal 0.18 (current releases); gapless trimming comes from symphonia's `AudioDecoderOptions::gapless` (default on) since `FormatOptions` no longer has `enable_gapless`.
- M3: Deck rings persist for the engine's lifetime; each Load spawns a decoder thread that pushes a Flush marker (epoch, source position) so the callback discards stale audio. EOF is signalled via `DeckShared::eof_frame` instead of a marker.
- M3: The cpal stream is created and owned inside the control thread (cpal streams are not `Send` on all platforms). `SourceSpec` currently has only `File`; `Http`/`Hls` land in M4 and `Push` in M11. `SetEq`/`SetLeveling`/`SetSpeed` are accepted but wired up in M4.
- M4: HTTP and HLS sources use blocking `ureq` on the decoder thread (no tokio dependency in `audio`); progressive seeks restart the download with a `Range` request when the target is beyond the buffered window, and a response without `Content-Length` is treated as non-seekable.
- M4: A 403/410 from a stream or HLS segment surfaces as `Error { message: "stream expired" }` so the controller can re-resolve once.
- M4: The compressor has no makeup gain (WebAudio adds some).
- M4: Network source tests use a raw `TcpListener` server because tiny_http chunk-encodes bodies over 32 KB and so hides `Content-Length`.
- M5: Auto steps (end of track, tail skip) load the next track on the other deck and unload the old one; manual steps reuse the active deck. True pre-loaded gapless at crossfade 0 is not done yet (the next load starts at `Ended`).
- M5: Repeat-one after EOF reloads the track (the decoder has exited, so a seek can't restart it); from tail-skip it seeks. End-of-track sleep triggered by a tail skip pauses the deck (the JS let the track run on and then advanced).
- M5: A play is recorded 20 s after each `load`, not after a session restore (the JS recorded a play for a restored track that was never played).
- M5: Pause/sleep fades are per-deck gain; the follow-up "restore volume" is a gain reset to 1 sent on the `Paused` event, because master volume is a separate engine parameter.
- M5: `core::queue::step_index` is the free-function form of `next_index_with` so the controller can walk past unavailable tracks.
- M5: `app/src/main.rs` carries a temporary `allow(dead_code)` on `mod player`; remove it in M7 when the UI consumes every effect.
