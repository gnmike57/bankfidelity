# build-dev.ps1 — Quick dev build script for Bank Statement Fidelity Editor
# Run this from the project root after all dependencies are installed.

$ErrorActionPreference = "Stop"
$PythonExe = (Get-Command python.exe -ErrorAction SilentlyContinue).Source
if ($PythonExe) {
    $Python = Split-Path -Parent $PythonExe
    $env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\nodejs;$Python;$Python\Scripts;$env:PATH"
} else {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\nodejs;$env:PATH"
}

Write-Host "=== Bank Statement Fidelity Editor — Dev Build ===" -ForegroundColor Cyan

# Load .env if it exists
if (Test-Path ".env") {
    Get-Content ".env" | Where-Object { $_ -match "^\s*[^#]" -and $_ -match "=" } | ForEach-Object {
        $parts = $_ -split "=", 2
        if ($parts[1] -ne "" -and $parts[1] -notmatch "PLACEHOLDER") {
            [System.Environment]::SetEnvironmentVariable($parts[0].Trim(), $parts[1].Trim(), "Process")
        }
    }
    Write-Host "Loaded .env" -ForegroundColor Green
}

Write-Host ""
Write-Host "Building with --features dev (relaxed passphrase check)..." -ForegroundColor Yellow
cargo build --features dev 2>&1

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "Build SUCCESS!" -ForegroundColor Green
    Write-Host "Run the GUI with: .\target\debug\dual-core-pdf-pipeline.exe gui" -ForegroundColor Cyan
    Write-Host "Run health check: .\target\debug\dual-core-pdf-pipeline.exe doctor" -ForegroundColor Cyan
} else {
    Write-Host ""
    Write-Host "Build FAILED — check errors above." -ForegroundColor Red
    exit 1
}
