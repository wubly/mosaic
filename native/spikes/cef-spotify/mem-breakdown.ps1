# Per-process memory of the SDK page with the player connected (idle), for one lever set.
#   .\mem-breakdown.ps1 -ClientId <id> -Levers "iso,gpu"
param(
    [Parameter(Mandatory = $true)] [string]$ClientId,
    [string]$Levers = "iso,gpu,net,audio,lowend,v8",
    [int]$Settle = 15,
    [string[]]$Extra = @()
)
$here = $PSScriptRoot
Get-Process cef-spotify-spike -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2
$out = Join-Path $here "logs\breakdown.log"
Start-Process (Join-Path $here "target\release\cef-spotify-spike.exe") `
    -ArgumentList (@("--spike-page=sdk", "--spike-hidden", "--spike-client-id=$ClientId", "--spike-levers=$Levers") + $Extra) `
    -RedirectStandardOutput $out -RedirectStandardError "$out.err" -WindowStyle Hidden | Out-Null
$deadline = (Get-Date).AddSeconds(40)
while ((Get-Date) -lt $deadline -and -not (Select-String $out -Pattern '"kind":"ready"' -Quiet -ErrorAction SilentlyContinue)) {
    Start-Sleep -Milliseconds 500
}
Start-Sleep -Seconds $Settle
Write-Host "== levers: $Levers"
$rows = Get-CimInstance Win32_Process -Filter "Name='cef-spotify-spike.exe'" | ForEach-Object {
    $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
    $type = if ($_.CommandLine -match '--type=(\S+)') { $Matches[1] } else { 'browser' }
    $sub = if ($_.CommandLine -match '--utility-sub-type=(\S+)') { $Matches[1] } else { '' }
    [pscustomobject]@{
        Type   = $type
        Sub    = $sub
        PrivMB = [math]::Round($p.PrivateMemorySize64 / 1MB)
        WsMB   = [math]::Round($p.WorkingSet64 / 1MB)
    }
}
$rows | Sort-Object PrivMB -Descending | Format-Table -AutoSize
Get-Process cef-spotify-spike -ErrorAction SilentlyContinue | Stop-Process -Force
