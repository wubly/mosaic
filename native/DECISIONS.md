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
