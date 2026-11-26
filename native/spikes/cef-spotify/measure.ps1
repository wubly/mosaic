# Samples memory and CPU of every cef-spotify-spike process (browser + its CEF children) while
# the spike runs. Start the spike first, start playback, then run this in a second terminal.
#   .\measure.ps1 -Seconds 60 -Label sdk-lean
# Prints one line per sample and a summary; appends the summary to logs\measurements.csv.
param(
    [int]$Seconds = 60,
    [int]$Interval = 5,
    [string]$Label = "run"
)
$ErrorActionPreference = "Stop"
$name = "cef-spotify-spike"
$cores = [Environment]::ProcessorCount
$samples = @()

function Snapshot {
    $procs = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    [pscustomobject]@{
        At      = Get-Date
        Count   = $procs.Count
        Private = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum
        Working = ($procs | Measure-Object WorkingSet64 -Sum).Sum
        Cpu     = ($procs | ForEach-Object { $_.TotalProcessorTime.TotalSeconds } | Measure-Object -Sum).Sum
    }
}

$prev = Snapshot
if ($prev.Count -eq 0) { Write-Error "No $name process is running." }
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
    Start-Sleep -Seconds $Interval
    $cur = Snapshot
    $wall = ($cur.At - $prev.At).TotalSeconds
    $cpuPct = if ($wall -gt 0) { 100 * ($cur.Cpu - $prev.Cpu) / ($wall * $cores) } else { 0 }
    $row = [pscustomobject]@{
        Processes = $cur.Count
        PrivateMB = [math]::Round($cur.Private / 1MB, 1)
        WorkingMB = [math]::Round($cur.Working / 1MB, 1)
        CpuPct    = [math]::Round($cpuPct, 2)
    }
    $samples += $row
    $row | Format-Table -HideTableHeaders | Out-String | ForEach-Object { $_.Trim() } | Write-Host
    $prev = $cur
}

$summary = [pscustomobject]@{
    Label         = $Label
    Date          = Get-Date -Format "s"
    Processes     = ($samples | Measure-Object Processes -Maximum).Maximum
    PrivateMBAvg  = [math]::Round(($samples | Measure-Object PrivateMB -Average).Average, 1)
    PrivateMBMax  = ($samples | Measure-Object PrivateMB -Maximum).Maximum
    WorkingMBAvg  = [math]::Round(($samples | Measure-Object WorkingMB -Average).Average, 1)
    CpuPctAvg     = [math]::Round(($samples | Measure-Object CpuPct -Average).Average, 2)
    Cores         = $cores
}
$summary | Format-List
New-Item -ItemType Directory -Force (Join-Path $PSScriptRoot "logs") | Out-Null
$summary | Export-Csv -Append -NoTypeInformation (Join-Path $PSScriptRoot "logs\measurements.csv")
