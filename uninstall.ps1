# Remove the `apc` binary installed by install.ps1. PowerShell 5.1+ (Windows 10 and later).
#
#   irm https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/uninstall.ps1 | iex
#   # not piped, so -Purge can be passed (also deletes config, sessions, models):
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/uninstall.ps1))) -Purge
#
# Nothing else is touched: no PATH edit, no environment variable change.
param(
    [string]$InstallDir = $env:APC_INSTALL_DIR,
    [switch]$Purge
)

$ErrorActionPreference = "Stop"
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "apc" }

# Where apc keeps its state on Windows: %APPDATA%\aplix\aplix\{config,data}
# (directories-rs: config_dir = roaming\project_path\config, data_dir = roaming\project_path\data).
$state = Join-Path $env:APPDATA "aplix\aplix"
$exe = Join-Path $InstallDir "apc.exe"

if (Test-Path $exe) {
    Remove-Item -LiteralPath $exe -Force
    "removed $exe"
} else {
    "no apc.exe at $exe"
}

if ($Purge) {
    if (Test-Path $state) {
        Remove-Item -LiteralPath $state -Recurse -Force
        "removed $state (config, sessions, models, serve token)"
    }
} else {
    "kept $state (config, sessions, models, serve token)"
    "run again with -Purge to delete it too"
}
