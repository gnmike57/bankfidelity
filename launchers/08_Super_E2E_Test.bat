@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY UFO // SUPER E2E TEST GAUNTLET
color 0E
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

set "UFO_ROOT=C:\ufo\ufo"
set "BF_DIR=C:\bankfidelity\bankfidelity"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONIOENCODING=utf-8"

cls
echo !ESC![95m================================================================================!ESC![0m
echo !ESC![95m                         UFO SUPER E2E TEST GAUNTLET!ESC![0m
echo !ESC![95m================================================================================!ESC![0m
echo !ESC![97mThis test sequence will validate the entire architecture from bottom to top:!ESC![0m
echo !ESC![90m  1. Static Architecture Audit!ESC![0m
echo !ESC![90m  2. Python Unit & Integration Tests (pytest)!ESC![0m
echo !ESC![90m  3. Rust Backend Tests (cargo test)!ESC![0m
echo !ESC![90m  4. Live UI Control Smoke Test (Notepad)!ESC![0m
echo.
echo !ESC![93mWARNING: DO NOT TOUCH THE MOUSE OR KEYBOARD ONCE PHASE 4 BEGINS.!ESC![0m
echo.
pause

:: -----------------------------------------------------------------------------
:: PHASE 1: STATIC ARCHITECTURE AUDIT
:: -----------------------------------------------------------------------------
echo.
echo !ESC![96m[PHASE 1] RUNNING SEQUENTIAL ARCHITECTURE AUDIT...!ESC![0m
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
if !ERRORLEVEL! NEQ 0 (
    echo !ESC![91m[FATAL ERROR] Phase 1: Architecture Audit FAILED. Aborting E2E Sequence.!ESC![0m
    pause
    exit /b 1
)
echo !ESC![92m[PASS] Architecture Audit Completed Successfully.!ESC![0m
echo.

:: -----------------------------------------------------------------------------
:: PHASE 2: PYTHON UNIT TESTS
:: -----------------------------------------------------------------------------
echo.
echo !ESC![96m[PHASE 2] RUNNING PYTHON UNIT TESTS (pytest)...!ESC![0m
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" -m pytest tests/unit/ -v --tb=short
if !ERRORLEVEL! NEQ 0 (
    echo !ESC![91m[FATAL ERROR] Phase 2: Python Unit Tests FAILED. Aborting E2E Sequence.!ESC![0m
    pause
    exit /b 1
)
echo !ESC![92m[PASS] Python Unit Tests Completed Successfully.!ESC![0m
echo.

:: -----------------------------------------------------------------------------
:: PHASE 3: RUST BACKEND TESTS
:: -----------------------------------------------------------------------------
echo.
echo !ESC![96m[PHASE 3] RUNNING RUST BACKEND TESTS (cargo test)...!ESC![0m
if exist "%BF_DIR%" (
    cd /d "%BF_DIR%"
    call cargo test --all
    if !ERRORLEVEL! NEQ 0 (
        echo !ESC![91m[FATAL ERROR] Phase 3: Rust Tests FAILED. Aborting E2E Sequence.!ESC![0m
        pause
        exit /b 1
    )
    echo !ESC![92m[PASS] Rust Backend Tests Completed Successfully.!ESC![0m
) else (
    echo !ESC![93m[WARN] BankFidelity Rust directory not found at %BF_DIR%. Skipping Phase 3.!ESC![0m
)
echo.

:: -----------------------------------------------------------------------------
:: PHASE 4: LIVE UI SMOKE TEST
:: -----------------------------------------------------------------------------
echo.
echo !ESC![96m[PHASE 4] RUNNING LIVE UI SMOKE TEST (Notepad Automation)...!ESC![0m
echo !ESC![93mPLEASE REMOVE HANDS FROM KEYBOARD AND MOUSE.!ESC![0m
timeout /t 5
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\smoke_test_e2e.py"
if !ERRORLEVEL! NEQ 0 (
    echo !ESC![91m[FATAL ERROR] Phase 4: Live UI Smoke Test FAILED.!ESC![0m
    pause
    exit /b 1
)
echo !ESC![92m[PASS] Live UI Smoke Test Completed Successfully.!ESC![0m
echo.

:: -----------------------------------------------------------------------------
:: SUCCESS
:: -----------------------------------------------------------------------------
echo !ESC![92m================================================================================!ESC![0m
echo !ESC![92m                          SUPER E2E GAUNTLET PASSED!!ESC![0m
echo !ESC![92m================================================================================!ESC![0m
pause
exit /b 0
