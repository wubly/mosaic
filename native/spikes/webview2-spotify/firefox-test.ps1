# The Linux candidate for Firefox-family users (Firefox, LibreWolf, Zen), tried on Windows with
# whichever one is installed: it loads the SDK page in its own profile (DRM and autoplay turned
# on there only), without a visible window. The test server hosts the page and prints its logs.
#   .\firefox-test.ps1 -ClientId <id> [-Browser <path>] [-Mode headless|window] [-Timeout 120]
param(
    [Parameter(Mandatory = $true)] [string]$ClientId,
    [string]$Browser = "$env:ProgramFiles\Zen Browser\zen.exe",
    [ValidateSet("headless", "window")] [string]$Mode = "headless",
    [int]$Timeout = 120,
    [switch]$Bidi
)
$here = $PSScriptRoot
$exeName = [IO.Path]::GetFileName($Browser)
$profileDir = Join-Path $env:LOCALAPPDATA "mosaic-firefox-spike"
New-Item -ItemType Directory -Force $profileDir | Out-Null

# Only this profile gets DRM and autoplay; the user's own browser profile is never touched.
@'
user_pref("media.eme.enabled", true);
user_pref("media.gmp-widevinecdm.enabled", true);
user_pref("media.gmp-widevinecdm.visible", true);
user_pref("media.gmp-manager.updateEnabled", true);
user_pref("media.autoplay.default", 0);
user_pref("media.autoplay.blocking_policy", 0);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("app.update.enabled", false);
user_pref("browser.sessionstore.resume_from_crash", false);
user_pref("browser.startup.page", 0);
user_pref("dom.ipc.processCount", 1);
user_pref("dom.ipc.processCount.webIsolated", 1);
user_pref("fission.autostart", false);
user_pref("dom.ipc.processPrelaunch.enabled", false);
user_pref("browser.tabs.remote.separatePrivilegedContentProcess", false);
user_pref("extensions.pocket.enabled", false);
user_pref("browser.newtabpage.enabled", false);
user_pref("media.gmp.decoder.enabled", true);
'@ | Set-Content -Encoding ascii (Join-Path $profileDir "user.js")

function Get-Tree {
    $all = Get-CimInstance Win32_Process -Filter "Name='$exeName'"
    $main = $all | Where-Object { $_.CommandLine -like "*mosaic-firefox-spike*" -and $_.CommandLine -notlike "*-parentPid*" }
    $ids = @($main | ForEach-Object { $_.ProcessId })
    $all | Where-Object { $ids -contains $_.ProcessId -or ($_.CommandLine -match '-parentPid (\d+)' -and $ids -contains [int]$Matches[1]) }
}
function Stop-All {
    Get-Process webview2-spike -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Tree | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 2
}
Stop-All
New-Item -ItemType Directory -Force (Join-Path $here "logs") | Out-Null
$out = Join-Path $here "logs\firefox-$Mode.log"
Start-Process (Join-Path $here "target\release\webview2-spike.exe") `
    -ArgumentList @("--spike-server-only", "--spike-autorun", "--spike-nocheck", "--spike-client-id=$ClientId") `
    -RedirectStandardOutput $out -RedirectStandardError "$out.err" -WindowStyle Hidden | Out-Null
Start-Sleep -Seconds 1

$pageUrl = "http://127.0.0.1:8898/sdk#post"
$browserArgs = @("--no-remote", "--profile", "`"$profileDir`"")
if ($Mode -eq "headless") { $browserArgs += "--headless" }
if ($Bidi) {
    # Diagnosis: start blank with WebDriver BiDi, inject instrument.js into every frame (including
    # the SDK's sdk.scdn.co iframe), then navigate.
    $browserArgs += @("--remote-debugging-port", "9222", "about:blank")
} else {
    $browserArgs += $pageUrl
}
Start-Process $Browser -ArgumentList $browserArgs | Out-Null

if ($Bidi) {
    # Node's BiDi client injects instrument.js into every frame, navigates, and records the console.
    $bidiOut = Join-Path $here "logs\firefox-bidi-console.log"
    Start-Process node -ArgumentList @("`"$(Join-Path $here 'bidi.mjs')`"", "9222", $pageUrl,
        "`"$(Join-Path $here '..\cef-spotify\pages\instrument.js')`"", "`"$bidiOut`"", "$Timeout") `
        -RedirectStandardOutput (Join-Path $here "logs\bidi-client.log") -WindowStyle Hidden | Out-Null
}

$deadline = (Get-Date).AddSeconds($Timeout)
$measured = $false
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 1
    if (-not $measured -and (Select-String $out -Pattern '"name":"time_to_sound_cold","ms"' -Quiet -ErrorAction SilentlyContinue)) {
        Start-Sleep -Seconds 2
        $rows = Get-Tree | ForEach-Object {
            $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $type = if ($_.CommandLine -match '-isForBrowser|-childID') {
                if ($_.CommandLine -match '\s(tab|rdd|gpu|socket|utility|gmplugin|forkserver)\s*$') { $Matches[1] } else { "child" }
            } else { "main" }
            if ($_.CommandLine -match '\s(tab|rdd|gpu|socket|utility|gmplugin)\s*$') { $type = $Matches[1] }
            [pscustomobject]@{ Type = $type; PrivMB = [math]::Round($p.PrivateMemorySize64 / 1MB); WsMB = [math]::Round($p.WorkingSet64 / 1MB) }
        }
        Write-Host "== $exeName $Mode, playing"
        $rows | Sort-Object PrivMB -Descending | Format-Table -AutoSize | Out-String | Write-Host
        Write-Host ("total: {0} processes, {1} MB private, {2} MB working set" -f @($rows).Count,
            ($rows | Measure-Object PrivMB -Sum).Sum, ($rows | Measure-Object WsMB -Sum).Sum)
        $measured = $true
    }
    if (Select-String $out -Pattern 'sequence_done' -Quiet -ErrorAction SilentlyContinue) { break }
}
Stop-All
Select-String $out -Pattern '"kind":"(page|me|ready|metric|playback_error|account_error|initialization_error|authentication_error|autoplay_failed|needs_signin|error|connect)"' |
    ForEach-Object { $_.Line.Substring(8, [math]::Min(240, $_.Line.Length - 8)) }
