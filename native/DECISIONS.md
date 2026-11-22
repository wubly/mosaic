# Decisions

One line per decision: `Mn: decision`.

- M0: Commit attribution uses `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` (the model actually running), not the Opus 5.5 line in the spec.
- M0: Material Symbols Rounded variable TTF is 15 MB; kept whole for now, subsetting deferred to packaging.
- M2: Local scanner cache uses its own `{v, entries}` layout (version 1) and never deletes the cover dir on a version change, so migrated covers survive.
- M2: Local track sorting uses a case/accent-insensitive key (NFKD, lowercase) instead of JS `localeCompare`.
- M2: Spotify/YouTube-free `Services` takes a shared `Arc<Mutex<Config>>`; the app mutates it via `Services::update_config`, which persists.
- M2: Embedded lyrics: SYLT (ms timestamps) parsed via lofty's MpegFile; other lyrics frames and `LYRICS` comments via lofty `ItemKey::{Lyrics,UnsyncLyrics}`.
