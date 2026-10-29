# Checks two overlay promises on a live desktop:
#   1. Only the pill is hit-testable: the island's window is exactly the pill, so the screen
#      around it belongs to whatever is underneath.
#   2. The overlay never takes foreground focus from the app the user is working in: the
#      foreground window must be the very same one after launch.
# Use a release build. A debug build is a console app, and its console window takes focus.
# usage: check-overlay.ps1 [-Exe target\release\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class N {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int i);
}
"@
[void][N]::SetProcessDPIAware()
function Owner-Pid([IntPtr]$h) { $p = 0; [void][N]::GetWindowThreadProcessId($h, [ref]$p); $p }
function Pid-At($x, $y) {
    $pt = New-Object N+POINT; $pt.X = [int]$x; $pt.Y = [int]$y
    Owner-Pid ([N]::WindowFromPoint($pt))
}

$scale = [N]::GetDpiForSystem() / 96.0
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = $screen.X + $screen.Width / 2
$fgBefore = [N]::GetForegroundWindow()
$env:TOPSY_STATE = Join-Path $env:TEMP 'topsy-overlay-state.txt'
$app = Start-Process $Exe -PassThru
$failures = @()
try {
    $mine = $app.Id
    # Wait for the window to actually appear (startup time varies with machine load).
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ((Pid-At $cx ((6 + 17) * $scale)) -ne $mine -and $sw.ElapsedMilliseconds -lt 15000) { Start-Sleep -Milliseconds 100 }
    "window up after $($sw.ElapsedMilliseconds)ms"
    Start-Sleep -Milliseconds 500

    # Idle pill is 168x34, 6px from the top of the screen, and the window is exactly that.
    $inside = Pid-At $cx ((6 + 17) * $scale)
    $belowPill = Pid-At $cx (100 * $scale)
    $besidePill = Pid-At ($cx + 150 * $scale) ((6 + 17) * $scale)
    $abovePill = Pid-At $cx (2 * $scale)         # the 6px gap above the pill
    "pid under pill:        $inside (topsy = $mine)"
    "pid below pill:        $belowPill"
    "pid beside pill:       $besidePill"
    "pid above pill:        $abovePill"
    if ($inside -ne $mine) { $failures += 'pill is not hit-testable' }
    if ($belowPill -eq $mine) { $failures += 'the area below the pill blocks clicks' }
    if ($besidePill -eq $mine) { $failures += 'the area beside the pill blocks clicks' }
    if ($abovePill -eq $mine) { $failures += 'the gap above the pill blocks clicks' }

    $fgNow = [N]::GetForegroundWindow()
    function Pname($i) { (Get-Process -Id $i -ErrorAction SilentlyContinue).ProcessName }
    "foreground before launch: $(Owner-Pid $fgBefore) ($(Pname (Owner-Pid $fgBefore))), after: $(Owner-Pid $fgNow) ($(Pname (Owner-Pid $fgNow)))"
    if ((Owner-Pid $fgNow) -eq $mine) { $failures += 'overlay stole foreground focus' }
    elseif ($fgNow -ne $fgBefore) { $failures += 'the foreground window changed across launch (is this a debug build? its console window takes focus)' }
} finally {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
}
if ($failures) { "FAIL: " + ($failures -join '; '); exit 1 }
"PASS"
