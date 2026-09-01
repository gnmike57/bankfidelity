@echo off
setlocal EnableDelayedExpansion
title Booting UFO AI Dream Team (Local LLMs)
color 0A
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"

:MENU
cls
echo !ESC![90m======================================================================!ESC![0m
echo !ESC![92;1m  LOCAL AI DREAM TEAM // LIFECYCLE MANAGER!ESC![0m
echo !ESC![90m======================================================================!ESC![0m
echo.
echo  !ESC![97m[1]!ESC![0m !ESC![92mStart Dream Team (Local Vision LLMs)!ESC![0m
echo  !ESC![97m[2]!ESC![0m !ESC![91mStop Dream Team (Local LLMs)!ESC![0m
echo  !ESC![97m[0]!ESC![0m !ESC![90mExit!ESC![0m
echo.
set /p CHOICE="!ESC![96mSelect Option:>!ESC![0m "

if "%CHOICE%"=="1" goto START_LLM
if "%CHOICE%"=="2" goto STOP_LLM
if "%CHOICE%"=="0" exit /b 0

goto MENU

:START_LLM
cls
echo !ESC![92mStarting Local LLM Stack...!ESC![0m
if exist "%UFO_ROOT%\scripts\setup_dream_team.bat" (
    call "%UFO_ROOT%\scripts\setup_dream_team.bat"
) else (
    echo !ESC![91m[ERROR] setup_dream_team.bat not found at %UFO_ROOT%\scripts\!ESC![0m
    pause
)
goto MENU

:STOP_LLM
cls
echo !ESC![91mStopping Local LLM Stack...!ESC![0m
if exist "%UFO_ROOT%\scripts\stop_local_llm.bat" (
    call "%UFO_ROOT%\scripts\stop_local_llm.bat"
) else (
    echo !ESC![91m[ERROR] stop_local_llm.bat not found at %UFO_ROOT%\scripts\!ESC![0m
    pause
)
goto MENU
