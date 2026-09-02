@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY UFO // SUPER E2E TEST GAUNTLET
color 0E
chcp 65001 >nul

set "UFO_ROOT=C:\ufo\ufo"
set "BF_DIR=C:\bankfidelity\bankfidelity"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONIOENCODING=utf-8"
set "PYTHONPATH=%UFO_ROOT%;%BF_DIR%"

cls
echo ================================================================================
echo                          UFO SUPER E2E TEST GAUNTLET
echo ================================================================================
echo This test sequence will validate the entire architecture from bottom to top:
echo   1. Static Architecture Audit
echo   2. Python Unit and Integration Tests (pytest)
echo   3. Rust Backend Tests (cargo test)
echo   4. Live UI Control Smoke Test (Notepad)
echo.
echo WARNING: DO NOT TOUCH THE MOUSE OR KEYBOARD ONCE PHASE 4 BEGINS.
echo.
pause

:: -----------------------------------------------------------------------------
:: PHASE 1: STATIC ARCHITECTURE AUDIT
:: -----------------------------------------------------------------------------
echo.
echo [PHASE 1] RUNNING SEQUENTIAL ARCHITECTURE AUDIT...
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
if !ERRORLEVEL! NEQ 0 (
    echo [FATAL ERROR] Phase 1: Architecture Audit FAILED. Aborting E2E Sequence.
    pause
    exit /b 1
)
echo [PASS] Architecture Audit Completed Successfully.
echo.

:: -----------------------------------------------------------------------------
:: PHASE 2: PYTHON UNIT TESTS
:: -----------------------------------------------------------------------------
echo.
echo [PHASE 2] RUNNING PYTHON UNIT TESTS (pytest)...
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" -m pytest tests/unit/ -v --tb=short
if !ERRORLEVEL! NEQ 0 (
    echo [FATAL ERROR] Phase 2: Python Unit Tests FAILED. Aborting E2E Sequence.
    pause
    exit /b 1
)
echo [PASS] Python Unit Tests Completed Successfully.
echo.

:: -----------------------------------------------------------------------------
:: PHASE 3: RUST BACKEND TESTS
:: -----------------------------------------------------------------------------
echo.
echo [PHASE 3] RUNNING RUST BACKEND TESTS (cargo test)...
if exist "%BF_DIR%" (
    cd /d "%BF_DIR%"
    call cargo test --lib
    if !ERRORLEVEL! NEQ 0 (
        echo [FATAL ERROR] Phase 3: Rust Tests FAILED. Aborting E2E Sequence.
        pause
        exit /b 1
    )
    echo [PASS] Rust Backend Tests Completed Successfully.
) else (
    echo [WARN] BankFidelity Rust directory not found at %BF_DIR%. Skipping Phase 3.
)
echo.

:: -----------------------------------------------------------------------------
:: PHASE 4: LIVE UI SMOKE TEST
:: -----------------------------------------------------------------------------
echo.
echo [PHASE 4] RUNNING LIVE UI SMOKE TEST (Notepad Automation)...
echo PLEASE REMOVE HANDS FROM KEYBOARD AND MOUSE.
timeout /t 3
cd /d "%UFO_ROOT%"
call "%UFO_ROOT%\scripts\smoke_test_e2e.bat"
if !ERRORLEVEL! NEQ 0 (
    echo [FATAL ERROR] Phase 4: Live UI Smoke Test FAILED.
    pause
    exit /b 1
)
echo [PASS] Live UI Smoke Test Completed Successfully.
echo.

:: -----------------------------------------------------------------------------
:: SUCCESS
:: -----------------------------------------------------------------------------
echo ================================================================================
echo                           SUPER E2E GAUNTLET PASSED!
echo ================================================================================
pause
exit /b 0
