@echo off
setlocal
title UFO Visual Diagnostic - Eyes Gauntlet
chcp 65001 >nul
echo ========================================
echo   UFO Visual Diagnostic - Eyes Gauntlet
echo ========================================
echo.
echo This test launches 3 apps, switches focus, captures
echo screenshots, and validates UFO's vision pipeline.
echo.
echo WARNING: Do not touch mouse or keyboard during test.
echo.

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"

"%PYTHON_EXE%" "%UFO_ROOT%\scratch\visual_diagnostic.py"
set EXIT_CODE=%ERRORLEVEL%

echo.
if %EXIT_CODE% EQU 0 (
    echo SUCCESS: UFO's vision pipeline is fully operational.
) else if %EXIT_CODE% EQU 1 (
    echo WARNING: Some visual checks failed - review the report.
) else (
    echo ERROR: Visual diagnostic script crashed.
)

echo.
echo Report: %UFO_ROOT%\logs\visual_diagnostic\diagnostic_report.md
echo Screenshots: %UFO_ROOT%\logs\visual_diagnostic\
echo.
pause
exit /b %EXIT_CODE%
