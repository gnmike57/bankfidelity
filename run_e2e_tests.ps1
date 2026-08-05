<#
.SYNOPSIS
Runs all real-world OS-level End-to-End UI tests.

.DESCRIPTION
This script compiles the Rust binary and then runs:
1. The native Rust UIAutomation suite
2. The Python PyWinAuto (Accessibility) suite
3. The Python PyAutoGUI (Vision) suite

.NOTES
Requires Python (with pytest, pywinauto, pyautogui) to be installed.
#>

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Enabling Max Deep Event Logging (RUST_LOG=trace)" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
$env:RUST_LOG = "trace"

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Building dual-core-pdf-pipeline (Debug)" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
cargo build
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed. Aborting tests." -ForegroundColor Red
    exit 1
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 0. Running All Existing Rust Unit & Integration Tests" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
cargo test --all-targets
if ($LASTEXITCODE -ne 0) {
    Write-Host "Unit tests failed. Aborting." -ForegroundColor Red
    exit 1
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 0.5. Running CLI Pipeline Self-Test" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
cargo run --bin dual-core-pdf-pipeline -- selftest
if ($LASTEXITCODE -ne 0) {
    Write-Host "CLI self-test failed. Aborting." -ForegroundColor Red
    exit 1
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 1. Running Native Rust UIAutomation Test Foundation" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
cargo test --test e2e_rust_uiautomation -- --nocapture

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 2. Running Python PyWinAuto (Accessibility) Foundation" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
# Run the pywinauto test
python -m pytest tests/e2e/test_pywinauto.py -v -s

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 3. Running Python PyAutoGUI (Vision) Foundation" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
# Run the pyautogui test
python -m pytest tests/e2e/test_vision_pyautogui.py -v -s

Write-Host "`n==========================================================" -ForegroundColor Green
Write-Host " All E2E Foundations Executed." -ForegroundColor Green
Write-Host " Note: If Python dependencies are missing, run: pip install -r tests/e2e/requirements.txt" -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Green
