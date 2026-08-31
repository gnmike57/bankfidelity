# run-after-gcc.ps1 — Auto-triggered once MinGW gcc is installed
# Runs cargo check with the GNU target to validate the full codebase compiles.

$ErrorActionPreference = "Continue"

$cargoPath  = "$env:USERPROFILE\.cargo\bin"
$PythonExe = (Get-Command python.exe -ErrorAction SilentlyContinue).Source
if (-not $PythonExe) {
    Write-Host "ERROR: python.exe not found in PATH" -ForegroundColor Red
    exit 1
}
$pythonPath = Split-Path -Parent $PythonExe
$mingwPath  = "C:\msys64\mingw64\bin"
$msys2Path  = "C:\msys64\usr\bin"
$nodePath   = "C:\Program Files\nodejs"

$env:PATH = "$cargoPath;$mingwPath;$msys2Path;$pythonPath;$pythonPath\Scripts;$nodePath;$env:PATH"

# Set PyO3 Python interpreter
$env:PYO3_PYTHON = "$pythonPath\python.exe"
$env:PYTHON_SYS_EXECUTABLE = "$pythonPath\python.exe"

# Verify gcc
$gccVersion = & "$mingwPath\gcc.exe" --version 2>&1 | Select-Object -First 1
Write-Host "GCC: $gccVersion" -ForegroundColor Green

Write-Host ""
Write-Host "=== Running cargo check (GNU target) ===" -ForegroundColor Cyan

cargo check --target x86_64-pc-windows-gnu 2>&1

if ($LASTEXITCODE -eq 0) {
    Write-Host "`n[SUCCESS] cargo check passed!" -ForegroundColor Green
} else {
    Write-Host "`n[PARTIAL] cargo check had errors — see above." -ForegroundColor Yellow
}
