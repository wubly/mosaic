# WebView2 Spotify spike

The WebView2 counterpart of `../cef-spotify`. It uses the same SDK page, server and
instrumentation (shared by path), but runs in the system's Edge WebView2 runtime. It's a separate
crate so its dependencies never change the CEF build. Results are in
`docs/spotify-cef-backend.md`, section 2.

```powershell
cargo build --release
```

Quit Mosaic and the CEF test first, because all three use port 8898. The sign-in token is shared
with the CEF test (`%LOCALAPPDATA%\mosaic-cef-spike\token.json`). If it's missing, sign in once
through the CEF test's `run.ps1`, or run this one visible with `--spike-client-id=<id>` and
press **Sign in**.

| What | Command |
|---|---|
| Play test, hidden | `.\target\release\webview2-spike.exe --spike-hidden --spike-autorun --spike-nocheck --spike-client-id=<id>` |
| Memory, idle | `.\mem.ps1 -ClientId <id>` |
| Memory, playing | `.\mem.ps1 -ClientId <id> -Playing` |
| Memory, trimmed | add `-BrowserArgs "<switches>" -FeaturesOff "IsolateOrigins,site-per-process,AudioServiceOutOfProcess"` |

WebView2 has no console hook in the host, so the pages post their `[spike]` lines to the server,
which prints them.
