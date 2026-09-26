<#
.SYNOPSIS
    Provisions the dedicated BankFidelity Specialist Virtual Environment (50GB+ dependency stack).
.DESCRIPTION
    Creates C:\bankfidelity\specialist_env, installs pinned PyTorch + DirectML / Vulkan,
    Surya layout engine, Reducto SDK, PyMuPDF Pro, FontTools, and verification dependencies.
#>

param(
    [string]$TargetEnvDir = "C:\bankfidelity\specialist_env",
    [string]$BasePython = "python",
    [switch]$SkipDownload
)

$ErrorActionPreference = "Stop"

Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host " BankFidelity 50GB+ SOTA Specialist Environment Provisioner     " -ForegroundColor Cyan
Write-Host " Zero Gemini | Zero DocAI | Zero Anthropic | Zero OpenAI/Qwen   " -ForegroundColor Cyan
Write-Host "=================================================================" -ForegroundColor Cyan

# 1. Verify Base Python Version
try {
    $pyVer = & $BasePython --version 2>&1
    Write-Host "[OK] Detected Base Python: $pyVer" -ForegroundColor Green
} catch {
    Write-Error "Base Python not found. Please ensure Python 3.11 or compatible is on PATH."
}

# 2. Create Destination Directory if missing
if (-not (Test-Path $TargetEnvDir)) {
    Write-Host "[PROVISION] Creating virtual environment at $TargetEnvDir ..." -ForegroundColor Yellow
    & $BasePython -m venv $TargetEnvDir
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to create virtual environment at $TargetEnvDir"
    }
} else {
    Write-Host "[EXISTS] Specialist environment directory found at $TargetEnvDir" -ForegroundColor Yellow
}

$EnvPython = Join-Path $TargetEnvDir "Scripts\python.exe"
$EnvPip    = Join-Path $TargetEnvDir "Scripts\pip.exe"

if (-not (Test-Path $EnvPython)) {
    Write-Error "Python executable not found at $EnvPython"
}

# 3. Upgrade pip and core packaging tools
Write-Host "[PIP] Upgrading pip, setuptools, and wheel ..." -ForegroundColor Yellow
& $EnvPython -m pip install --upgrade pip setuptools wheel --quiet

# 4. Install Specialist Requirements
$ReqFile = Join-Path $PSScriptRoot "..\python\requirements_specialist.txt"
if (Test-Path $ReqFile) {
    Write-Host "[INSTALL] Installing specialist requirements from $ReqFile ..." -ForegroundColor Yellow
    & $EnvPip install -r $ReqFile --quiet
} else {
    Write-Error "Requirements file not found at $ReqFile"
}

# 5. Pre-create local model directories
$ModelsDir = "C:\bankfidelity\models"
$SubDirs = @(
    "florence2",
    "surya",
    "phi35_vision",
    "deepfont",
    "fonts"
)

foreach ($sub in $SubDirs) {
    $path = Join-Path $ModelsDir $sub
    if (-not (Test-Path $path)) {
        New-Item -ItemType Directory -Path $path -Force | Out-Null
        Write-Host "[DIR] Initialized model cache: $path" -ForegroundColor DarkCyan
    }
}

# 6. Run Model Downloader if requested
if (-not $SkipDownload) {
    $Downloader = Join-Path $PSScriptRoot "download_specialist_models.py"
    if (Test-Path $Downloader) {
        Write-Host "[MODELS] Initiating pre-staging of specialist model weights ..." -ForegroundColor Yellow
        & $EnvPython $Downloader
    }
}

Write-Host "=================================================================" -ForegroundColor Green
Write-Host " [SUCCESS] Specialist environment ready at $TargetEnvDir" -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Green
