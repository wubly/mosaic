# Starts the spike with the CEF runtime on PATH and Mosaic's Spotify client id.
#   .\run.ps1                      SDK page, visible window
#   .\run.ps1 -Page web            open.spotify.com with the observing driver
#   .\run.ps1 -Hidden -Autorun     SDK page in a hidden window, runs the test sequence by itself
#   .\run.ps1 -Full                without the trimming switches (memory comparison)
# Output goes to the console and to logs\<page>-<timestamp>.log.
param(
    [ValidateSet("sdk", "web")] [string]$Page = "sdk",
    [switch]$Hidden,
    [switch]$Autorun,
    [switch]$Full,
    [string]$ClientId = "",
    [string]$Tracks = ""
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

if (-not $ClientId) {
    $config = Join-Path $env:APPDATA "mosaic\data\config.json"
    if (Test-Path $config) {
        $ClientId = (Get-Content $config -Raw | ConvertFrom-Json).spotifyClientId
    }
}
if (-not $ClientId -and $Page -eq "sdk") {
    Write-Error "No Spotify client id. Pass -ClientId <id> (the one in Mosaic's Settings > Spotify); its app must allow the redirect http://127.0.0.1:8898/callback."
}

$exe = Join-Path $here "target\release\cef-spotify-spike.exe"
if (-not (Test-Path (Join-Path $here "target\release\libcef.dll"))) {
    Write-Error "CEF runtime not found next to the exe. Run .\setup.ps1 after building."
}

$spikeArgs = @("--spike-page=$Page")
if ($Hidden) { $spikeArgs += "--spike-hidden" }
if ($Autorun) { $spikeArgs += "--spike-autorun" }
if ($Full) { $spikeArgs += "--spike-full" }
if ($ClientId) { $spikeArgs += "--spike-client-id=$ClientId" }
if ($Tracks) { $spikeArgs += "--spike-tracks=$Tracks" }

New-Item -ItemType Directory -Force (Join-Path $here "logs") | Out-Null
$log = Join-Path $here ("logs\{0}{1}-{2}.log" -f $Page, $(if ($Full) { "-full" } else { "" }), (Get-Date -Format "yyyyMMdd-HHmmss"))
Write-Host "Logging to $log"
& $exe @spikeArgs | Tee-Object -FilePath $log
