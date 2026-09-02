@echo off
setlocal
title UFO Core UI Mechanics Sandbox Test
chcp 65001 >nul
echo ========================================
echo     UFO Core UI Mechanics Sandbox Test
echo ========================================
echo.
echo WARNING: This test will take over your mouse and keyboard.
echo Please do not touch your mouse or keyboard until the test finishes.
echo.

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"

"%PYTHON_EXE%" "%UFO_ROOT%\scratch\sandbox_ui_test.py"
set EXIT_CODE=%ERRORLEVEL%

echo.
if %EXIT_CODE% EQU 0 (
    echo SUCCESS: The core UI mechanics are fully operational.
) else (
    echo ERROR: The core UI mechanics failed during the sandbox test.
)

pause
exit /b %EXIT_CODE%
