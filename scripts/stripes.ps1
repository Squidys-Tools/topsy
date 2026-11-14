# A topmost window of 6px black/white stripes, the backdrop check-glass.ps1 measures the blur against.
# usage: powershell -STA -File stripes.ps1 <centreX> <top>
param(
    [int]$CenterX = 960,
    [int]$Top = 0
)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System.Runtime.InteropServices;
public static class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@
[void][Dpi]::SetProcessDPIAware()

$f = New-Object System.Windows.Forms.Form
$f.Text = 'topsy-glass-backdrop'
$f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false
$f.TopMost = $true
$f.Location = New-Object System.Drawing.Point(($CenterX - 400), $Top)
$f.Size = New-Object System.Drawing.Size(800, 220)
$f.BackColor = [System.Drawing.Color]::Black
$f.Add_Paint({
    param($s, $e)
    for ($x = 0; $x -lt 800; $x += 12) { $e.Graphics.FillRectangle([System.Drawing.Brushes]::White, $x, 0, 6, 220) }
})
# Safety net: never leave a stripe window on someone's screen.
$t = New-Object System.Windows.Forms.Timer; $t.Interval = 90000; $t.Add_Tick({ $f.Close() }); $t.Start()
[void]$f.ShowDialog()
