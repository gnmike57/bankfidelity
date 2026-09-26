@echo off
setlocal EnableDelayedExpansion
title UFO Interactive Desktop Shell (Elevated)
chcp 65001 >nul
color 0A

:: Check for Administrator privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Requesting administrative privileges...
    powershell -NoProfile -Command "Start-Process cmd -ArgumentList '/k cd /d C:\ufo\ufo & \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

set "BF_DIR=C:\bankfidelity\bankfidelity"
set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONIOENCODING=utf-8"
set "PYTHONPATH=%UFO_ROOT%;%BF_DIR%"

cd /d "%UFO_ROOT%"

echo ======================================================================
echo  UFO INTERACTIVE DESKTOP SHELL
echo  Full Rights: Screenshots, Telemetry, UI Control, Admin Access
echo ======================================================================
echo.
echo You are now operating with full desktop context and admin privileges.
echo The GetForegroundWindow API will now function correctly.
echo.
echo Type your task in plain English and hit Enter.
echo Type 'smoke' to run the E2E verification test.
echo Type 'audit' to run the Sequential Architecture Audit.
echo Type 'exit' to close.
echo.

:loop
set "user_task="
set /p user_task="UFO Task > "
if /i "%user_task%"=="" goto loop
if /i "%user_task%"=="exit" exit /b 0
if /i "%user_task%"=="smoke" (
    call "%UFO_ROOT%\scripts\smoke_test_e2e.bat"
) else if /i "%user_task%"=="audit" (
    "%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
) else (
    "%PYTHON_EXE%" -m ufo --task "%user_task%"
)
echo.
goto loop
