# Build and launch the Hold My Beer desktop GUI on Windows.
#
# Usage:  .\start.ps1
#
# What it does:
#   1. Checks that cargo, node, and pnpm are installed.
#   2. Builds the Tauri GUI (which also builds the Rust CLI + server sidecars).
#   3. Launches the built executable.

$ErrorActionPreference = "Stop"

function Have($name) {
    return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

$missing = @()
if (-not (Have "cargo")) { $missing += "cargo (Rust toolchain) -> https://rustup.rs/" }
if (-not (Have "node"))  { $missing += "node (Node.js >= 20) -> https://nodejs.org/" }
if (-not (Have "pnpm"))  { $missing += "pnpm -> https://pnpm.io/installation  (or:  npm install -g pnpm)" }

if ($missing.Count -gt 0) {
    Write-Host ""
    Write-Host "Missing required tools:" -ForegroundColor Red
    foreach ($m in $missing) { Write-Host "  - $m" }
    Write-Host ""
    Write-Host "Install the tools above, open a fresh shell, then run .\start.ps1 again."
    exit 1
}

Write-Host ("cargo:  " + (cargo --version))  -ForegroundColor Green
Write-Host ("node:   " + (node --version))   -ForegroundColor Green
Write-Host ("pnpm:   " + (pnpm --version))   -ForegroundColor Green
Write-Host ""

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location (Join-Path $root "collab-gui")

if (-not (Test-Path "node_modules")) {
    Write-Host "Installing frontend dependencies (first run only)..." -ForegroundColor Yellow
    pnpm install
    if ($LASTEXITCODE -ne 0) { exit 1 }
    Write-Host ""
}

Write-Host "Building the GUI (this takes a few minutes the first time)..." -ForegroundColor Yellow
pnpm run build
if ($LASTEXITCODE -ne 0) { exit 1 }
Write-Host ""

$exe = Join-Path $root "collab-gui\src-tauri\target\release\hold-my-beer-gui.exe"
if (-not (Test-Path $exe)) {
    Write-Host "Build succeeded but the executable wasn't found at:" -ForegroundColor Red
    Write-Host "  $exe"
    exit 1
}

Write-Host "Launching Hold My Beer..." -ForegroundColor Green
Start-Process $exe
