# End-to-end check of the tray icon, its menu and start-with-Windows on a live desktop:
#   the icon is registered in the notification area -> left click toggles the shelf -> right click
#   opens the menu -> "Start with Windows" writes and then removes the real HKCU Run value ->
#   Show shelf / Clear shelf -> right-clicking the pill opens the same menu -> Quit exits the app
#   and takes the icon out of the notification area.
# Moves the real cursor for ~40s. Briefly edits HKCU\...\Run\topsy (start with Windows) and the shell's
# own "show this icon" flag for topsy under HKCU\Control Panel\NotifyIconSettings; both are restored.
# Works with an auto-hidden taskbar: the cursor is parked on the screen edge to bring it up.
# usage: e2e-tray.ps1 [-Exe target\release\topsy.exe]
param([string]$Exe = "$PSScriptRoot\..\target\release\topsy.exe")
$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path $Exe).Path
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct NOTIFYICONIDENTIFIER { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT r);
  // First window of `pid` with this class (hidden ones included when `visibleOnly` is false).
  public static IntPtr WindowOf(uint pid, string cls, bool visibleOnly) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p); if (p != pid) return true;
      var s = new StringBuilder(256); GetClassName(h, s, 256); if (s.ToString() != cls) return true;
      if (visibleOnly && !IsWindowVisible(h)) return true;
      found = h; return false; }, IntPtr.Zero);
    return found;
  }
  // The island: the process's visible window that is neither the tray's hidden one nor a menu.
  public static IntPtr IslandOf(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p); if (p != pid || !IsWindowVisible(h)) return true;
      var s = new StringBuilder(256); GetClassName(h, s, 256);
      if (s.ToString() == "#32768" || s.ToString() == "topsy-tray") return true;
      found = h; return false; }, IntPtr.Zero);
    return found;
  }
}
"@
[void][Native]::SetProcessDPIAware()
$LEFTDOWN = 0x2; $LEFTUP = 0x4; $RIGHTDOWN = 0x8; $RIGHTUP = 0x10
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

$work = Join-Path $env:TEMP 'topsy-e2e-tray'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
$files = 'tray-a.txt', 'tray-b.txt' | ForEach-Object { $p = Join-Path $work $_; Set-Content $p "topsy $_"; $p }
$appLog = Join-Path $work 'app.log'
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

