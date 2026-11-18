# Idle footprint of the release build, with and without the global mouse hook.
# usage: measure-idle.ps1 [-Rounds 2] [-WarmupSec 8] [-WindowSec 15]
param(
    [int]$Rounds = 2,
    [int]$WarmupSec = 8,
    [int]$WindowSec = 15,
    [string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe"
)
Import-Module "$PSScriptRoot\lib\Topsy.psm1" -DisableNameChecking

function Measure-Idle([string]$Label, [bool]$NoHook) {
    $topsyEnv = @{ TOPSY_STATE = (Join-Path $env:TEMP 'topsy-idle-state.txt') }
    if ($NoHook) { $topsyEnv.TOPSY_NO_HOOK = '1' } else { Remove-Item Env:\TOPSY_NO_HOOK -ErrorAction SilentlyContinue }
    $app = Start-Topsy -Exe $Exe -Env $topsyEnv
    Start-Sleep -Seconds $WarmupSec
    $before = (Get-Process -Id $app.Id).TotalProcessorTime.TotalMilliseconds
    Start-Sleep -Seconds $WindowSec
    $p = Get-Process -Id $app.Id
    $cpuMs = $p.TotalProcessorTime.TotalMilliseconds - $before
    $pct = $cpuMs / ($WindowSec * 1000) * 100
    '{0,-10} cpu {1,5:N0} ms over {2}s = {3:N2}% of a core | working set {4:N1} MB | private {5:N1} MB | {6} threads' -f `
        $Label, $cpuMs, $WindowSec, $pct, ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB), $p.Threads.Count
    Stop-Topsy $app
}

for ($i = 0; $i -lt $Rounds; $i++) {
    Measure-Idle 'with hook' $false
    Measure-Idle 'no hook' $true
}
