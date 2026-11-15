# Checks the overlay promises on a live desktop:
#   1. At rest the notch is hidden: nothing of the island is on screen or takes clicks.
#   2. Resting the cursor on the top edge above it brings the notch down, and only the notch
#      is hit-testable: the screen around it belongs to whatever is underneath.
#   3. Moving away folds it up again.
#   4. The overlay never takes foreground focus from the app the user is working in: the
#      foreground window must be the very same one after launch and after the reveal.
#   5. With a fullscreen window in front (scripts/fullscreen.ps1), the top edge does nothing.
# Moves the real cursor along the top of the screen and covers it for a few seconds.
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
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
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
# Injected moves reach the low-level mouse hook; SetCursorPos does not reliably.
function Move-Cursor($x, $y) {
    $nx = [int]([math]::Round($x * 65535 / ($screen.Width - 1)))
    $ny = [int]([math]::Round($y * 65535 / ($screen.Height - 1)))
    [N]::mouse_event(0x8001, $nx, $ny, 0, [UIntPtr]::Zero)   # MOVE | ABSOLUTE
}
function Wait-For([scriptblock]$cond, $ms) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $cond) -and $sw.ElapsedMilliseconds -lt $ms) { Start-Sleep -Milliseconds 50 }
    & $cond
}
function Pname($i) { (Get-Process -Id $i -ErrorAction SilentlyContinue).ProcessName }

$notchY = 16 * $scale   # the middle of the 32 px peek
$away = @($cx, (300 * $scale))
Move-Cursor $away[0] $away[1]
$fgBefore = [N]::GetForegroundWindow()
$env:TOPSY_STATE = Join-Path $env:TEMP 'topsy-overlay-state.txt'
$env:TOPSY_LOG = '1'
$appLog = Join-Path $env:TEMP 'topsy-overlay.log'
$app = Start-Process $Exe -PassThru -RedirectStandardError $appLog
$failures = @()
try {
    $mine = $app.Id
    # A hidden notch can't be hit-tested, so wait for it to draw its first frame.
    if (-not (Wait-For { (Test-Path $appLog) -and (Select-String -Path $appLog -Pattern 'topsy: render' -SimpleMatch -Quiet) } 15000)) {
        throw 'the island never rendered'
    }
    Start-Sleep -Milliseconds 1000

    $hiddenAt = @((Pid-At $cx 0), (Pid-At $cx $notchY))
    "hidden: pid at top edge $($hiddenAt[0]), at notch $($hiddenAt[1]) (topsy = $mine)"
    if ($hiddenAt -contains $mine) { $failures += 'the hidden notch is still on screen and takes clicks' }

    # Rest on the top edge above the notch.
    Move-Cursor ($cx - 40 * $scale) 0; Start-Sleep -Milliseconds 30; Move-Cursor $cx 0
    $revealed = Wait-For { (Pid-At $cx $notchY) -eq $mine } 2000
    $below = Pid-At $cx (100 * $scale)
    $beside = Pid-At ($cx + 150 * $scale) $notchY
    "revealed: $revealed; pid below notch $below, beside notch $beside"
    if (-not $revealed) { $failures += 'resting on the top edge did not bring the notch down' }
    if ($below -eq $mine) { $failures += 'the area below the notch blocks clicks' }
    if ($beside -eq $mine) { $failures += 'the area beside the notch blocks clicks' }
    $fgRevealed = [N]::GetForegroundWindow()

    Move-Cursor $away[0] $away[1]
    $folded = Wait-For { (Pid-At $cx $notchY) -ne $mine -and (Pid-At $cx 0) -ne $mine } 3000
    "folded after leaving: $folded"
    if (-not $folded) { $failures += 'the notch stayed down after the cursor left' }

    $fgNow = [N]::GetForegroundWindow()
    "foreground before launch: $(Owner-Pid $fgBefore) ($(Pname (Owner-Pid $fgBefore))), after: $(Owner-Pid $fgNow) ($(Pname (Owner-Pid $fgNow)))"
    if ((Owner-Pid $fgNow) -eq $mine -or (Owner-Pid $fgRevealed) -eq $mine) { $failures += 'overlay stole foreground focus' }
    elseif ($fgNow -ne $fgBefore) { $failures += 'the foreground window changed (is this a debug build? its console window takes focus)' }

    $full = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\fullscreen.ps1") -PassThru
    if (-not (Wait-For { (Owner-Pid ([N]::GetForegroundWindow())) -eq $full.Id } 10000)) {
        $failures += 'the fullscreen fixture never came to the front, so the fullscreen case went unchecked'
    } else {
        Start-Sleep -Milliseconds 300
        Move-Cursor ($cx - 40 * $scale) 0; Start-Sleep -Milliseconds 30; Move-Cursor $cx 0
        $shown = Wait-For { (Pid-At $cx $notchY) -eq $mine } 2000
        "revealed over a fullscreen window: $shown"
        if ($shown) { $failures += 'the notch came down over a fullscreen window' }
        Move-Cursor $away[0] $away[1]
    }
} finally {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
    if ($full) { Stop-Process -Id $full.Id -Force -ErrorAction SilentlyContinue }
}
if ($failures) { "FAIL: " + ($failures -join '; '); exit 1 }
"PASS"
