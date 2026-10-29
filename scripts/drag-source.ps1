# A tiny window that starts a real OLE file drag (CF_HDROP) when pressed.
# Test fixture for the island: gives us a genuine drag without touching the user's files.
# With -Text the payload is that string as plain text (a selection drag), not a file.
# usage: powershell -STA -File drag-source.ps1 <file-to-drag> <x> <y> <logfile> [-Text]
param(
    [Parameter(Mandatory)][string]$File,
    [int]$X = 200,
    [int]$Y = 400,
    [Parameter(Mandatory)][string]$LogFile,
    [switch]$Text
)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System.Runtime.InteropServices;
public static class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@
[void][Dpi]::SetProcessDPIAware()

$form = New-Object System.Windows.Forms.Form
$form.Text = 'topsy-drag-source'
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point($X, $Y)
$form.Size = New-Object System.Drawing.Size(320, 200)
$form.TopMost = $true
$label = New-Object System.Windows.Forms.Label
$label.Dock = 'Fill'; $label.Text = 'press and drag'; $label.TextAlign = 'MiddleCenter'
$form.Controls.Add($label)
$label.Add_MouseDown({
    Add-Content $LogFile 'source: dragging'
    $data = New-Object System.Windows.Forms.DataObject
    if ($Text) { $data.SetData([System.Windows.Forms.DataFormats]::UnicodeText, $File) }
    else { $data.SetData([System.Windows.Forms.DataFormats]::FileDrop, [string[]]@($File)) }
    $effect = $label.DoDragDrop($data, [System.Windows.Forms.DragDropEffects]::Copy)
    Add-Content $LogFile "source: drag ended, effect=$effect"
    $form.Close()
})
$form.Add_Shown({ Add-Content $LogFile 'source: ready' })
[void]$form.ShowDialog()
