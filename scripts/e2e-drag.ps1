# End-to-end check of the real input chain on a live desktop:
#   real OLE file drag -> synthetic jiggle -> global mouse hook -> the basket opens under the
#   pointer -> release on it -> GPUI drop target receives the file path -> the basket stays open
#   holding it -> drag its stack out to a separate app -> the file lands there and the basket closes.
# Moves the real cursor for ~6s. All input stays inside the fixture windows and the basket.
# usage: e2e-drag.ps1 [-Exe target\debug\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\debug\topsy.exe")
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\Topsy.psm1" -DisableNameChecking
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  // The basket: the one of topsy's windows not hanging from the top edge, as the notch does.
  public static IntPtr BasketOf(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; RECT r; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h) && GetWindowRect(h, out r) && r.Top > 0) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
"@
$LEFTDOWN = 0x2; $LEFTUP = 0x4

$work = Join-Path $env:TEMP 'topsy-e2e'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$payload = Join-Path $work 'e2e-payload.txt'
Set-Content $payload 'topsy e2e'
$appLog = Join-Path $work 'app.log'
$srcLog = Join-Path $work 'source.log'
$tgtLog = Join-Path $work 'target.log'
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

$srcX = 300; $srcY = 500
$tgtX = 1000; $tgtY = 450
function Basket-Rect {
    $h = [Native]::BasketOf($app.Id)
    if ($h -eq [IntPtr]::Zero) { throw 'the basket has no visible window' }
    $r = New-Object Native+RECT; [void][Native]::GetWindowRect($h, [ref]$r); $r
}
function Is-Open { $r = Basket-Rect; ($r.Bottom - $r.Top) -ge 200 * $scale }
function Shoot-Basket($name) {
    $r = Basket-Rect; $pad = [int](24 * $scale)
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots $name) -Rect ($r.Left - $pad), ($r.Top - $pad), ($r.Right - $r.Left + 2 * $pad), ($r.Bottom - $r.Top + 2 * $pad)
}

$app = Start-Topsy -Exe $Exe -Env @{ TOPSY_LOG = '1'; TOPSY_STATE = (Join-Path $work 'shelf.state') } -Log $appLog
$src = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\drag-source.ps1", $payload, $srcX, $srcY, $srcLog) -PassThru
$failed = $null; $tgt = $null
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
    if (-not (Wait-Log $appLog 'basket at' 1000)) { throw 'the jiggle did not open the basket' }
    Start-Sleep -Milliseconds 450   # let the spring settle
    Shoot-Basket 'e2e-1-expanded.png'
    $fgPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::GetForegroundWindow(), [ref]$fgPid)
    if ($fgPid -eq $app.Id) { throw 'opening the basket stole foreground focus' }

    # The basket opens centred on the pointer: drop a little off-centre, on it.
    $tx = $x + 20 * $scale
    $ty = $py + 10 * $scale

    # Safety: never release the drag unless the basket is really under the pointer, or the
    # drop lands on whatever app is underneath. The window under the drop point must be ours.
    $pt = New-Object Native+POINT; $pt.X = [int]$tx; $pt.Y = [int]$ty
    $ownerPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::WindowFromPoint($pt), [ref]$ownerPid)
    if ($ownerPid -ne $app.Id) { throw "the basket is not under the drop point (that window belongs to pid $ownerPid); aborting drag" }
    $fx = $x; $fy = $py
    for ($i = 1; $i -le 5; $i++) {
        Move-Cursor ($fx + ($tx - $fx) * $i / 5) ($fy + ($ty - $fy) * $i / 5)
        Start-Sleep -Milliseconds 15
    }
    Start-Sleep -Milliseconds 300
    Shoot-Basket 'e2e-2-hover.png'
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 500
    Shoot-Basket 'e2e-3-dropped.png'
    if (-not (Wait-Log $appLog 'basket drop \[' 1500)) { throw 'the basket never received the dropped file' }
    if (-not (Select-String -Path $appLog -Pattern 'e2e-payload.txt' -Quiet)) { throw 'drop did not carry the expected file name' }
    Wait-Log $srcLog 'effect=' 2000 | Out-Null

    # Droppy's basket keeps what it holds: still open well after the drag ended.
    Start-Sleep -Milliseconds 2500
    if (-not (Is-Open)) { throw 'the basket closed by itself while holding a file' }
    Shoot-Basket 'e2e-4-holding.png'

    # Drag the stack out to another app: the file lands there and the basket, now empty, closes.
    $tgt = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\drop-target.ps1", $tgtX, $tgtY, $tgtLog) -PassThru
    if (-not (Wait-Log $tgtLog 'target: ready' 8000)) { throw 'the drop target never came up' }
    Start-Sleep -Milliseconds 600
    $r = Basket-Rect
    # The front card of the stack sits a little above the basket's middle.
    $sx = ($r.Left + $r.Right) / 2; $sy = ($r.Top + $r.Bottom) / 2 - 10 * $scale
    $pt = New-Object Native+POINT; $pt.X = [int]$sx; $pt.Y = [int]$sy
    $ownerPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::WindowFromPoint($pt), [ref]$ownerPid)
    if ($ownerPid -ne $app.Id) { throw "the basket's stack is not under the pointer (pid $ownerPid)" }
    Move-Cursor $sx $sy; Start-Sleep -Milliseconds 150
    [Native]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    $dx = $tgtX + 160; $dy = $tgtY + 100
    for ($i = 1; $i -le 20; $i++) {
        Move-Cursor ($sx + ($dx - $sx) * $i / 20) ($sy + ($dy - $sy) * $i / 20)
        Start-Sleep -Milliseconds 20
    }
    Start-Sleep -Milliseconds 300
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    if (-not (Wait-Log $tgtLog 'target: dropped .*e2e-payload\.txt' 3000)) { throw 'the drop target never received the basket''s file' }
    if (-not (Wait-Log $appLog 'basket dragout dropped' 3000)) { throw 'the basket did not see its drag out land' }
    Start-Sleep -Milliseconds 600
    if (Is-Open) { throw 'the basket stayed open after its last file was dragged out' }
} catch {
    $failed = $_
    # Cancel any drag still in flight so we don't leave the mouse button stuck.
    [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
} finally {
    Start-Sleep -Milliseconds 200
    Stop-Topsy $app
    Stop-Process -Id $src.Id -Force -ErrorAction SilentlyContinue
    if ($tgt) { Stop-Process -Id $tgt.Id -Force -ErrorAction SilentlyContinue }
}
"--- app log"; if (Test-Path $appLog) { Get-Content $appLog }
"--- source log"; if (Test-Path $srcLog) { Get-Content $srcLog }
"--- target log"; if (Test-Path $tgtLog) { Get-Content $tgtLog }
if ($failed) { "FAIL: $($failed.Exception.Message)"; exit 1 }
"PASS"
