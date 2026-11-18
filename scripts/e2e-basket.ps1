# End-to-end check of the basket's restore, its menus and rename on a live desktop:
#   a saved basket comes back after a restart (a file gone meanwhile is dropped) -> "Show basket"
#   in the notch's menu brings it up without taking focus -> the pill opens the list -> a tile's
#   Rename takes the focus, typing replaces the name before the extension, Enter renames the file
#   on disk and hands the focus back -> the new name is saved and comes back after a restart ->
#   Share opens Windows' share sheet for the file, which is then closed.
# Moves the real cursor and types into the basket for ~20s. Use a release build: a debug build's
# console takes the focus.
# usage: e2e-basket.ps1 [-Exe target\release\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe")
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\Topsy.psm1" -DisableNameChecking
$Exe = (Resolve-Path $Exe).Path
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  // The process's visible window of class `cls` (a menu is #32768).
  public static IntPtr WindowOf(uint pid, string cls) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p); if (p != pid || !IsWindowVisible(h)) return true;
      var s = new StringBuilder(256); GetClassName(h, s, 256); if (s.ToString() != cls) return true;
      found = h; return false; }, IntPtr.Zero);
    return found;
  }
  // Windows' share sheet: a frame of explorer's (the taskbar's process), shown and not cloaked,
  // narrower than the screen (explorer keeps a cloaked full-screen one).
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string title);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int s);
  public static IntPtr Sheet() {
    uint shell; GetWindowThreadProcessId(FindWindow("Shell_TrayWnd", null), out shell);
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; RECT r; int cloaked; GetWindowThreadProcessId(h, out p);
      var s = new StringBuilder(256); GetClassName(h, s, 256);
      if (p != shell || !IsWindowVisible(h) || s.ToString() != "ApplicationFrameWindow") return true;
      DwmGetWindowAttribute(h, 14, out cloaked, 4);
      if (cloaked == 0 && GetWindowRect(h, out r) && r.Top < 400 && r.Right - r.Left < 1200) { found = h; return false; }
      return true; }, IntPtr.Zero);
    return found;
  }
  public static bool ShareSheet() { return Sheet() != IntPtr.Zero; }
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  // The island hangs from the top edge; the basket floats below it. Neither is a menu.
  public static IntPtr Glass(uint pid, bool island) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; RECT r; GetWindowThreadProcessId(h, out p);
      if (p != pid || !IsWindowVisible(h) || !GetWindowRect(h, out r)) return true;
      var s = new StringBuilder(256); GetClassName(h, s, 256);
      if (s.ToString() == "#32768" || s.ToString() == "topsy-tray") return true;
      if (island ? (r.Top == 0 && r.Right - r.Left > 20) : r.Top > 0) { found = h; return false; }
      return true; }, IntPtr.Zero);
    return found;
  }
}
"@
$work = Join-Path $env:TEMP 'topsy-e2e-basket'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$first = Join-Path $work 'IMG_0042.txt'
$second = Join-Path $work 'notes.txt'
$renamed = Join-Path $work 'beach.txt'
$gone = Join-Path $work 'gone-meanwhile.txt'
Set-Content $first 'topsy basket a'; Set-Content $second 'topsy basket b'
$state = Join-Path $work 'shelf.state'
$basketState = Join-Path $work 'shelf.basket'
Set-Content $basketState "$first`n$gone`n$second" -Encoding utf8NoBOM
$appLog = Join-Path $work 'app.log'
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

