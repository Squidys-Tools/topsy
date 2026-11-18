# How many times does the island re-render while idle? (needs a build with the `render` log line)
# usage: count-renders.ps1 [-Seconds 10]
param(
    [int]$Seconds = 10,
    [string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe"
)
Import-Module "$PSScriptRoot\lib\Topsy.psm1" -DisableNameChecking
$err = Join-Path $env:TEMP 'topsy-render.log'
Remove-Item $err -ErrorAction SilentlyContinue
$app = Start-Topsy -Exe $Exe -Env @{ TOPSY_LOG = '1'; TOPSY_STATE = (Join-Path $env:TEMP 'topsy-count-state.txt') } -Log $err
Start-Sleep -Seconds 6                      # startup + first frames
$startLines = @(Get-Content $err -ErrorAction SilentlyContinue | Where-Object { $_ -match 'render' }).Count
Start-Sleep -Seconds $Seconds
$endLines = @(Get-Content $err -ErrorAction SilentlyContinue | Where-Object { $_ -match 'render' }).Count
Stop-Topsy $app
"renders: $startLines during startup, $($endLines - $startLines) over the next ${Seconds}s of idle"
