# Install or update `apc` on Windows from a GitHub release. PowerShell 5.1+ (Windows 10 and later).
#
#   irm https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/install.ps1 | iex
#   $env:APC_VERSION = "v1.2.0"; irm .../install.ps1 | iex      # pin a release tag
#   $env:APC_INSTALL_DIR = "C:\tools\apc"; irm .../install.ps1 | iex
#
# Downloads apc-Windows-X64.zip (or ARM64), checks it against the published .sha256, and puts
# apc.exe in APC_INSTALL_DIR (default %LOCALAPPDATA%\apc). Running it again is the update path.
param(
    [string]$Version = $env:APC_VERSION,
    [string]$InstallDir = $env:APC_INSTALL_DIR
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

if (-not $Version) { $Version = "latest" }
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "apc" }

$repo = "STIFLEUR390/aplix-code"
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "X64" }
    "ARM64" { "ARM64" }
    default { throw "apc: unsupported architecture '$($env:PROCESSOR_ARCHITECTURE)'" }
}

# Asset names come from .github/workflows/release.yml: apc-<runner.os>-<runner.arch>.zip, plus a
# <name>.sha256 named after the package, not the archive.
$stem = "apc-Windows-$arch"
$asset = "$stem.zip"
$base = if ($Version -eq "latest") {
    "https://github.com/$repo/releases/latest/download"
} else {
    "https://github.com/$repo/releases/download/$Version"
}

$exe = Join-Path $InstallDir "apc.exe"
if (Test-Path $exe) {
    "updating $((& $exe --version 2>$null) | Select-Object -First 1)"
} else {
    "installing apc"
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("apc-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

try {
    "downloading $asset ($Version)"
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile (Join-Path $tmp $asset)
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$stem.sha256" -OutFile (Join-Path $tmp "$stem.sha256")

    $line = Get-Content (Join-Path $tmp "$stem.sha256") |
        Where-Object { $_ -match [regex]::Escape($asset) } |
        Select-Object -First 1
    if (-not $line) { throw "apc: $asset is missing from $stem.sha256" }
    $expected = ($line -split '\s+')[0]
    # -ne on strings ignores case, which is what makes this compare with sha256sum/shasum output.
    $actual = (Get-FileHash -Path (Join-Path $tmp $asset) -Algorithm SHA256).Hash
    if ($actual -ne $expected) { throw "apc: checksum mismatch for $asset - refusing to install" }

    Expand-Archive -LiteralPath (Join-Path $tmp $asset) -DestinationPath $tmp -Force
    $found = Get-ChildItem -Path $tmp -Filter "apc.exe" -Recurse | Select-Object -First 1
    if (-not $found) { throw "apc: $asset contains no apc.exe" }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -LiteralPath $found.FullName -Destination $exe -Force
} finally {
    Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

""
& $exe --version
if (($env:PATH -split ';') -notcontains $InstallDir) {
    ""
    "$InstallDir is not in your PATH. Add it once, then open a new terminal:"
    "  [Environment]::SetEnvironmentVariable('Path', `"$InstallDir;`" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
}
""
"next: apc init && apc config"
