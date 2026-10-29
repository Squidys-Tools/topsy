# End-to-end check of a drag that carries text, not files, on a live desktop:
#   real OLE text drag -> synthetic jiggle -> the hook opens the tray (it can't tell what is being
#   dragged) -> carry the drag onto the island -> the drop target refuses it -> the tray must fold
#   at once, while the button is still down -> release: nothing lands on the shelf.
# Moves the real cursor for ~4s. All input stays inside the fixture window and the island.
# usage: e2e-textdrag.ps1 [-Exe target\release\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static IntPtr VisibleWindowOf(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
"@
[void][Native]::SetProcessDPIAware()
$LEFTDOWN = 0x2; $LEFTUP = 0x4

$work = Join-Path $env:TEMP 'topsy-e2e-textdrag'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
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
function Move-Cursor($x, $y) {
    $nx = [int]([math]::Round($x * 65535 / ($screen.Width - 1)))
    $ny = [int]([math]::Round($y * 65535 / ($screen.Height - 1)))
    [Native]::mouse_event(0x8001, $nx, $ny, 0, [UIntPtr]::Zero)
}
function Island-Height {
    $h = [Native]::VisibleWindowOf($app.Id)
    if ($h -eq [IntPtr]::Zero) { throw 'the island has no visible window' }
    $r = New-Object Native+RECT; [void][Native]::GetWindowRect($h, [ref]$r); $r.Bottom - $r.Top
}
function Is-Open { (Island-Height) -ge 100 * $scale }
$srcX = 300; $srcY = 500

$env:TOPSY_LOG = '1'
$env:TOPSY_STATE = Join-Path $work 'shelf.state'
$app = Start-Process $Exe -PassThru -RedirectStandardError $appLog
$src = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\drag-source.ps1", '"topsy selected text"', $srcX, $srcY, $srcLog, '-Text') -PassThru
$failed = $null
try {
    if (-not (Wait-Log $srcLog 'source: ready' 8000)) { throw 'drag source never came up' }
    Start-Sleep -Milliseconds 600

    $px = $srcX + 160; $py = $srcY + 110
    Move-Cursor $px $py
    Start-Sleep -Milliseconds 100
    [Native]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    foreach ($d in 6, 12, 18) { Move-Cursor ($px + $d) $py; Start-Sleep -Milliseconds 10 }
    $x = $px + 18
    for ($stroke = 0; $stroke -lt 7; $stroke++) {
        $dir = if ($stroke % 2 -eq 0) { 1 } else { -1 }
        for ($i = 0; $i -lt 6; $i++) { $x += $dir * 14; Move-Cursor $x $py; Start-Sleep -Milliseconds 8 }
    }
    if (-not (Wait-Log $appLog 'hook Jiggle' 1500)) { throw 'jiggle was not detected by the hook' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Is-Open)) { if ($sw.ElapsedMilliseconds -gt 3000) { throw 'the jiggle did not open the tray' }; Start-Sleep -Milliseconds 30 }
    Start-Sleep -Milliseconds 450
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'textdrag-1-open.png')

    # Carry the drag onto the tray, as the file drag does.
    $tx = $screen.X + $screen.Width / 2
    $ty = $screen.Y + 70 * $scale
    $pt = New-Object Native+POINT; $pt.X = [int]$tx; $pt.Y = [int]$ty
    $ownerPid = 0; [void][Native]::GetWindowThreadProcessId([Native]::WindowFromPoint($pt), [ref]$ownerPid)
    if ($ownerPid -ne $app.Id) { throw "the island is not under the drop point (that window belongs to pid $ownerPid); aborting drag" }
    $fx = $x; $fy = $py
    for ($i = 1; $i -le 20; $i++) {
        Move-Cursor ($fx + ($tx - $fx) * $i / 20) ($fy + ($ty - $fy) * $i / 20)
        Start-Sleep -Milliseconds 15
    }

    # The button is still down: the refusal alone must fold the tray, not the release.
    if (-not (Wait-Log $appLog 'drop-target Refused \(mode Tray\)' 1500)) { throw 'the drop target never reported the text drag as refused' }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while (Is-Open) { if ($sw.ElapsedMilliseconds -gt 1000) { throw 'the tray stayed open after the drag was refused' }; Start-Sleep -Milliseconds 30 }
    if (Select-String -Path $appLog -Pattern 'hook ButtonUp' -Quiet) { throw 'the tray folded only after the button was released' }
    Start-Sleep -Milliseconds 300
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'textdrag-2-folded.png')

    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 600
    if (Select-String -Path $appLog -Pattern 'drop \[|shelf now' -Quiet) { throw 'the refused text drag put something on the shelf' }
    Wait-Log $srcLog 'effect=' 2000 | Out-Null
} catch {
    $failed = $_
    [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
    [Native]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
} finally {
    Start-Sleep -Milliseconds 200
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $src.Id -Force -ErrorAction SilentlyContinue
}
"--- app log (render lines omitted)"; if (Test-Path $appLog) { Get-Content $appLog | Where-Object { $_ -notmatch 'topsy: render$' } }
"--- source log"; if (Test-Path $srcLog) { Get-Content $srcLog }
if ($failed) { "FAIL: $($failed.Exception.Message)"; exit 1 }
"PASS"
