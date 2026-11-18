# Does the notch really blur what is behind it?
#
# Puts a hard black/white stripe pattern (6px stripes) behind the open tray, then compares
# the stripe amplitude seen *through* its glass with the amplitude of the same stripes
# beside it. A blur flattens fine stripes; a tint or a transparent notch does not.
# Runs the tray four ways against the same backdrop:
#
#   TOPSY_GLASS=clear    control: transparent, no blur   -> ratio ~1, proves the measurement
#   TOPSY_GLASS=tint     control: the fill, no blur      -> ratio set by the fill's opacity alone
#   TOPSY_GLASS=blur     the live compositor glass       -> must beat the tint control
#   TOPSY_GLASS=host     the host backdrop, its fallback -> must beat the tint control too
#   TOPSY_GLASS=acrylic  the fallback                    -> must beat the tint control too
#
# The tint control is what makes the verdict about the blur and not about the dark fill, which
# would cut stripe contrast by itself.
#
# Shows a stripe window at the top of the screen for about 50 seconds.
# usage: check-glass.ps1 [-Exe target\debug\topsy.exe] [-MaxBlurShare 0.5]
param(
    [string]$Exe = "$PSScriptRoot\..\target\debug\topsy.exe",
    # The blurred ratio must be at most this fraction of the tint-only ratio.
    [double]$MaxBlurShare = 0.5
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class G {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
"@
[void][G]::SetProcessDPIAware()
$scale = [G]::GetDpiForSystem() / 96.0
$cx = [int]([G]::GetSystemMetrics(0) / 2)   # SM_CXSCREEN; the primary display starts at (0,0)
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

# The stripe backdrop, a separate process (like the other fixtures) so it pumps its own messages.
$bd = Start-Process powershell -ArgumentList @('-NoProfile', '-STA', '-File', "$PSScriptRoot\stripes.ps1", $cx, 0) -PassThru

function Amplitude($left, $top, $width, $height) {
    $bmp = New-Object System.Drawing.Bitmap $width, $height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($left, $top, 0, 0, $bmp.Size); $g.Dispose()
    $min = 255.0; $max = 0.0
    for ($y = 0; $y -lt $height; $y++) {
        for ($x = 0; $x -lt $width; $x++) {
            $c = $bmp.GetPixel($x, $y)
            $l = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            if ($l -lt $min) { $min = $l }; if ($l -gt $max) { $max = $l }
        }
    }
    $bmp.Dispose()
    $max - $min
}

# Run the pill in one glass mode and return inside/beside stripe amplitude.
function Measure-Mode($mode) {
    $env:TOPSY_GLASS = $mode
    # The notch is solid black where it meets the screen edge; the glass is the open tray's
    # lower part, so keep the tray open.
    $env:TOPSY_IDLE = 'tray'
    $env:TOPSY_STATE = Join-Path $env:TEMP 'topsy-glass-state.txt'
    # A playing song puts its row where the glass is measured.
    $env:TOPSY_NO_MEDIA = '1'
    $app = Start-Process $Exe -PassThru
    try {
        # Wait for the pill: its window answers hit-tests at the pill's centre.
        $sw = [Diagnostics.Stopwatch]::StartNew()
        do {
            $pt = New-Object G+POINT; $pt.X = $cx; $pt.Y = [int]((6 + 17) * $scale)
            $owner = 0; [void][G]::GetWindowThreadProcessId([G]::WindowFromPoint($pt), [ref]$owner)
            if ($owner -eq $app.Id) { break }
            Start-Sleep -Milliseconds 100
        } while ($sw.ElapsedMilliseconds -lt 20000)
        if ($owner -ne $app.Id) { throw "the pill never appeared (mode $mode)" }
        Start-Sleep -Milliseconds 1200   # first frames, blur settle

        $rowTop = [int](100 * $scale); $rows = [int](10 * $scale); $cols = [int](40 * $scale)
        # Beside the pill, on the same stripes: the reference. It sits well outside the pill's
        # whole window (220px either side of centre), so nothing the app does can touch it.
        # Wait until the backdrop has actually painted; without it there is nothing to compare,
        # and a ratio of two flat areas means nothing.
        $beside = 0
        $sw.Restart()
        while ($beside -lt 200 -and $sw.ElapsedMilliseconds -lt 15000) {
            $beside = Amplitude ([int]($cx + 300 * $scale)) $rowTop $cols $rows
            if ($beside -lt 200) { Start-Sleep -Milliseconds 250 }
        }
        if ($beside -lt 200) { throw "the stripe backdrop never became visible (amplitude $beside), so there is nothing to measure" }
        # Inside the tray's glass: its lowest part, where the black has faded the most.
        $inside = Amplitude ([int]($cx - 185 * $scale)) $rowTop $cols $rows

        # Screenshot for eyeballing.
        $shot = New-Object System.Drawing.Bitmap 520, 140
        $sg = [System.Drawing.Graphics]::FromImage($shot)
        $sg.CopyFromScreen($cx - 260, 0, 0, 0, $shot.Size); $sg.Dispose()
        $shot.Save((Join-Path $shots "glass-$mode.png")); $shot.Dispose()

        $ratio = $inside / $beside
        Write-Host ("mode={0,-7}  stripe amplitude inside pill={1,3:N0}  beside pill={2,3:N0}  ratio={3:N2}" -f $mode, $inside, $beside, $ratio)
        $ratio
    } finally {
        Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
        $app.WaitForExit(3000) | Out-Null
        Remove-Item Env:TOPSY_GLASS, Env:\TOPSY_IDLE, Env:\TOPSY_STATE, Env:\TOPSY_NO_MEDIA -ErrorAction SilentlyContinue
    }
}

$code = 0
try {
    # The stripes must be up before the island: both are topmost, and the newer one covers.
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do {
        $pt = New-Object G+POINT; $pt.X = $cx + [int](300 * $scale); $pt.Y = [int](50 * $scale)
        $owner = 0; [void][G]::GetWindowThreadProcessId([G]::WindowFromPoint($pt), [ref]$owner)
        if ($owner -eq $bd.Id) { break }
        Start-Sleep -Milliseconds 100
    } while ($sw.ElapsedMilliseconds -lt 15000)
    if ($owner -ne $bd.Id) { throw 'the stripe backdrop never came up' }
    Start-Sleep -Milliseconds 300
    $clear = Measure-Mode 'clear' | Select-Object -Last 1
    $tint = Measure-Mode 'tint' | Select-Object -Last 1
    $blur = Measure-Mode 'blur' | Select-Object -Last 1
    $hostBlur = Measure-Mode 'host' | Select-Object -Last 1
    $acrylic = Measure-Mode 'acrylic' | Select-Object -Last 1
    Write-Host ("tint-only ratio {0:N2}; blurred ratio {1:N2} = {2:N0}% of it, host backdrop {3:N2}, acrylic fallback {4:N2} (limit {5:P0})" -f $tint, $blur, (100 * $blur / $tint), $hostBlur, $acrylic, $MaxBlurShare)

    if ($clear -lt 0.8) {
        Write-Host "FAIL: control (clear) lost stripe contrast ($clear); the measurement is not trustworthy"; $code = 1
    } elseif ($blur -gt $tint * $MaxBlurShare -or $hostBlur -gt $tint * $MaxBlurShare -or $acrylic -gt $tint * $MaxBlurShare) {
        Write-Host "FAIL: a blurred pill keeps too much of the stripe contrast the tint alone leaves: no (strong enough) blur"; $code = 1
    } else {
        Write-Host "PASS (blurred)"
    }
} catch {
    Write-Host "FAIL: $_"; $code = 1
} finally {
    Stop-Process -Id $bd.Id -Force -ErrorAction SilentlyContinue
}
# Environment.Exit drops output still buffered for a pipe, hence Write-Host for the verdict.
[Environment]::Exit($code)
