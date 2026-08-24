@echo off
setlocal EnableDelayedExpansion
title UFO Interactive Desktop Shell (Elevated)

:: Check for Administrator privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Requesting administrative privileges...
    powershell -Command "Start-Process cmd -ArgumentList '/c cd /d C:\ufo\ufo & \""%~f0\""' -Verb RunAs"
    exit /b
)

echo ======================================================================
echo  UFO INTERACTIVE DESKTOP SHELL
echo  Full Rights: Screenshots, Telemetry, UI Control, Admin Access
echo ======================================================================
echo.

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
if not exist "%PYTHON_EXE%" (
    for /f "delims=" %%P in ('where python.exe 2^>nul') do set "PYTHON_EXE=%%P"
)

cd /d "%UFO_ROOT%"

echo You are now operating with full desktop context and admin privileges.
echo The GetForegroundWindow API will now function correctly.
echo.
echo Type your task in plain English and hit Enter.
echo Type 'smoke' to run the E2E verification test.
echo Type 'exit' to close.
echo.

:loop
set /p user_task="UFO Task > "
if /i "%user_task%"=="" goto loop
if /i "%user_task%"=="exit" exit /b
if /i "%user_task%"=="smoke" (
    call scripts\smoke_test_e2e.bat
) else (
    "%PYTHON_EXE%" -m ufo --task "%user_task%"
)
echo.
goto loop
