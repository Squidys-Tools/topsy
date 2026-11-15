# Keeps the source tree in the shape AGENTS.md describes (Structure). CI runs it, and so does check.ps1.
#   1. No file under src/ grows past $MaxLines. A file over it today has its own ceiling in
#      $Ceilings, which may only come down: lower it when the file shrinks, never raise it.
#   2. src/model/ stays pure: no Win32, no GPUI, no clock and no environment, so its rules are
#      tested by driving time and input directly.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$MaxLines = 500
$Ceilings = @{
    'src/main.rs' = 950
}

$failures = @()
foreach ($file in Get-ChildItem (Join-Path $root 'src') -Recurse -Filter *.rs) {
    $rel = $file.FullName.Substring($root.Length + 1).Replace('\', '/')
    $lines = @(Get-Content -LiteralPath $file.FullName).Count
    $limit = if ($Ceilings.ContainsKey($rel)) { $Ceilings[$rel] } else { $MaxLines }
    if ($lines -gt $limit) {
        $failures += "$rel has $lines lines (limit $limit): split it by job, see AGENTS.md (Structure)"
    } elseif ($Ceilings.ContainsKey($rel) -and $lines -lt $limit) {
        Write-Host "note: $rel is down to $lines lines; lower its ceiling in scripts/check-structure.ps1"
    }
}
foreach ($rel in $Ceilings.Keys) {
    if (-not (Test-Path (Join-Path $root $rel))) {
        $failures += "$rel is gone: remove its ceiling from scripts/check-structure.ps1"
    }
}

$model = Join-Path $root 'src/model'
if (Test-Path $model) {
    $banned = 'windows::', 'gpui::', 'Instant::now', 'SystemTime::now', 'std::env'
    foreach ($file in Get-ChildItem $model -Recurse -Filter *.rs) {
        $rel = $file.FullName.Substring($root.Length + 1).Replace('\', '/')
        $n = 0
        foreach ($line in Get-Content -LiteralPath $file.FullName) {
            $n++
            # Tests may take a real clock as their starting point.
            if ($line -match '^#\[cfg\(test\)\]') { break }
            foreach ($word in $banned) {
                if ($line.Contains($word)) { $failures += "${rel}:$n is in src/model/, which must stay pure: $($line.Trim())"; break }
            }
        }
    }
}

if ($failures) {
    $failures | ForEach-Object { Write-Host "FAIL: $_" }
    exit 1
}
Write-Host 'Structure OK.'
