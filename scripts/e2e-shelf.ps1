# End-to-end check of the shelf on a live desktop, driving the real input chain:
#   click the pill to open the tray -> drag a chip out onto a separate app (it must receive the
#   file and the chip must leave the shelf) -> Escape mid-drag (the chip must stay) -> remove a
#   chip with its cross -> click a chip to open it -> the bin -> drop a file from another app onto
#   the island (it must still be a drop target after being dragged out of).
# Moves the real cursor for ~30s. Opens (and closes) one Explorer window for the open check.
# usage: e2e-shelf.ps1 [-Exe target\release\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe")
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\Topsy.psm1" -DisableNameChecking
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static IntPtr VisibleWindowOf(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
  public static IntPtr[] VisibleWindowsOfClass(string cls) {
    var list = new List<IntPtr>();
    EnumWindows((h, l) => { var s = new StringBuilder(256); GetClassName(h, s, 256); if (s.ToString() == cls && IsWindowVisible(h)) list.Add(h); return true; }, IntPtr.Zero);
    return list.ToArray();
  }
}
"@
$LEFTDOWN = 0x2; $LEFTUP = 0x4

$work = Join-Path $env:TEMP 'topsy-e2e-shelf'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$files = 'shelf-a.txt', 'shelf-b.txt', 'shelf-c.txt' | ForEach-Object { $p = Join-Path $work $_; Set-Content $p "topsy $_"; $p }
$dir = Join-Path $work 'shelf-dir'; New-Item -ItemType Directory $dir | Out-Null
$appLog = Join-Path $work 'app.log'
$tgtLog = Join-Path $work 'target.log'
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

