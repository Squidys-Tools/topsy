# Checks two overlay promises on a live desktop:
#   1. Only the pill is hit-testable; the transparent rest of the window is click-through.
#   2. The overlay never takes foreground focus from the app the user is working in.
# usage: check-overlay.ps1 [-Exe target\debug\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\debug\topsy.exe")
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
$app = Start-Process $Exe -PassThru
$failures = @()
try {
    $mine = $app.Id
    # Wait for the window to actually appear (startup time varies with machine load).
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ((Pid-At $cx ((6 + 17) * $scale)) -ne $mine -and $sw.ElapsedMilliseconds -lt 15000) { Start-Sleep -Milliseconds 100 }
    "window up after $($sw.ElapsedMilliseconds)ms"
    Start-Sleep -Milliseconds 500

    # Idle pill is 168x34 at 6px from the top, inside a 440x150 window.
    $inside = Pid-At $cx ((6 + 17) * $scale)
    $belowPill = Pid-At $cx (100 * $scale)       # inside the OS window, below the pill
    $besidePill = Pid-At ($cx + 150 * $scale) ((6 + 17) * $scale)   # inside the window, right of the pill
    "pid under pill:        $inside (topsy = $mine)"
    "pid below pill:        $belowPill"
    "pid beside pill:       $besidePill"
    if ($inside -ne $mine) { $failures += 'pill is not hit-testable' }
    if ($belowPill -eq $mine) { $failures += 'transparent area below the pill blocks clicks' }
    if ($besidePill -eq $mine) { $failures += 'transparent area beside the pill blocks clicks' }

    $fgNow = [N]::GetForegroundWindow()
    function Pname($i) { (Get-Process -Id $i -ErrorAction SilentlyContinue).ProcessName }
    "foreground before launch: $(Owner-Pid $fgBefore) ($(Pname (Owner-Pid $fgBefore))), after: $(Owner-Pid $fgNow) ($(Pname (Owner-Pid $fgNow)))"
    if ((Owner-Pid $fgNow) -eq $mine) { $failures += 'overlay stole foreground focus' }

    $ex = [N]::GetWindowLongW($fgNow, -20)
} finally {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
}
if ($failures) { "FAIL: " + ($failures -join '; '); exit 1 }
"PASS"
