# Helpers shared by the live checks in scripts/: waiting on a condition or a log line, injected
# cursor moves and clicks, the DPI scale, and starting and stopping topsy by its own PID.
# Importing it makes the script DPI aware and sets $scale (DPI / 96) and $screen (the primary
# display's bounds, in physical pixels).
# usage: Import-Module "$PSScriptRoot\lib\Topsy.psm1" -DisableNameChecking
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
if (-not ('TopsyLive.Input' -as [type])) {
    Add-Type @"
using System; using System.Runtime.InteropServices;
namespace TopsyLive {
  public static class Input {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  }
}
"@
}
[void][TopsyLive.Input]::SetProcessDPIAware()
$scale = [TopsyLive.Input]::GetDpiForSystem() / 96.0
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$LEFTDOWN = 0x2; $LEFTUP = 0x4; $RIGHTDOWN = 0x8; $RIGHTUP = 0x10

function Wait-Until([scriptblock]$cond, $ms) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 30 }
    return $false
}
function Wait-Log($path, $pattern, $ms) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) {
        # A fixture may be writing the log this instant; a locked read is just "not yet".
        try { if ((Test-Path $path) -and (Select-String -Path $path -Pattern $pattern -Quiet)) { return $true } } catch {}
        Start-Sleep -Milliseconds 25
    }
    return $false
}
function Log-Count($path, $pattern) { if (Test-Path $path) { @(Select-String -Path $path -Pattern $pattern).Count } else { 0 } }

# Injected absolute moves go through the real input path (SetCursorPos does not reliably reach
# low-level mouse hooks, which is what the checks exercise).
function Move-Cursor($x, $y) {
    $nx = [int]([math]::Round($x * 65535 / ($screen.Width - 1)))
    $ny = [int]([math]::Round($y * 65535 / ($screen.Height - 1)))
    [TopsyLive.Input]::mouse_event(0x8001, $nx, $ny, 0, [UIntPtr]::Zero)   # MOVE | ABSOLUTE
}
function Click($x, $y, [switch]$Right) {
    Move-Cursor $x $y; Start-Sleep -Milliseconds 150
    $down, $up = if ($Right) { $RIGHTDOWN, $RIGHTUP } else { $LEFTDOWN, $LEFTUP }
    [TopsyLive.Input]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 50
    [TopsyLive.Input]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}

# Sets the TOPSY_* variables in -Env and starts the app with them; Stop-Topsy removes them again.
# No -WindowStyle Hidden: it hides the first window the process shows, i.e. the island.
function Start-Topsy([string]$Exe, [hashtable]$Env, [string]$Log) {
    foreach ($name in $Env.Keys) { Set-Item -Path "Env:$name" -Value $Env[$name] }
    $start = @{ FilePath = $Exe; PassThru = $true }
    if ($Log) { $start.RedirectStandardError = $Log }
    try { $app = Start-Process @start } catch { Remove-TopsyEnv @($Env.Keys); throw }
    # Cache the handle, or HasExited and ExitCode can't be read once the process is gone.
    [void]$app.Handle
    $app | Add-Member -NotePropertyName TopsyEnv -NotePropertyValue @($Env.Keys)
    $app
}
# Stops only this process, by the PID captured at start; never by name (AGENTS.md).
function Stop-Topsy($app) {
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
    Remove-TopsyEnv $app.TopsyEnv
}
function Remove-TopsyEnv([string[]]$names) {
    foreach ($name in $names) { Remove-Item -Path "Env:$name" -ErrorAction SilentlyContinue }
}

Export-ModuleMember -Function Wait-Until, Wait-Log, Log-Count, Move-Cursor, Click, Start-Topsy, Stop-Topsy -Variable scale, screen
