@echo off
setlocal EnableDelayedExpansion
title UFO End-to-End Tests
color 0E
chcp 65001 >nul

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONPATH=%UFO_ROOT%"
cd /d "%UFO_ROOT%"

:menu
cls
echo ======================================================================
echo   UFO END-TO-END DIAGNOSTICS ^& SMOKE TESTS
echo ======================================================================
echo.
echo  [1] Cloud Smoke Test (Gemini API)
echo  [2] Multi-Agent Showcase
echo  [3] Observe Last Results
echo  [4] Sequential Architecture Audit
echo  [0] Exit
echo.
set /p c="Choice:> "

if "%c%"=="1" goto cloud
if "%c%"=="2" goto showcase
if "%c%"=="3" goto observe
if "%c%"=="4" goto audit
if "%c%"=="0" exit /b 0
goto menu

:cloud
cls
echo [Step 1/1] Running Cloud Smoke Test Launcher...
call scripts\cloud_smoke_test.bat
cd /d "%UFO_ROOT%"
goto menu

:showcase
cls
echo [Showcase] Running Compound Task (Calculator -^> Notepad)
"%PYTHON_EXE%" -m ufo --request "Open Calculator and calculate 25 times 4. After getting the result, open Notepad, write 'The result of 25 x 4 is: 100' and save the file to the Desktop as ufo_stage8_result.txt."
pause
goto menu

:observe
cls
call scripts\observe_smoke_results.bat
cd /d "%UFO_ROOT%"
goto menu

:audit
cls
echo [Audit] Running UFO Sequential E2E Architecture Audit...
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
pause
goto menu
