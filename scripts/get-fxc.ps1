# Gets fxc.exe, the shader compiler GPUI's build needs, without installing the Windows SDK or needing admin:
# takes it from Microsoft's Windows SDK package on NuGet and points GPUI_FXC_PATH (for this user) at it.
# usage: get-fxc.ps1 [-Force]   (-Force fetches it even when an fxc.exe is already found)
param([switch]$Force)
$ErrorActionPreference = 'Stop'
$version = '10.0.28000.2957'
$dest = Join-Path $env:LOCALAPPDATA 'topsy-build\fxc'
$fxc = Join-Path $dest 'fxc.exe'

if (-not $Force) {
    $found = @($env:GPUI_FXC_PATH, (Get-Command fxc.exe -ErrorAction SilentlyContinue).Source,
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.26100.0\x64\fxc.exe") |
        Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if ($found) { "fxc.exe is already there: $found"; return }
}

$work = Join-Path $env:TEMP "topsy-fxc-$PID"
New-Item -ItemType Directory -Force $work, $dest | Out-Null
try {
    $package = Join-Path $work 'sdk.zip'
    "Downloading the Windows SDK package $version from NuGet (about 160 MB)..."
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest "https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp/$version/microsoft.windows.sdk.cpp.$version.nupkg" -OutFile $package
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($package)
    try {
        # fxc.exe loads the d3dcompiler_47.dll beside it before the system's.
        foreach ($name in 'fxc.exe', 'd3dcompiler_47.dll') {
            $entry = $zip.Entries | Where-Object { $_.FullName -match "^c/bin/[^/]+/x64/$([regex]::Escape($name))$" } | Select-Object -First 1
            if (-not $entry) { throw "$name is not in the package" }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $dest $name), $true)
        }
    } finally { $zip.Dispose() }
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

$signature = Get-AuthenticodeSignature $fxc
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
    Remove-Item -Recurse -Force $dest
    throw "fxc.exe is not signed by Microsoft ($($signature.Status)); removed it"
}

[Environment]::SetEnvironmentVariable('GPUI_FXC_PATH', $fxc, 'User')
$env:GPUI_FXC_PATH = $fxc
"fxc.exe is at $fxc, and GPUI_FXC_PATH points at it for this user. Open a new terminal, then cargo build."
