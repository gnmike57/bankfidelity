@echo off
setlocal EnableDelayedExpansion
title UFO End-to-End Tests
color 0E
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONPATH=%UFO_ROOT%"
cd /d "%UFO_ROOT%"

:menu
cls
echo !ESC![90m======================================================================!ESC![0m
echo !ESC![93;1m  UFO END-TO-END DIAGNOSTICS ^& SMOKE TESTS!ESC![0m
echo !ESC![90m======================================================================!ESC![0m
echo.
echo  !ESC![97m[1]!ESC![0m !ESC![96mCloud Smoke Test (Gemini API)!ESC![0m
echo  !ESC![97m[2]!ESC![0m !ESC![96mMulti-Agent Showcase!ESC![0m
echo  !ESC![97m[3]!ESC![0m !ESC![96mObserve Last Results!ESC![0m
echo  !ESC![97m[4]!ESC![0m !ESC![93mSequential Architecture Audit!ESC![0m
echo  !ESC![97m[0]!ESC![0m !ESC![91mExit!ESC![0m
echo.
set /p c="!ESC![93mChoice:>!ESC![0m "

if "%c%"=="1" goto cloud
if "%c%"=="2" goto showcase
if "%c%"=="3" goto observe
if "%c%"=="4" goto audit
if "%c%"=="0" exit /b 0
goto menu

:cloud
cls
echo !ESC![96m[Step 1/1] Running Cloud Smoke Test Launcher...!ESC![0m
call scripts\cloud_smoke_test.bat
cd /d "%UFO_ROOT%"
goto menu

:showcase
cls
echo !ESC![96m[Showcase] Running Compound Task (Calculator -^> Notepad)!ESC![0m
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
echo !ESC![93m[Audit] Running UFO Sequential E2E Architecture Audit...!ESC![0m
"%PYTHON_EXE%" scripts\diagnostics.py --audit
pause
goto menu
