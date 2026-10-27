# How many times does the island re-render while idle? (needs a build with the `render` log line)
# usage: count-renders.ps1 [-Seconds 10]
param(
    [int]$Seconds = 10,
    [string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe"
)
$err = Join-Path $env:TEMP 'topsy-render.log'
Remove-Item $err -ErrorAction SilentlyContinue
$env:TOPSY_LOG = '1'
$app = Start-Process $Exe -PassThru -RedirectStandardError $err
Start-Sleep -Seconds 6                      # startup + first frames
$startLines = @(Get-Content $err -ErrorAction SilentlyContinue | Where-Object { $_ -match 'render' }).Count
Start-Sleep -Seconds $Seconds
$endLines = @(Get-Content $err -ErrorAction SilentlyContinue | Where-Object { $_ -match 'render' }).Count
Stop-Process -Id $app.Id -Force
Remove-Item Env:\TOPSY_LOG -ErrorAction SilentlyContinue
"renders: $startLines during startup, $($endLines - $startLines) over the next ${Seconds}s of idle"