function Wait-Log($pattern, $ms) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) {
        if ((Test-Path $appLog) -and (Select-String -Path $appLog -Pattern $pattern -Quiet)) { return $true }
        Start-Sleep -Milliseconds 25
    }
    return $false
}
function Log-Count($pattern) { if (Test-Path $appLog) { @(Select-String -Path $appLog -Pattern $pattern).Count } else { 0 } }
function Wait-Until([scriptblock]$cond, $ms) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $ms) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 30 }
    return $false
}
$scale = [Native]::GetDpiForSystem() / 96.0
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
function Move-Cursor($x, $y) {
    $nx = [int]([math]::Round($x * 65535 / ($screen.Width - 1)))
    $ny = [int]([math]::Round($y * 65535 / ($screen.Height - 1)))
    [Native]::mouse_event(0x8001, $nx, $ny, 0, [UIntPtr]::Zero)
}
function Click($x, $y, [switch]$Right) {
    Move-Cursor $x $y; Start-Sleep -Milliseconds 150
    $down, $up = if ($Right) { $RIGHTDOWN, $RIGHTUP } else { $LEFTDOWN, $LEFTUP }
    [Native]::mouse_event($down, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 50
    [Native]::mouse_event($up, 0, 0, 0, [UIntPtr]::Zero)
}
function Tap-Key([byte]$vk) {
    [Native]::keybd_event($vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
    [Native]::keybd_event($vk, 0, 2, [UIntPtr]::Zero)
}
function Island-Rect {
    $h = [Native]::IslandOf($app.Id)
    if ($h -eq [IntPtr]::Zero) { throw 'the island has no visible window' }
    $r = New-Object Native+RECT; [void][Native]::GetWindowRect($h, [ref]$r); $r
}
function Island-Open { $r = Island-Rect; $r.Bottom - $r.Top -ge 100 * $scale }
function Menu-Visible { [Native]::WindowOf($app.Id, '#32768', $true) -ne [IntPtr]::Zero }
function Run-Value { (Get-ItemProperty -Path $RunKey -Name topsy -ErrorAction SilentlyContinue).topsy }

# Where the notification-area icon is, as the shell reports it; throws if it was never added.
function Icon-Rect {
    $tray = [Native]::WindowOf($app.Id, 'topsy-tray', $false)
    if ($tray -eq [IntPtr]::Zero) { throw 'the tray window does not exist' }
    $id = New-Object Native+NOTIFYICONIDENTIFIER
    $id.cbSize = [Runtime.InteropServices.Marshal]::SizeOf([type][Native+NOTIFYICONIDENTIFIER])
    $id.hWnd = $tray; $id.uID = 1
    $r = New-Object Native+RECT
    $hr = [Native]::Shell_NotifyIconGetRect([ref]$id, [ref]$r)
    if ($hr -ne 0) { throw ('the shell does not know the tray icon (HRESULT 0x{0:X8})' -f $hr) }
    $r
}
# An auto-hidden taskbar reports the icon below the screen edge. Park the cursor on the edge
# and wait for the taskbar to stop sliding up, then return the icon's on-screen rectangle.
function Reveal-Taskbar {
    $x = ((Icon-Rect).Left + (Icon-Rect).Right) / 2
    Move-Cursor $x ($screen.Height - 1)
    $last = $null
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 5000) {
        $r = Icon-Rect
        $now = "$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)"
        if ($r.Bottom -le $screen.Height -and $r.Top -ge 0 -and $now -eq $last) { return $r }
        $last = $now; Start-Sleep -Milliseconds 250
    }
    throw "the taskbar never settled with the icon on screen: $last"
}
function Icon-Click([switch]$Right) {
    $r = Reveal-Taskbar
    Click (($r.Left + $r.Right) / 2) (($r.Top + $r.Bottom) / 2) -Right:$Right
}
# How many pixels in the icon's rectangle are the icon's blue (it is the only blue thing in the tray).
function Blue-Pixels($r) {
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size); $g.Dispose()
    $n = 0
    for ($y = 0; $y -lt $h; $y++) { for ($x = 0; $x -lt $w; $x++) {
        $c = $bmp.GetPixel($x, $y)
        if ($c.B -ge 200 -and $c.R -le 170 -and ($c.B - $c.R) -ge 70) { $n++ }
    } }
    $bmp.Dispose(); $n
}
function Shot-Region($r, $path, $pad = 40) {
    $x = [math]::Max(0, $r.Left - $pad); $y = [math]::Max(0, $r.Top - $pad)
    $w = [math]::Min($screen.Width - $x, $r.Right - $r.Left + 2 * $pad); $h = [math]::Min($screen.Height - $y, $r.Bottom - $r.Top + 2 * $pad)
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size); $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
function Shot-Screen($path) {
    $bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($screen.X, $screen.Y, 0, 0, $bmp.Size); $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
# Run `$open` (which right-clicks something), wait for the menu, press `key` (a mnemonic) or Escape.
function Use-Menu([scriptblock]$open, [byte]$key, $what) {
    & $open
    if (-not (Wait-Until { Menu-Visible } 2500)) { throw "the menu did not open ($what)" }
    Start-Sleep -Milliseconds 250
    if ($key -eq 0) { Shot-Screen (Join-Path $shots "tray-menu-$what.png"); Tap-Key 0x1B } else { Tap-Key $key }
    if (-not (Wait-Until { -not (Menu-Visible) } 2000)) { throw "the menu did not close ($what)" }
}

# The shell's per-icon "show in the taskbar" flag; new icons otherwise start in the overflow flyout.
function Find-IconSettings {
    $base = 'HKCU:\Control Panel\NotifyIconSettings'
    foreach ($k in Get-ChildItem $base -ErrorAction SilentlyContinue) {
        if ((Get-ItemProperty $k.PSPath).ExecutablePath -eq $Exe) { return $k.PSPath }
    }
}
$promoteKey = $null; $hadPromoted = $null
$IconMenu = { Icon-Click -Right }

$hadRun = $null -ne (Run-Value)
$savedRun = Run-Value
Remove-ItemProperty -Path $RunKey -Name topsy -ErrorAction SilentlyContinue

$env:TOPSY_LOG = '1'
$env:TOPSY_SEED = $files -join ';'
$app = Start-Process $Exe -PassThru -RedirectStandardError $appLog
[void]$app.Handle   # cache the handle, or ExitCode reads as empty once the process is gone
$failed = $null
try {
    if (-not (Wait-Log 'seeded 2' 8000)) { throw 'the shelf was never seeded' }
    if ($null -ne (Run-Value)) { throw 'starting the app must not turn on start-with-Windows by itself' }

    # 1. The icon is registered with the shell (it can say where it is), promoted out of the overflow
    #    flyout, and drawn there.
    if (-not (Wait-Until { try { [void](Icon-Rect); $true } catch { $false } } 6000)) { Icon-Rect | Out-Null }
    if (-not (Wait-Until { $null -ne (Find-IconSettings) } 6000)) { throw 'the shell never recorded the tray icon in NotifyIconSettings' }
    $promoteKey = Find-IconSettings
    $hadPromoted = (Get-ItemProperty $promoteKey).IsPromoted
    Set-ItemProperty -Path $promoteKey -Name IsPromoted -Value 1 -Type DWord
    Start-Sleep -Milliseconds 1200
    $ir = Reveal-Taskbar
    if ($ir.Right - $ir.Left -lt 8 -or $ir.Bottom -le $ir.Top) { throw 'the tray icon has no size on screen' }
    Start-Sleep -Milliseconds 400
    Shot-Region $ir (Join-Path $shots 'tray-icon.png')
    $blue = Blue-Pixels $ir
    if ($blue -lt 30) { throw "the tray icon is not drawn where the shell put it ($blue blue pixels)" }

    # 2. Left click toggles the shelf: open, then closed again.
    Icon-Click
    if (-not (Wait-Log 'tray Toggle \(mode Idle\)' 3000)) { throw 'left-clicking the icon did nothing' }
    if (-not (Wait-Until { Island-Open } 3000)) { throw 'the shelf did not open from the icon' }
    Start-Sleep -Milliseconds 400
    & "$PSScriptRoot\shot.ps1" -Out (Join-Path $shots 'tray-1-opened.png')
    Icon-Click
    if (-not (Wait-Log 'tray Toggle \(mode Tray\)' 3000)) { throw 'the second click on the icon did nothing' }
    if (-not (Wait-Until { -not (Island-Open) } 3000)) { throw 'the shelf did not close from the icon' }

    # 3. Start with Windows: the menu item writes the real Run value, and clears it again.
    Use-Menu $IconMenu 0x57 'autostart-on'          # W
    if (-not (Wait-Until { $null -ne (Run-Value) } 3000)) { throw 'Start with Windows did not write the Run value' }
    if ((Run-Value) -ne ('"' + $Exe + '"')) { throw "the Run value is not the quoted exe path: $(Run-Value)" }
    Use-Menu $IconMenu 0 'autostart-checked'        # look at it (checked), then Escape
    if ($null -eq (Run-Value)) { throw 'dismissing the menu must not change start-with-Windows' }
    Use-Menu $IconMenu 0x57 'autostart-off'         # W again
    if (-not (Wait-Until { $null -eq (Run-Value) } 3000)) { throw 'Start with Windows did not remove the Run value' }

    # 4. Show shelf opens it (a closed island), Clear shelf empties it.
    Use-Menu $IconMenu 0x53 'show'                  # S
    if (-not (Wait-Log 'tray Show' 3000)) { throw 'Show shelf did nothing' }
    if (-not (Wait-Until { Island-Open } 3000)) { throw 'Show shelf did not open the shelf' }
    Use-Menu $IconMenu 0x43 'clear'                 # C
    if (-not (Wait-Log 'tray ClearShelf' 3000)) { throw 'Clear shelf did nothing' }
    if (-not (Wait-Log 'shelf now 0' 3000)) { throw 'Clear shelf did not empty the shelf' }
    Use-Menu $IconMenu 0 'empty'                    # Clear is greyed out now; look, then Escape
    if ((Log-Count 'tray ClearShelf') -ne 1) { throw 'dismissing the menu must not clear anything' }

    # 5. Right-clicking the pill itself opens the same menu.
    if (Island-Open) { Icon-Click; [void](Wait-Until { -not (Island-Open) } 3000) }   # fold it so the pill is what is on screen
    Move-Cursor ($screen.Width / 2) 300; Start-Sleep -Milliseconds 400
    Use-Menu { $r = Island-Rect; Click (($r.Left + $r.Right) / 2) (($r.Top + $r.Bottom) / 2) -Right } 0 'pill'

    # 6. Quit exits the app and takes the icon out of the notification area.
    [void](Reveal-Taskbar)
    $blue = Blue-Pixels $ir
    if ($blue -lt 30) { throw "the tray icon vanished before Quit ($blue blue pixels)" }
    Use-Menu $IconMenu 0x51 'quit'                  # Q
    if (-not (Wait-Until { $app.HasExited } 5000)) { throw 'Quit did not end the process' }
    if ($app.ExitCode -ne 0) { throw "the app exited with code $($app.ExitCode)" }
    Start-Sleep -Milliseconds 700
    $left = Blue-Pixels $ir
    Shot-Region $ir (Join-Path $shots 'tray-icon-after-quit.png')
    if ($left -gt 3) { throw "the icon is still in the notification area after Quit ($left blue pixels)" }
} catch {
    $failed = $_
    [Native]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [Native]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
} finally {
    Start-Sleep -Milliseconds 200
    if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
    # Leave the shell's icon flag and start-with-Windows exactly as they were found.
    if ($promoteKey) { if ($null -ne $hadPromoted) { Set-ItemProperty -Path $promoteKey -Name IsPromoted -Value $hadPromoted -Type DWord } else { Remove-ItemProperty -Path $promoteKey -Name IsPromoted -ErrorAction SilentlyContinue } }
    Move-Cursor ($screen.Width / 2) ($screen.Height / 2)
    if ($hadRun) { Set-ItemProperty -Path $RunKey -Name topsy -Value $savedRun } else { Remove-ItemProperty -Path $RunKey -Name topsy -ErrorAction SilentlyContinue }
}
"--- app log (render lines omitted)"; if (Test-Path $appLog) { Get-Content $appLog | Where-Object { $_ -notmatch 'topsy: render$' } }
"--- Run value now: " + $(if ($null -eq (Run-Value)) { '(absent)' } else { Run-Value })
if ($failed) { "FAIL: $($failed.Exception.Message)"; exit 1 }
"PASS"
