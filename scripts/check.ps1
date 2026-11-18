# The same gates as CI (.github/workflows/build.yml), for running before a push.
# Stops at the first failure.

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)

foreach ($step in @(
        @('fmt', '--check'),
        @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'),
        @('test', '--workspace', '--locked')
    )) {
    Write-Host "> cargo $($step -join ' ')"
    & cargo @step
    if ($LASTEXITCODE -ne 0) { throw "cargo $($step[0]) failed ($LASTEXITCODE)" }
}
Write-Host '> check-structure'
& "$PSScriptRoot\check-structure.ps1"
if ($LASTEXITCODE -ne 0) { throw 'check-structure failed' }
Write-Host 'All checks passed.'