# Settles 120 ms before pressing, not the module's 150.
function Click($x, $y) {
    Move-Cursor $x $y; Start-Sleep -Milliseconds 120
    [Native]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 50
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
}
function Pill-Rect {
    $h = [Native]::VisibleWindowOf($app.Id)
    if ($h -eq [IntPtr]::Zero) { throw 'the island has no visible window' }
    $r = New-Object Native+RECT; [void][Native]::GetWindowRect($h, [ref]$r); $r
}
function Is-Open { (Pill-Rect).Bottom - (Pill-Rect).Top -ge 100 * $scale }
# Click the idle pill to open the tray, and wait for the spring to settle.
function Ensure-Open {
    if (-not (Is-Open)) {
        $r = Pill-Rect
        Click (($r.Left + $r.Right) / 2) (($r.Top + $r.Bottom) / 2)
        $sw = [Diagnostics.Stopwatch]::StartNew()
        while (-not (Is-Open)) { if ($sw.ElapsedMilliseconds -gt 3000) { throw 'clicking the pill did not open the tray' }; Start-Sleep -Milliseconds 30 }
        Start-Sleep -Milliseconds 500
    }
}
# Where things are inside the open tray, from its layout: the window leaves 16px either side for
# the notch's ears, then 10px padding, a 28px row of round buttons, 6px gap, and a row of 104px
# chips 6px apart, each a 66px picture frame (2px down, centred) over its name.
$ear = 16   # notch::EAR
function Chip-Center($i) { $r = Pill-Rect; @(($r.Left + ($ear + 10 + 110 * $i + 52) * $scale), ($r.Top + 79 * $scale)) }
function Cross-Center($i) { $r = Pill-Rect; @(($r.Left + ($ear + 10 + 110 * $i + 78) * $scale), ($r.Top + 53 * $scale)) }
function Assert-Ours($x, $y, $what) {
    $pt = New-Object Native+POINT; $pt.X = [int]$x; $pt.Y = [int]$y
    $ownerPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::WindowFromPoint($pt), [ref]$ownerPid)
    if ($ownerPid -ne $app.Id) { throw "$what is not under the cursor (that window belongs to pid $ownerPid); aborting" }
}
# Press a chip and drag it onto the fixture window. -Escape cancels mid-drag instead of dropping.
function Drag-Chip($i, [switch]$Escape) {
    $c = Chip-Center $i
    Assert-Ours $c[0] $c[1] "chip $i"
    Move-Cursor $c[0] $c[1]; Start-Sleep -Milliseconds 150
    [Native]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 100
    $tx = $tgtX + 160; $ty = $tgtY + 100
    foreach ($d in 4, 8, 12, 16) { Move-Cursor ($c[0] + $d) $c[1]; Start-Sleep -Milliseconds 15 }
    $fx = $c[0] + 16; $fy = $c[1]
    for ($s = 1; $s -le 25; $s++) {
        Move-Cursor ($fx + ($tx - $fx) * $s / 25) ($fy + ($ty - $fy) * $s / 25)
        Start-Sleep -Milliseconds 15
    }
    Start-Sleep -Milliseconds 300
    if ($Escape) { [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 250; [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 150 }
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
}

$tgtX = 300; $tgtY = 500
$app = Start-Topsy -Exe $Exe -Log $appLog -Env @{
    TOPSY_LOG = '1'
    # Keep the notch down at rest, so there is something to click.
    TOPSY_IDLE = 'peek'
    TOPSY_SEED = ($files + $dir) -join ';'
    TOPSY_STATE = Join-Path $work 'shelf.state'
    # A playing song adds a row to the tray and moves the chips this clicks.
    TOPSY_NO_MEDIA = '1'
    TOPSY_NO_WIDGETS = '1'
}
$tgt = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\drop-target.ps1", $tgtX, $tgtY, $tgtLog) -PassThru
$failed = $null
$src = $null
try {
    if (-not (Wait-Log $appLog 'seeded 4' 8000)) { throw 'the shelf was never seeded' }
    if (-not (Wait-Log $tgtLog 'target: ready' 8000)) { throw 'the drop target never came up' }
    Start-Sleep -Milliseconds 600
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'shelf-0-idle.png')

    # 1. Click the pill: the tray opens with four chips.
    Ensure-Open
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'shelf-1-open.png')

    # 2. Drag chip 0 (shelf-a.txt) onto the other app: it must receive the file, and the chip leaves.
    Drag-Chip 0
    if (-not (Wait-Log $tgtLog 'target: dropped .*shelf-a\.txt' 3000)) { throw 'the drop target never received shelf-a.txt' }
    if (-not (Wait-Log $appLog 'dragout done shelf-a\.txt \(dropped\)' 3000)) { throw 'the island did not see the drag out complete' }
    if (-not (Wait-Log $appLog 'shelf now 3' 2000)) { throw 'the dragged-out chip did not leave the shelf' }
    if (Test-Path $files[0]) { } else { throw 'dragging out must not delete the original file' }

    # 3. Drag chip 0 (now shelf-b.txt) and press Escape: nothing is delivered, the chip stays.
    Ensure-Open
    Drag-Chip 0 -Escape
    if (-not (Wait-Log $appLog 'dragout done shelf-b\.txt \(cancelled\)' 3000)) { throw 'Escape did not cancel the drag out' }
    Start-Sleep -Milliseconds 300
    if (Select-String -Path $tgtLog -Pattern 'shelf-b' -Quiet) { throw 'the cancelled drag still delivered shelf-b.txt' }
    if ((Log-Count $appLog 'shelf now') -ne 2) { throw 'the cancelled drag changed the shelf' }   # seeded + shelf-a leaving

    # 4. The chip's cross removes it from the shelf (not from disk) without opening it.
    Ensure-Open
    $c = Chip-Center 0; Move-Cursor $c[0] $c[1]; Start-Sleep -Milliseconds 200
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'shelf-2-hover.png')
    $x = Cross-Center 0; Assert-Ours $x[0] $x[1] 'the remove cross'
    Click $x[0] $x[1]
    if (-not (Wait-Log $appLog 'removed shelf-b\.txt' 2000)) { throw 'the cross did not remove the chip' }
    if (-not (Wait-Log $appLog 'shelf now 2' 1000)) { throw 'the shelf did not shrink to two' }
    if (Select-String -Path $appLog -Pattern 'open shelf-b' -Quiet) { throw 'clicking the cross also opened the file' }
    if (-not (Test-Path $files[1])) { throw 'removing a chip must not delete the file' }

    # 5. Clicking a chip opens it: chip 1 is the folder, which opens in Explorer.
    Ensure-Open
    $before = @([Native]::VisibleWindowsOfClass('CabinetWClass'))
    $c = Chip-Center 1; Assert-Ours $c[0] $c[1] 'the folder chip'
    Click $c[0] $c[1]
    if (-not (Wait-Log $appLog 'open shelf-dir \(ok\)' 3000)) { throw 'clicking the folder chip did not open it' }
    Start-Sleep -Milliseconds 1500
    # Close only a window that is new (a folder opened as a tab of an existing window is left alone).
    foreach ($h in [Native]::VisibleWindowsOfClass('CabinetWClass')) { if ($before -notcontains $h) { [void][Native]::SendMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) } }

    # 6. The bin, with nothing picked, empties the shelf.
    Ensure-Open
    $r = Pill-Rect; $cx = $r.Left + ($ear + 436) * $scale; $cy = $r.Top + 24 * $scale
    Assert-Ours $cx $cy 'the bin button'
    Click $cx $cy
    if (-not (Wait-Log $appLog 'shelf now 0' 2000)) { throw 'the bin did not empty the shelf' }
    Start-Sleep -Milliseconds 300
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'shelf-3-empty.png')

    # 7. The island must still take drops from other apps after its own chips were dragged out
    #    (a drag out pauses its drop target and has to put it back).
    $srcLog = Join-Path $work 'source.log'
    $payload = Join-Path $work 'shelf-in.txt'; Set-Content $payload 'topsy shelf-in'
    $srcX = 700; $srcY = 500
    $src = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\drag-source.ps1", $payload, $srcX, $srcY, $srcLog) -PassThru
    if (-not (Wait-Log $srcLog 'source: ready' 8000)) { throw 'the external drag source never came up' }
    Start-Sleep -Milliseconds 600
    $r = Pill-Rect
    $tx = ($r.Left + $r.Right) / 2; $ty = $r.Top + 12 * $scale
    Assert-Ours $tx $ty 'the island'
    $px = $srcX + 160; $py = $srcY + 110
    Move-Cursor $px $py; Start-Sleep -Milliseconds 100
    [Native]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 100
    foreach ($d in 6, 12, 18) { Move-Cursor ($px + $d) $py; Start-Sleep -Milliseconds 10 }
    $fx = $px + 18
    for ($i = 1; $i -le 20; $i++) {
        Move-Cursor ($fx + ($tx - $fx) * $i / 20) ($py + ($ty - $py) * $i / 20)
        Start-Sleep -Milliseconds 15
    }
    Start-Sleep -Milliseconds 300
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    if (-not (Wait-Log $appLog 'drop \[.*shelf-in\.txt' 3000)) { throw 'the island stopped accepting drops after a drag out' }
    if (-not (Wait-Log $appLog 'shelf now 1' 1000)) { throw 'the dropped file did not land on the shelf' }
    Start-Sleep -Milliseconds 500
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'shelf-4-external-drop.png')
} catch {
    $failed = $_
    [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
} finally {
    Start-Sleep -Milliseconds 200
    Stop-Topsy $app
    Stop-Process -Id $tgt.Id -Force -ErrorAction SilentlyContinue
    if ($src) { Stop-Process -Id $src.Id -Force -ErrorAction SilentlyContinue }
}
"--- app log (render lines omitted)"; if (Test-Path $appLog) { Get-Content $appLog | Where-Object { $_ -notmatch 'topsy: render$' } }
"--- target log"; if (Test-Path $tgtLog) { Get-Content $tgtLog }
if ($srcLog -and (Test-Path $srcLog)) { "--- source log"; Get-Content $srcLog }
if ($failed) { "FAIL: $($failed.Exception.Message)"; exit 1 }
"PASS"
