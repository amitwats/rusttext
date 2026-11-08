<#
.SYNOPSIS
    Build RustText for Windows.

.DESCRIPTION
    Runs the tests, builds the release binary and packages it into
    dist\windows\rusttext.exe and dist\rusttext-<version>-windows-<arch>.zip

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
    powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1 -SkipTests -Profile debug
#>
param(
    # Not named $Profile: that clashes with PowerShell's automatic $PROFILE variable.
    [Alias("Profile")]
    [ValidateSet("release", "debug")]
    [string]$BuildProfile = "release",
    [switch]$SkipTests,
    [string]$Target = ""
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo not found - install Rust from https://rustup.rs"
}

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$arch = if ($Target) { $Target } else { $env:PROCESSOR_ARCHITECTURE.ToLower() }

$cargoArgs = @()
if ($BuildProfile -eq "release") { $cargoArgs += "--release" }
if ($Target) { $cargoArgs += @("--target", $Target) }

if (-not $SkipTests) {
    Write-Host "==> Running tests"
    cargo test @cargoArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "==> Building RustText $version ($BuildProfile)"
cargo build @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$outDir = if ($Target) { "target\$Target\$BuildProfile" } else { "target\$BuildProfile" }
New-Item -ItemType Directory -Force -Path dist\windows | Out-Null
Copy-Item "$outDir\rusttext.exe" dist\windows\ -Force
Copy-Item README.md dist\windows\ -Force

$archive = "dist\rusttext-$version-windows-$arch.zip"
if (Test-Path $archive) { Remove-Item $archive }
Compress-Archive -Path dist\windows\* -DestinationPath $archive
Write-Host "==> Done: dist\windows\rusttext.exe and $archive"
