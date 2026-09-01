@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY UFO // SUPER E2E TEST GAUNTLET
color 0E
chcp 65001 >nul

:: Set ESC character for ANSI colors

set "UFO_ROOT=C:\ufo\ufo"
set "BF_DIR=C:\bankfidelity\bankfidelity"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONIOENCODING=utf-8"

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
echo HASE 1] RUNNING SEQUENTIAL ARCHITECTURE AUDIT...
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
if !ERRORLEVEL! NEQ 0 (
    echo ATAL ERROR] Phase 1: Architecture Audit FAILED. Aborting E2E Sequence.
    pause
    exit /b 1
)
echo ASS] Architecture Audit Completed Successfully.
echo.

:: -----------------------------------------------------------------------------
:: PHASE 2: PYTHON UNIT TESTS
:: -----------------------------------------------------------------------------
echo.
echo HASE 2] RUNNING PYTHON UNIT TESTS (pytest)...
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" -m pytest tests/unit/ -v --tb=short
if !ERRORLEVEL! NEQ 0 (
    echo ATAL ERROR] Phase 2: Python Unit Tests FAILED. Aborting E2E Sequence.
    pause
    exit /b 1
)
echo ASS] Python Unit Tests Completed Successfully.
echo.

:: -----------------------------------------------------------------------------
:: PHASE 3: RUST BACKEND TESTS
:: -----------------------------------------------------------------------------
echo.
echo HASE 3] RUNNING RUST BACKEND TESTS (cargo test)...
if exist "%BF_DIR%" (
    cd /d "%BF_DIR%"
    call cargo test --all
    if !ERRORLEVEL! NEQ 0 (
        echo ATAL ERROR] Phase 3: Rust Tests FAILED. Aborting E2E Sequence.
        pause
        exit /b 1
    )
    echo ASS] Rust Backend Tests Completed Successfully.
) else (
    echo ARN] BankFidelity Rust directory not found at %BF_DIR%. Skipping Phase 3.
)
echo.

:: -----------------------------------------------------------------------------
:: PHASE 4: LIVE UI SMOKE TEST
:: -----------------------------------------------------------------------------
echo.
echo HASE 4] RUNNING LIVE UI SMOKE TEST (Notepad Automation)...
echo PLEASE REMOVE HANDS FROM KEYBOARD AND MOUSE.
timeout /t 5
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\smoke_test_e2e.py"
if !ERRORLEVEL! NEQ 0 (
    echo ATAL ERROR] Phase 4: Live UI Smoke Test FAILED.
    pause
    exit /b 1
)
echo ASS] Live UI Smoke Test Completed Successfully.
echo.

:: -----------------------------------------------------------------------------
:: SUCCESS
:: -----------------------------------------------------------------------------
echo ================================================================================
echo                           SUPER E2E GAUNTLET PASSED!
echo ================================================================================
pause
exit /b 0
