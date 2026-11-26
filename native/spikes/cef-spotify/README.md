# CEF Spotify spike

A throwaway test for `docs/spotify-cef-backend.md` (section 14). It isn't part of the Mosaic
workspace and isn't shipped. Results go in section 2 of that doc.

## Build (once)

CMake and Ninja must be on `PATH` (they ship with VS Build Tools):

```powershell
$vsc = "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake"
$env:PATH = "$vsc\CMake\bin;$vsc\Ninja;$env:PATH"
cargo build --release
.\setup.ps1
```

The first build downloads CEF (about 430 MB). On first start, the Widevine CDM downloads into
`%LOCALAPPDATA%\mosaic-cef-spike`. Restart once after that.

## Run

Quit Mosaic first, because both use port 8898. Your Spotify app (the client ID in Mosaic's
Settings) must allow the redirect URI `http://127.0.0.1:8898/callback`.

| Step | Command | What to do |
|---|---|---|
| SDK page (Premium) | `.\run.ps1 -ClientId <id>` | Sign in, then press **Run test sequence**. Listen for sound |
| SDK page, hidden | `.\run.ps1 -ClientId <id> -Hidden -Autorun` | Runs the sequence by itself (sign in visibly first) |
| Web player (free or Premium) | `.\run.ps1 -Page web` | Sign in on open.spotify.com, play anything. The driver only observes |
| Memory and CPU | `.\measure.ps1 -Seconds 60 -Label sdk-lean` | In a second terminal while something plays |
| Without trimming | add `-Full` to `run.ps1` | Then measure again with another label |
| Memory per lever | `.\mem-levers.ps1 -ClientId <id>` | Adds each lever (`iso`, `gpu`, `net`, `audio`, `lowend`, `v8`) one at a time, with the player idle |
| Memory per process | `.\mem-breakdown.ps1 -ClientId <id> -Levers "iso,gpu" -Extra "--spike-nocheck"` | `--spike-nocheck` skips the page's own Widevine check, which adds a CDM process |

The sign-in token is kept in `%LOCALAPPDATA%\mosaic-cef-spike\token.json`. It's served only to
the test page itself.

Logs go to `logs\`. Lines that start with `[spike]` are the results: `widevine`, `mse_type`,
`metric` (timings), `state`, `end_test_state`, and `*_error`. Send `logs\` back for analysis.
