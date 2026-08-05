# launch.ps1 — Bank Statement Fidelity Editor launcher
# Double-click or run from any terminal to start the GUI.
# All environment variables (API keys etc.) are loaded from .env automatically.

$ErrorActionPreference = "Continue"
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser -Force -ErrorAction SilentlyContinue

# ─── Paths ───────────────────────────────────────────────────────────────────
$ProjectRoot = $PSScriptRoot
$PythonExe = (Get-Command python.exe -ErrorAction SilentlyContinue).Source
if (-not $PythonExe) {
    Write-Host "[launch] ERROR: python.exe not found in PATH" -ForegroundColor Red
    exit 1
}
$Python      = Split-Path -Parent $PythonExe
$PyMuPdfDir  = "$Python\Lib\site-packages\pymupdf"
$MinGW       = "C:\msys64\mingw64\bin"
$Cargo       = "$env:USERPROFILE\.cargo\bin"
$NodeJs      = "C:\Program Files\nodejs"

# ─── PATH ────────────────────────────────────────────────────────────────────
$env:PATH = "$Python;$PyMuPdfDir;$MinGW;$Cargo;$NodeJs;$env:PATH"

# ─── Python interpreter for PyO3 ─────────────────────────────────────────────
$env:PYO3_PYTHON            = "$Python\python.exe"
$env:PYTHON_SYS_EXECUTABLE  = "$Python\python.exe"
$env:PYTHONHOME             = $Python
$env:PYTHONPATH             = "$Python\Lib\site-packages;$Python\Lib;$Python\DLLs"

# ─── Load .env ───────────────────────────────────────────────────────────────
$envFile = "$ProjectRoot\.env"
if (Test-Path $envFile) {
    Get-Content $envFile | Where-Object { $_ -match "^\s*[^#\s]" -and $_ -match "=" } | ForEach-Object {
        $parts = $_ -split "=", 2
        $key   = $parts[0].Trim()
        $val   = $parts[1].Trim()
        # Skip empty values and obvious placeholders
        if ($val -ne "" -and $val -notmatch "^(PLACEHOLDER|your_|/path/to)") {
            [System.Environment]::SetEnvironmentVariable($key, $val, "Process")
        }
    }
    Write-Host "[launch] Loaded .env" -ForegroundColor DarkGray
} else {
    Write-Host "[launch] WARNING: .env not found — using dev passphrase fallback" -ForegroundColor Yellow
    $env:DUAL_CORE_PASSPHRASE = "dev-passphrase-for-testing-only-2026"
}

# ─── Silence OTLP if no endpoint configured ──────────────────────────────────
if (-not $env:OTEL_EXPORTER_OTLP_ENDPOINT -or $env:OTEL_EXPORTER_OTLP_ENDPOINT -eq "") {
    $env:OTEL_EXPORTER_OTLP_ENDPOINT = ""
}

# ─── Find binary (release preferred, fall back to debug) ─────────────────────
$release = "$ProjectRoot\target\x86_64-pc-windows-gnu\release\dual-core-pdf-pipeline.exe"
$debug   = "$ProjectRoot\target\x86_64-pc-windows-gnu\debug\dual-core-pdf-pipeline.exe"
if (Test-Path $release) { $binary = $release; $mode = "release" }
elseif (Test-Path $debug) { $binary = $debug; $mode = "debug" }
else {
    Write-Host "[launch] ERROR: No binary found. Run 'cargo build --features dev' first." -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}

Write-Host "[launch] Starting Bank Statement Fidelity Editor ($mode build)..." -ForegroundColor Cyan
Write-Host "[launch] Binary: $binary" -ForegroundColor DarkGray
Write-Host ""

# ─── Launch GUI ──────────────────────────────────────────────────────────────
& $binary gui
