# Copies the CEF runtime that cef-dll-sys downloaded next to the spike's exe (CEF looks for its
# .pak/.dat/.bin resources beside the executable). Run once after `cargo build --release`.
# Only en-US locale files are copied, as the design doc's packaging section plans.
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$dest = Join-Path $here "target\release"
$dist = Get-ChildItem (Join-Path $here "target\release\build") -Directory -Filter "cef-dll-sys-*" |
    ForEach-Object { Join-Path $_.FullName "out\cef_windows_x86_64" } |
    Where-Object { Test-Path (Join-Path $_ "libcef.dll") } | Select-Object -First 1
if (-not $dist) { Write-Error "No CEF distribution under target\release\build. Build first." }

$files = Get-ChildItem $dist -File | Where-Object { $_.Extension -in ".dll", ".pak", ".dat", ".bin", ".json" }
$files | Copy-Item -Destination $dest -Force
New-Item -ItemType Directory -Force (Join-Path $dest "locales") | Out-Null
Get-ChildItem (Join-Path $dist "locales") -Filter "en-US*.pak" | Copy-Item -Destination (Join-Path $dest "locales") -Force

$size = ($files | Measure-Object Length -Sum).Sum + (Get-ChildItem (Join-Path $dest "locales") | Measure-Object Length -Sum).Sum
Write-Host ("CEF runtime copied from {0}" -f $dist)
Write-Host ("Runtime size (en-US only): {0:N0} MB" -f ($size / 1MB))
