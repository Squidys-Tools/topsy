# End-to-end check of the real input chain on a live desktop:
#   real OLE file drag -> synthetic jiggle -> global mouse hook -> tray expands
#   -> drag onto the pill -> GPUI drop target receives the file path.
# Moves the real cursor for ~3s. All input stays inside the fixture window and the island.
# usage: e2e-drag.ps1 [-Exe target\debug\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\debug\topsy.exe")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
"@
[void][Native]::SetProcessDPIAware()
$LEFTDOWN = 0x2; $LEFTUP = 0x4

$work = Join-Path $env:TEMP 'topsy-e2e'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$payload = Join-Path $work 'e2e-payload.txt'
Set-Content $payload 'topsy e2e'
$appLog = Join-Path $work 'app.log'
$srcLog = Join-Path $work 'source.log'
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

function Wait-Log($path, $pattern, $ms) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) {
        if ((Test-Path $path) -and (Select-String -Path $path -Pattern $pattern -Quiet)) { return $true }
        Start-Sleep -Milliseconds 25
    }
    return $false
}
$scale = [Native]::GetDpiForSystem() / 96.0
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
# Injected absolute moves go through the real input path (SetCursorPos does not
# reliably reach low-level mouse hooks, which is exactly what we're testing).
function Move-Cursor($x, $y) {
    $nx = [int]([math]::Round($x * 65535 / ($screen.Width - 1)))
    $ny = [int]([math]::Round($y * 65535 / ($screen.Height - 1)))
    [Native]::mouse_event(0x8001, $nx, $ny, 0, [UIntPtr]::Zero)   # MOVE | ABSOLUTE
}
$srcX = 300; $srcY = 500

$env:TOPSY_LOG = '1'
$env:TOPSY_STATE = Join-Path $work 'shelf.state'
# No -WindowStyle Hidden: it hides the first window the process shows, i.e. the island.
$app = Start-Process $Exe -PassThru -RedirectStandardError $appLog
$src = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\drag-source.ps1", $payload, $srcX, $srcY, $srcLog) -PassThru
$failed = $null
try {
    if (-not (Wait-Log $srcLog 'source: ready' 8000)) { throw 'drag source never came up' }
    Start-Sleep -Milliseconds 600

    # Press inside the fixture window and start the drag.
    $px = $srcX + 160; $py = $srcY + 110
    Move-Cursor $px $py
    Start-Sleep -Milliseconds 100
    [Native]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    # Nudge so the OLE drag begins, then jiggle side to side.
    foreach ($d in 6, 12, 18) { Move-Cursor ($px + $d) $py; Start-Sleep -Milliseconds 10 }
    $x = $px + 18
    for ($stroke = 0; $stroke -lt 7; $stroke++) {
        $dir = if ($stroke % 2 -eq 0) { 1 } else { -1 }
        for ($i = 0; $i -lt 6; $i++) { $x += $dir * 14; Move-Cursor $x $py; Start-Sleep -Milliseconds 8 }
    }
    if (-not (Wait-Log $appLog 'hook Jiggle' 1500)) { throw 'jiggle was not detected by the hook' }
    Start-Sleep -Milliseconds 450   # let the spring settle
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'e2e-1-expanded.png')
    $fgPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::GetForegroundWindow(), [ref]$fgPid)
    if ($fgPid -eq $app.Id) { throw 'expanding the tray stole foreground focus' }

    # Carry the drag up to the pill in steps so DragOver fires along the way.
    $tx = $screen.X + $screen.Width / 2
    $ty = $screen.Y + (6 + 59) * $scale

    # Safety: never carry a drag to the top of the screen unless the island is really
    # there, or the drop lands on whatever app is underneath. The island's window is the
    # pill, so the window under the drop point must be ours.
    $pt = New-Object Native+POINT; $pt.X = [int]$tx; $pt.Y = [int]$ty
    $ownerPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::WindowFromPoint($pt), [ref]$ownerPid)
    if ($ownerPid -ne $app.Id) { throw "the island is not under the drop point (that window belongs to pid $ownerPid); aborting drag" }
    $fx = $x; $fy = $py
    for ($i = 1; $i -le 20; $i++) {
        Move-Cursor ($fx + ($tx - $fx) * $i / 20) ($fy + ($ty - $fy) * $i / 20)
        Start-Sleep -Milliseconds 15
    }
    Start-Sleep -Milliseconds 300
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'e2e-2-hover.png')
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 500
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'e2e-3-dropped.png')
    if (-not (Wait-Log $appLog 'drop \[' 1500)) { throw 'island never received the dropped file' }
    if (-not (Select-String -Path $appLog -Pattern 'e2e-payload.txt' -Quiet)) { throw 'drop did not carry the expected file name' }
    Wait-Log $srcLog 'effect=' 2000 | Out-Null
} catch {
    $failed = $_
    # Cancel any drag still in flight so we don't leave the mouse button stuck.
    [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
} finally {
    Start-Sleep -Milliseconds 200
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $src.Id -Force -ErrorAction SilentlyContinue
}
"--- app log"; if (Test-Path $appLog) { Get-Content $appLog }
"--- source log"; if (Test-Path $srcLog) { Get-Content $srcLog }
if ($failed) { "FAIL: $($failed.Exception.Message)"; exit 1 }
"PASS"
