@echo off
setlocal EnableDelayedExpansion
title Booting UFO AI Dream Team (Local LLMs)
color 0A
chcp 65001 >nul

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"

:MENU
cls
echo ======================================================================
echo   LOCAL AI DREAM TEAM // LIFECYCLE MANAGER
echo ======================================================================
echo.
echo  [1] Start Dream Team (Local Vision LLMs)
echo  [2] Stop Dream Team (Local LLMs)
echo  [0] Exit
echo.
set /p CHOICE="Select Option:> "

if "%CHOICE%"=="1" goto START_LLM
if "%CHOICE%"=="2" goto STOP_LLM
if "%CHOICE%"=="0" exit /b 0

goto MENU

:START_LLM
cls
echo Starting Local LLM Stack...
if exist "%UFO_ROOT%\scripts\setup_dream_team.bat" (
    call "%UFO_ROOT%\scripts\setup_dream_team.bat"
) else (
    echo [ERROR] setup_dream_team.bat not found at %UFO_ROOT%\scripts\
    pause
)
goto MENU

:STOP_LLM
cls
echo Stopping Local LLM Stack...
if exist "%UFO_ROOT%\scripts\stop_local_llm.bat" (
    call "%UFO_ROOT%\scripts\stop_local_llm.bat"
) else (
    echo [ERROR] stop_local_llm.bat not found at %UFO_ROOT%\scripts\
    pause
)
goto MENU
