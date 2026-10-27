# Capture the top-center strip of the primary screen to a PNG.
# usage: shot.ps1 <out.png> [delayMs] [count] [intervalMs]
param(
    [Parameter(Mandatory)][string]$Out,
    [int]$DelayMs = 0,
    [int]$Count = 1,
    [int]$IntervalMs = 0
)
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$w = 520; $h = 170
$x = $screen.X + [int](($screen.Width - $w) / 2)
$y = $screen.Y
if ($DelayMs -gt 0) { Start-Sleep -Milliseconds $DelayMs }
for ($i = 0; $i -lt $Count; $i++) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $g.Dispose()
    $path = if ($Count -gt 1) { [IO.Path]::ChangeExtension($Out, $null).TrimEnd('.') + "-$i.png" } else { $Out }
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    if ($IntervalMs -gt 0) { Start-Sleep -Milliseconds $IntervalMs }
}
