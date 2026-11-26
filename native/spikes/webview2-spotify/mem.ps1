# Memory of the WebView2 test: the host exe plus the msedgewebview2.exe processes that use the
# test's profile folder (other apps' WebView2 processes are left out).
#   .\mem.ps1 -ClientId <id> [-Playing] [-BrowserArgs "--renderer-process-limit=1 ..."]
param(
    [Parameter(Mandatory = $true)] [string]$ClientId,
    [switch]$Playing,
    [string]$BrowserArgs = "",
    [string]$FeaturesOff = "",
    [int]$Settle = 15
)
$here = $PSScriptRoot
Get-Process webview2-spike -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2
$out = Join-Path $here "logs\mem.log"
$argList = @("--spike-page=sdk", "--spike-hidden", "--spike-nocheck", "--spike-client-id=$ClientId")
if ($Playing) { $argList += "--spike-autorun" }
# Start-Process joins arguments with spaces, so a value containing spaces must carry its own quotes.
if ($BrowserArgs) { $argList += "`"--spike-args=$BrowserArgs`"" }
if ($FeaturesOff) { $argList += "--spike-features-off=$FeaturesOff" }
Start-Process (Join-Path $here "target\release\webview2-spike.exe") -ArgumentList $argList `
    -RedirectStandardOutput $out -RedirectStandardError "$out.err" -WindowStyle Hidden | Out-Null
$wait = if ($Playing) { '"name":"time_to_sound_cold"' } else { '"kind":"ready"' }
$deadline = (Get-Date).AddSeconds(45)
while ((Get-Date) -lt $deadline -and -not (Select-String $out -Pattern $wait -Quiet -ErrorAction SilentlyContinue)) {
    Start-Sleep -Milliseconds 500
}
# While playing, measure during the first track (the sequence seeks after 4 s, so settle less).
Start-Sleep -Seconds $(if ($Playing) { 3 } else { $Settle })
$rows = Get-CimInstance Win32_Process | Where-Object {
    $_.Name -eq "webview2-spike.exe" -or ($_.Name -eq "msedgewebview2.exe" -and $_.CommandLine -like "*mosaic-wv2-spike*")
} | ForEach-Object {
    $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
    $type = if ($_.Name -eq "webview2-spike.exe") { "host (our exe)" }
            elseif ($_.CommandLine -match '--type=(\S+)') { $Matches[1] } else { "browser" }
    $sub = if ($_.CommandLine -match '--utility-sub-type=(\S+)') { $Matches[1] } else { "" }
    [pscustomobject]@{ Type = $type; Sub = $sub
        PrivMB = [math]::Round($p.PrivateMemorySize64 / 1MB); WsMB = [math]::Round($p.WorkingSet64 / 1MB) }
}
Write-Host ("== {0}{1}" -f $(if ($Playing) { "playing" } else { "idle, player connected" }), $(if ($BrowserArgs) { " + $BrowserArgs" } else { "" }))
$rows | Sort-Object PrivMB -Descending | Format-Table -AutoSize
"total: {0} processes, {1} MB private, {2} MB working set" -f @($rows).Count, ($rows | Measure-Object PrivMB -Sum).Sum, ($rows | Measure-Object WsMB -Sum).Sum
Get-Process webview2-spike -ErrorAction SilentlyContinue | Stop-Process -Force
