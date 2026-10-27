# A tiny window that accepts file drops and logs what it received.
# Test fixture for dragging files out of the island: a real, separate app to drop onto.
# usage: powershell -STA -File drop-target.ps1 <x> <y> <logfile>
param(
    [int]$X = 300,
    [int]$Y = 500,
    [Parameter(Mandatory)][string]$LogFile
)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System.Runtime.InteropServices;
public static class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@
[void][Dpi]::SetProcessDPIAware()

$form = New-Object System.Windows.Forms.Form
$form.Text = 'topsy-drop-target'
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point($X, $Y)
$form.Size = New-Object System.Drawing.Size(320, 200)
$form.TopMost = $true
$label = New-Object System.Windows.Forms.Label
$label.Dock = 'Fill'; $label.Text = 'drop files here'; $label.TextAlign = 'MiddleCenter'
$label.AllowDrop = $true
$form.Controls.Add($label)
$label.Add_DragLeave({ Add-Content $LogFile 'target: drag left' })
$label.Add_DragEnter({
    Add-Content $LogFile 'target: drag enter begin'
    $files = $_.Data.GetDataPresent([System.Windows.Forms.DataFormats]::FileDrop)
    Add-Content $LogFile "target: drag entered (files=$files)"
    if ($files) { $_.Effect = [System.Windows.Forms.DragDropEffects]::Copy }
})
$label.Add_DragDrop({
    $files = $_.Data.GetData([System.Windows.Forms.DataFormats]::FileDrop)
    Add-Content $LogFile ("target: dropped " + ($files -join ';'))
})
$form.Add_Shown({ Add-Content $LogFile 'target: ready' })
[void]$form.ShowDialog()
