@echo off
setlocal EnableDelayedExpansion
title UFO Automated E2E Test (Elevated)
chcp 65001 >nul

:: Check for Administrator privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Requesting administrative privileges...
    powershell -NoProfile -Command "Start-Process cmd -ArgumentList '/k cd /d C:\ufo\ufo & \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

cd /d C:\ufo\ufo
call scripts\smoke_test_e2e.bat
pause
