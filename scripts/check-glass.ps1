# Does the pill really blur what is behind it?
#
# Puts a hard black/white stripe pattern (6px stripes) behind the idle pill, then compares
# the stripe amplitude seen *through* the pill with the amplitude of the same stripes
# beside it. A blur flattens fine stripes; a tint or a transparent pill does not.
# Runs the pill three ways against the same backdrop:
#
#   TOPSY_GLASS=clear  control: transparent pill, no blur   -> ratio ~1, proves the measurement
#   TOPSY_GLASS=tint   control: the pill's fill, no blur    -> ratio set by the fill's opacity alone
#   (default) blur     the real thing                       -> must beat the tint control
#
# The tint control is what makes the verdict about the blur and not about the dark fill, which
# would cut stripe contrast by itself.
#
# Shows a stripe window at the top of the screen for about 30 seconds.
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
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
[void][G]::SetProcessDPIAware()
$scale = [G]::GetDpiForSystem() / 96.0
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = [int]($screen.X + $screen.Width / 2)
$shots = Join-Path (Split-Path $PSScriptRoot) 'shots'
New-Item -ItemType Directory $shots -Force | Out-Null

# The stripe backdrop, in a runspace of its own so this script can keep measuring.
$bd = [powershell]::Create()
[void]$bd.AddScript({
    param($cx, $top)
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $f = New-Object System.Windows.Forms.Form
    $f.Text = 'topsy-glass-backdrop'
    $f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false
    # Safety net: never leave a stripe window on someone's screen.
    $t = New-Object System.Windows.Forms.Timer; $t.Interval = 60000; $t.Add_Tick({ $f.Close() }); $t.Start()
    $f.Location = New-Object System.Drawing.Point(($cx - 400), $top)
    $f.Size = New-Object System.Drawing.Size(800, 220)
    $f.BackColor = [System.Drawing.Color]::Black
    $f.Add_Paint({
        param($s, $e)
        for ($x = 0; $x -lt 800; $x += 12) { $e.Graphics.FillRectangle([System.Drawing.Brushes]::White, $x, 0, 6, 220) }
    })
    [void]$f.ShowDialog()
}).AddArgument($cx).AddArgument($screen.Y)
[void]$bd.BeginInvoke()

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
    $env:TOPSY_STATE = Join-Path $env:TEMP 'topsy-glass-state.txt'
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

        $rowTop = [int]((6 + 12) * $scale); $rows = [int](10 * $scale); $cols = [int](45 * $scale)
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
        # Inside the idle pill, left of the "topsy" label.
        $inside = Amplitude ([int]($cx - 70 * $scale)) $rowTop $cols $rows

        # Screenshot for eyeballing.
        $shot = New-Object System.Drawing.Bitmap 520, 80
        $sg = [System.Drawing.Graphics]::FromImage($shot)
        $sg.CopyFromScreen($cx - 260, $screen.Y, 0, 0, $shot.Size); $sg.Dispose()
        $shot.Save((Join-Path $shots "glass-$mode.png")); $shot.Dispose()

        $ratio = $inside / $beside
        Write-Host ("mode={0,-5}  stripe amplitude inside pill={1,3:N0}  beside pill={2,3:N0}  ratio={3:N2}" -f $mode, $inside, $beside, $ratio)
        $ratio
    } finally {
        Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
        $app.WaitForExit(3000) | Out-Null
        Remove-Item Env:TOPSY_GLASS, Env:\TOPSY_STATE -ErrorAction SilentlyContinue
    }
}

$code = 0
try {
    Start-Sleep -Milliseconds 1200
    $clear = Measure-Mode 'clear' | Select-Object -Last 1
    $tint = Measure-Mode 'tint' | Select-Object -Last 1
    $blur = Measure-Mode 'blur' | Select-Object -Last 1
    "tint-only ratio {0:N2}; blurred ratio {1:N2} = {2:N0}% of it (limit {3:P0})" -f $tint, $blur, (100 * $blur / $tint), $MaxBlurShare

    if ($clear -lt 0.8) {
        "FAIL: control (clear) lost stripe contrast ($clear); the measurement is not trustworthy"; $code = 1
    } elseif ($blur -gt $tint * $MaxBlurShare) {
        "FAIL: the blurred pill keeps too much of the stripe contrast the tint alone leaves: no (strong enough) blur"; $code = 1
    } else {
        "PASS (blurred)"
    }
} catch {
    "FAIL: $_"; $code = 1
} finally {
    $h = [G]::FindWindow($null, 'topsy-glass-backdrop')
    if ($h -ne [IntPtr]::Zero) { [void][G]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) }   # WM_CLOSE
    Start-Sleep -Milliseconds 300
}
# No $bd.Dispose(): it blocks until the backdrop runspace's pipeline ends. Exiting the process cleans up.
[Environment]::Exit($code)