function Tap-Key([byte]$vk) {
    [Native]::keybd_event($vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40
    [Native]::keybd_event($vk, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40
}
function Rect-Of([bool]$island) {
    $h = [Native]::Glass($app.Id, $island)
    if ($h -eq [IntPtr]::Zero) { throw "no visible $(if ($island) { 'island' } else { 'basket' }) window" }
    $r = New-Object Native+RECT; [void][Native]::GetWindowRect($h, [ref]$r); $r
}
function Basket-Open { $h = [Native]::Glass($app.Id, $false); if ($h -eq [IntPtr]::Zero) { return $false }; $r = Rect-Of $false; ($r.Bottom - $r.Top) -ge 200 * $scale }
function Foreground-Ours { $p = 0; [void][Native]::GetWindowThreadProcessId([Native]::GetForegroundWindow(), [ref]$p); $p -eq $app.Id }
function Shoot($name) {
    $r = Rect-Of $false; $pad = [int](24 * $scale)
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots $name) -Rect ($r.Left - $pad), ($r.Top - $pad), ($r.Right - $r.Left + 2 * $pad), ($r.Bottom - $r.Top + 2 * $pad)
}
function Use-Menu([scriptblock]$open, [byte]$key, $what) {
    & $open
    if (-not (Wait-Until { [Native]::WindowOf($app.Id, '#32768') -ne [IntPtr]::Zero } 2500)) { throw "the menu did not open ($what)" }
    Start-Sleep -Milliseconds 200
    Tap-Key $key
    if (-not (Wait-Until { [Native]::WindowOf($app.Id, '#32768') -eq [IntPtr]::Zero } 2000)) { throw "the menu did not close ($what)" }
}
function Start-App {
    $script:app = Start-Topsy -Exe $Exe -Env $topsyEnv -Log $appLog
    if (-not (Wait-Log $appLog 'restored' 8000)) { throw 'the app never started' }
    Start-Sleep -Milliseconds 800
}

$topsyEnv = @{ TOPSY_LOG = '1'; TOPSY_STATE = $state; TOPSY_IDLE = 'peek'; TOPSY_NO_MEDIA = '1'; TOPSY_NO_WIDGETS = '1' }
if ([Native]::ShareSheet()) { throw 'a share sheet is already on screen; close it first' }
$failed = $null; $app = $null
$log = ''
try {
    # 1. A saved basket comes back; the file that went away while the app was off is dropped.
    Start-App
    if (-not (Wait-Log $appLog 'restored 0 \(basket 3\)' 100)) { throw 'the saved basket was not read back' }
    if (-not (Wait-Log $appLog 'basket pruned, 2 left' 3000)) { throw 'the missing file was not dropped from the basket' }

    # 2. "Show basket" in the notch's menu (the tray icon's menu) brings it up, without the focus.
    $r = Rect-Of $true
    Use-Menu { Click (($r.Left + $r.Right) / 2) (($r.Top + $r.Bottom) / 2) -Right } 0x4B 'show-basket'   # K
    if (-not (Wait-Log $appLog 'basket at' 2000)) { throw '"Show basket" did not bring the basket up' }
    if (-not (Wait-Until { Basket-Open } 2000)) { throw 'the basket did not open' }
    Start-Sleep -Milliseconds 500
    if (Foreground-Ours) { throw 'showing the basket took the focus' }
    Shoot 'basket-1-restored.png'

    # 3. The pill opens the list, a tile per file.
    $b = Rect-Of $false
    Click (($b.Left + $b.Right) / 2) ($b.Top + (228 - 27) * $scale)
    if (-not (Wait-Until { $r2 = Rect-Of $false; ($r2.Right - $r2.Left) -ge 290 * $scale } 2000)) { throw 'the pill did not open the list' }
    Start-Sleep -Milliseconds 500
    Shoot 'basket-2-list.png'

    # 4. Rename the first tile: the field takes the focus with the stem selected; typing replaces
    #    it and Enter renames the file, then hands the focus back.
    $b = Rect-Of $false
    $tile = { Click ($b.Left + (10 + 32) * $scale) ($b.Top + (52 + 39) * $scale) -Right }
    Use-Menu $tile 0x4E 'rename'   # N
    if (-not (Wait-Log $appLog 'basket rename IMG_0042.txt' 2000)) { throw 'Rename did not put the field up' }
    if (-not (Wait-Until { Foreground-Ours } 2000)) { throw 'the rename field did not get the keyboard' }
    foreach ($vk in 0x42, 0x45, 0x41, 0x43, 0x48) { Tap-Key $vk }   # b e a c h
    Start-Sleep -Milliseconds 300
    Shoot 'basket-3-renaming.png'
    Tap-Key 0x0D
    if (-not (Wait-Log $appLog 'basket renamed to beach.txt' 3000)) { throw 'Enter did not rename the file' }
    if (-not (Test-Path $renamed) -or (Test-Path $first)) { throw 'the file was not renamed on disk' }
    if (-not (Wait-Until { -not (Foreground-Ours) } 2000)) { throw 'the focus was not handed back after the rename' }
    Start-Sleep -Milliseconds 400
    Shoot 'basket-4-renamed.png'
    if (-not (Wait-Until { (Get-Content $basketState -Raw) -match [regex]::Escape($renamed) } 2000)) { throw 'the new name was not saved' }

    # 5. Share opens Windows' share sheet for the file.
    Use-Menu $tile 0x53 'share'   # S
    if (-not (Wait-Log $appLog 'share beach.txt' 2000)) { throw 'Share did not run' }
    if (-not (Wait-Until { [Native]::ShareSheet() } 5000)) { throw 'the share sheet did not come up' }
    if (-not (Wait-Log $appLog 'share filled' 5000)) { throw 'the share sheet never asked for the file' }
    Start-Sleep -Milliseconds 1500
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'basket-5-share.png') -Rect $screen.X, $screen.Y, $screen.Width, $screen.Height
    if (Select-String -Path $appLog -Pattern 'could not (open the share sheet|share)' -Quiet) { throw 'the share sheet failed' }
    # Close it as its own close button would; the sheet outlives topsy otherwise.
    [void][Native]::PostMessage([Native]::Sheet(), 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
    if (-not (Wait-Until { -not [Native]::ShareSheet() } 2000)) { throw 'the share sheet would not close' }

    # 6. A restart brings the renamed file back.
    Stop-Process -Id $app.Id -Force
    $log += Get-Content $appLog -Raw
    Start-App
    if (-not (Wait-Log $appLog 'restored 0 \(basket 2\)' 100)) { throw 'the basket did not come back after a restart' }
    if (Wait-Log $appLog 'basket pruned' 1500) { throw 'a restored file was wrongly pruned' }
} catch {
    $failed = $_
    [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
    if ([Native]::ShareSheet()) { [void][Native]::PostMessage([Native]::Sheet(), 0x10, [IntPtr]::Zero, [IntPtr]::Zero) }
} finally {
    Start-Sleep -Milliseconds 200
    if ($app) { Stop-Topsy $app }
}
"--- app log"; $log; if (Test-Path $appLog) { Get-Content $appLog }
if ($failed) { "FAIL: $($failed.Exception.Message) (line $($failed.InvocationInfo.ScriptLineNumber))"; exit 1 }
"PASS"
