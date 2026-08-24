@echo off
setlocal EnableDelayedExpansion
title UFO Automated E2E Test (Elevated)

:: Check for Administrator privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Requesting administrative privileges...
    powershell -Command "Start-Process cmd -ArgumentList '/c cd /d C:\ufo\ufo & \""%~f0\""' -Verb RunAs"
    exit /b
)

cd /d C:\ufo\ufo
call scripts\smoke_test_e2e.bat
pause
