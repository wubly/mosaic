# The Linux candidate, tried on Windows: an installed Chrome loads the SDK page in its own profile,
# without a visible window. The test server (webview2-spike --spike-server-only) hosts the page
# and prints its log lines.
#   .\chrome-test.ps1 -ClientId <id> [-Mode headless|offscreen] [-Trim]
param(
    [Parameter(Mandatory = $true)] [string]$ClientId,
    [ValidateSet("headless", "offscreen")] [string]$Mode = "headless",
    [switch]$Trim,
    [string]$Chrome = "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    [int]$Timeout = 90
)
$here = $PSScriptRoot
$profileDir = Join-Path $env:LOCALAPPDATA "mosaic-chrome-spike"
function Stop-All {
    Get-Process webview2-spike -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" |
        Where-Object { $_.CommandLine -like "*mosaic-chrome-spike*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 2
}
Stop-All
New-Item -ItemType Directory -Force (Join-Path $here "logs") | Out-Null
$out = Join-Path $here ("logs\chrome-{0}{1}.log" -f $Mode, $(if ($Trim) { "-trim" } else { "" }))
Start-Process (Join-Path $here "target\release\webview2-spike.exe") `
    -ArgumentList @("--spike-server-only", "--spike-autorun", "--spike-nocheck", "--spike-client-id=$ClientId") `
    -RedirectStandardOutput $out -RedirectStandardError "$out.err" -WindowStyle Hidden | Out-Null
Start-Sleep -Seconds 1

$chromeArgs = @(
    "--user-data-dir=`"$profileDir`"", "--no-first-run", "--no-default-browser-check",
    "--autoplay-policy=no-user-gesture-required", "--app=http://127.0.0.1:8898/sdk#post"
)
if ($Mode -eq "headless") { $chromeArgs += "--headless=new" }
else {
    # An off-screen window counts as occluded: Chrome throttles it and the player stalls.
    $chromeArgs += @("--window-position=-32000,-32000", "--window-size=400,300",
        "--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding",
        "--disable-background-timer-throttling")
}
$featuresOff = @()
if ($Mode -eq "offscreen") { $featuresOff += "CalculateNativeWinOcclusion" }
if ($Trim) {
    $chromeArgs += @("--renderer-process-limit=1", "--in-process-gpu", "--disable-gpu",
        "--disable-gpu-compositing", "--disable-site-isolation-trials", "--enable-low-end-device-mode",
        "--js-flags=--lite-mode", "--disable-extensions", "--disable-sync")
    $featuresOff += @("IsolateOrigins", "site-per-process", "AudioServiceOutOfProcess")
}
if ($featuresOff) { $chromeArgs += "--disable-features=$($featuresOff -join ',')" }
Start-Process $Chrome -ArgumentList $chromeArgs | Out-Null

$deadline = (Get-Date).AddSeconds($Timeout)
$measured = $false
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 1
    if (-not $measured -and (Select-String $out -Pattern '"name":"time_to_sound_cold"' -Quiet -ErrorAction SilentlyContinue)) {
        Start-Sleep -Seconds 2
        $rows = Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" |
            Where-Object { $_.CommandLine -like "*mosaic-chrome-spike*" } | ForEach-Object {
                $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
                $type = if ($_.CommandLine -match '--type=(\S+)') { $Matches[1] } else { "browser" }
                $sub = if ($_.CommandLine -match '--utility-sub-type=(\S+)') { $Matches[1] } else { "" }
                [pscustomobject]@{ Type = $type; Sub = $sub
                    PrivMB = [math]::Round($p.PrivateMemorySize64 / 1MB); WsMB = [math]::Round($p.WorkingSet64 / 1MB) }
            }
        Write-Host "== Chrome $Mode$(if ($Trim) { ' trimmed' }), playing"
        $rows | Sort-Object PrivMB -Descending | Format-Table -AutoSize | Out-String | Write-Host
        Write-Host ("total: {0} processes, {1} MB private, {2} MB working set" -f @($rows).Count,
            ($rows | Measure-Object PrivMB -Sum).Sum, ($rows | Measure-Object WsMB -Sum).Sum)
        $measured = $true
    }
    if (Select-String $out -Pattern 'sequence_done' -Quiet -ErrorAction SilentlyContinue) { break }
}
Stop-All
Select-String $out -Pattern '"kind":"(page|me|ready|metric|playback_error|account_error|initialization_error|authentication_error|autoplay_failed|needs_signin|error)"' |
    ForEach-Object { $_.Line.Substring(8, [math]::Min(220, $_.Line.Length - 8)) }
