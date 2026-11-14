# A borderless window covering the primary display, standing in for a fullscreen video or game.
# check-overlay.ps1 puts it in front and expects the notch to stay hidden.
# usage: powershell -STA -File fullscreen.ps1
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System.Runtime.InteropServices;
public static class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@
[void][Dpi]::SetProcessDPIAware()

$f = New-Object System.Windows.Forms.Form
$f.Text = 'topsy-fullscreen'
$f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'
$f.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$f.BackColor = [System.Drawing.Color]::FromArgb(20, 20, 60)
$f.Add_Shown({ $f.Activate() })
# Safety net: never leave a fullscreen window on someone's screen.
$t = New-Object System.Windows.Forms.Timer; $t.Interval = 30000; $t.Add_Tick({ $f.Close() }); $t.Start()
[void]$f.ShowDialog()
