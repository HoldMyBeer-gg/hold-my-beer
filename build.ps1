# Build and install collab on Windows

Write-Host "Building and installing collab..." -ForegroundColor Cyan

cargo install --path collab-cli
if ($LASTEXITCODE -ne 0) { exit 1 }

cargo install --path collab-server
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host ""
Write-Host "Done. 'collab' and 'collab-server' are now on your PATH." -ForegroundColor Green
Write-Host ""
Write-Host "Configure: create $env:USERPROFILE\.collab.toml" -ForegroundColor Cyan
Write-Host "  host = `"http://your-server:8000`""
Write-Host "  instance = `"your-worker-name`""
Write-Host "  recipients = [`"other-worker`"]"
Write-Host ""
Write-Host "Run 'collab config-path' to confirm the config file location."
