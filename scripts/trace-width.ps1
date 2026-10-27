# Sample a 1px-high strip through the pill and log its pure-black run width over time.
# Gives a time series of the spring animation as actually composited on screen.
# Needs the pill to be pure black, so launch the app with TOPSY_GLASS=solid (and TOPSY_DEMO=1
# for something to animate); the default glass pill is not black.
# usage: trace-width.ps1 <out.csv> [durationMs] [rowY]
param(
    [Parameter(Mandatory)][string]$Out,
    [int]$DurationMs = 8000,
    [int]$RowY = 23
)
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$w = 600
$x = $screen.X + [int](($screen.Width - $w) / 2)
$y = $screen.Y + $RowY
$bmp = New-Object System.Drawing.Bitmap $w, 1
$g = [System.Drawing.Graphics]::FromImage($bmp)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rows = New-Object System.Collections.Generic.List[string]
$rows.Add("ms,width")
while ($sw.ElapsedMilliseconds -lt $DurationMs) {
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $mid = [int]($w / 2)
    $c = $bmp.GetPixel($mid, 0)
    $run = 0
    if ($c.R -eq 0 -and $c.G -eq 0 -and $c.B -eq 0) {
        $l = $mid; while ($l -gt 0 -and ($p = $bmp.GetPixel($l - 1, 0)) -and $p.R -eq 0 -and $p.G -eq 0 -and $p.B -eq 0) { $l-- }
        $r = $mid; while ($r -lt $w - 1 -and ($p = $bmp.GetPixel($r + 1, 0)) -and $p.R -eq 0 -and $p.G -eq 0 -and $p.B -eq 0) { $r++ }
        $run = $r - $l + 1
    }
    $rows.Add("$($sw.ElapsedMilliseconds),$run")
}
$g.Dispose(); $bmp.Dispose()
$rows | Set-Content $Out
