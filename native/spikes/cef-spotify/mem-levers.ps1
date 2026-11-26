# Measures the SDK page with the player connected (idle, nothing playing) under each memory
# lever, adding one lever at a time. Needs a saved sign-in (run .\run.ps1 -ClientId <id> and
# sign in once). Results: console table + logs\levers.csv.
#   .\mem-levers.ps1 -ClientId <id> [-Settle 15]
param(
    [Parameter(Mandatory = $true)] [string]$ClientId,
    [int]$Settle = 15
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$exe = Join-Path $here "target\release\cef-spotify-spike.exe"
$logs = Join-Path $here "logs"
New-Item -ItemType Directory -Force $logs | Out-Null

$configs = @(
    @{ Name = "full (no trimming)"; Args = @("--spike-full") },
    @{ Name = "base trimming";      Args = @("--spike-levers=") },
    @{ Name = "+iso";               Args = @("--spike-levers=iso") },
    @{ Name = "+gpu";               Args = @("--spike-levers=iso,gpu") },
    @{ Name = "+net";               Args = @("--spike-levers=iso,gpu,net") },
    @{ Name = "+audio";             Args = @("--spike-levers=iso,gpu,net,audio") },
    @{ Name = "+lowend";            Args = @("--spike-levers=iso,gpu,net,audio,lowend") },
    @{ Name = "+v8 (all)";          Args = @("--spike-levers=iso,gpu,net,audio,lowend,v8") }
)

function Stop-Spike {
    Get-Process -Name "cef-spotify-spike" -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 2
}

$results = @()
foreach ($c in $configs) {
    Stop-Spike
    $out = Join-Path $logs ("levers-{0}.log" -f ($c.Name -replace '[^a-z0-9]+', '-'))
    $argList = @("--spike-page=sdk", "--spike-hidden", "--spike-client-id=$ClientId") + $c.Args
    $p = Start-Process -FilePath $exe -ArgumentList $argList -RedirectStandardOutput $out `
        -RedirectStandardError "$out.err" -PassThru -WindowStyle Hidden
    $deadline = (Get-Date).AddSeconds(40)
    $ready = $false
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        if ((Test-Path $out) -and (Select-String -Path $out -Pattern '"kind":"ready"' -Quiet)) { $ready = $true; break }
        if ($p.HasExited) { break }
    }
    if (-not $ready) {
        Write-Warning "$($c.Name): player never became ready (see $out)"
        $results += [pscustomobject]@{ Config = $c.Name; Ready = $false }
        continue
    }
    Start-Sleep -Seconds $Settle
    $procs = @(Get-Process -Name "cef-spotify-spike" -ErrorAction SilentlyContinue)
    $results += [pscustomobject]@{
        Config    = $c.Name
        Ready     = $true
        Processes = $procs.Count
        PrivateMB = [math]::Round(($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB)
        WorkingMB = [math]::Round(($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB)
    }
    Write-Host ("{0,-20} {1} procs  {2} MB private  {3} MB working set" -f $c.Name, $procs.Count, $results[-1].PrivateMB, $results[-1].WorkingMB)
}
Stop-Spike
$results | Format-Table -AutoSize
$results | Export-Csv -NoTypeInformation (Join-Path $logs "levers.csv")
