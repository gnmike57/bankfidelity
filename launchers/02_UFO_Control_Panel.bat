@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul
title UFO Master Control Panel
color 0B

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONIOENCODING=utf-8"
set "PYTHONPATH=%UFO_ROOT%"

if not exist "%PYTHON_EXE%" (
    echo !ESC![91m[FATAL] python_env not found at %PYTHON_EXE%.!ESC![0m
    pause
    exit /b 1
)

:menu
cls
echo.
echo !ESC![96m +==============================================================+!ESC![0m
echo !ESC![96m |!ESC![0m !ESC![97;1m             UFO MASTER CONTROL PANEL                       !ESC![0m!ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m !ESC![93m         Vision-Based Windows UI Automation                 !ESC![0m!ESC![96m|!ESC![0m
echo !ESC![96m +==============================================================+!ESC![0m
echo !ESC![96m |!ESC![0m                                                              !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[1]!ESC![0m  !ESC![92mRun UFO Task (Interactive)!ESC![0m                          !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[2]!ESC![0m  !ESC![92mRun UFO Planner (Follower Mode)!ESC![0m                     !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[3]!ESC![0m  !ESC![96mUFO Preflight Check!ESC![0m                                 !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[4]!ESC![0m  !ESC![95mLaunch AI Dream Team (Local Vision LLMs)!ESC![0m            !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[5]!ESC![0m  !ESC![95mStop AI Dream Team (Local LLMs)!ESC![0m                     !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[6]!ESC![0m  !ESC![93mRun Sequential Architecture Audit!ESC![0m                   !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[7]!ESC![0m  !ESC![93mRun PyTest Unit Tests!ESC![0m                               !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[8]!ESC![0m  !ESC![97mView UFO Configuration (agents.yaml)!ESC![0m                !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m   !ESC![97m[0]!ESC![0m  !ESC![91mExit!ESC![0m                                                !ESC![96m|!ESC![0m
echo !ESC![96m |!ESC![0m                                                              !ESC![96m|!ESC![0m
echo !ESC![96m +==============================================================+!ESC![0m
echo.
set "choice="
set /p "choice=!ESC![96m  Select option [0-9]:>!ESC![0m "

if "%choice%"=="1" goto :run_task
if "%choice%"=="2" goto :run_follower
if "%choice%"=="3" goto :preflight
if "%choice%"=="4" goto :start_llm
if "%choice%"=="5" goto :stop_llm
if "%choice%"=="6" goto :run_audit
if "%choice%"=="7" goto :unit_tests
if "%choice%"=="8" goto :view_config
if "%choice%"=="0" exit /b 0

goto :menu

:run_task
cls
echo !ESC![92m ==============================================================!ESC![0m
echo !ESC![92;1m  RUN UFO TASK!ESC![0m
echo !ESC![92m ==============================================================!ESC![0m
set "task="
set /p "task=!ESC![97m  Task:>!ESC![0m "
if not "%task%"=="" (
    cd /d "%UFO_ROOT%"
    "%PYTHON_EXE%" -m ufo --task "%task%"
)
pause
goto :menu

:run_follower
cls
echo !ESC![92m ==============================================================!ESC![0m
echo !ESC![92;1m  RUN UFO FOLLOWER!ESC![0m
echo !ESC![92m ==============================================================!ESC![0m
set /p PLAN_PATH="!ESC![97m  Enter absolute path to Plan file:>!ESC![0m "
if not "%PLAN_PATH%"=="" (
    cd /d "%UFO_ROOT%"
    "%PYTHON_EXE%" -m ufo --mode follower --plan "%PLAN_PATH%"
)
pause
goto :menu

:preflight
cls
echo !ESC![96m ==============================================================!ESC![0m
echo !ESC![96;1m  UFO PREFLIGHT CHECK!ESC![0m
echo !ESC![96m ==============================================================!ESC![0m
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" scripts\preflight.py
pause
goto :menu

:start_llm
cls
echo !ESC![95m ==============================================================!ESC![0m
echo !ESC![95;1m  LAUNCHING AI DREAM TEAM!ESC![0m
echo !ESC![95m ==============================================================!ESC![0m
if exist "%UFO_ROOT%\scripts\setup_dream_team.bat" (
    call "%UFO_ROOT%\scripts\setup_dream_team.bat"
) else (
    echo !ESC![91m[ERROR] setup_dream_team.bat not found.!ESC![0m
)
pause
goto :menu

:stop_llm
cls
echo !ESC![95m ==============================================================!ESC![0m
echo !ESC![95;1m  STOPPING AI DREAM TEAM!ESC![0m
echo !ESC![95m ==============================================================!ESC![0m
if exist "%UFO_ROOT%\scripts\stop_local_llm.bat" (
    call "%UFO_ROOT%\scripts\stop_local_llm.bat"
) else (
    echo !ESC![91m[ERROR] stop_local_llm.bat not found.!ESC![0m
)
pause
goto :menu

:run_audit
cls
echo !ESC![93m ==============================================================!ESC![0m
echo !ESC![93;1m  RUNNING SEQUENTIAL ARCHITECTURE AUDIT!ESC![0m
echo !ESC![93m ==============================================================!ESC![0m
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
pause
goto :menu

:unit_tests
cls
echo !ESC![93m ==============================================================!ESC![0m
echo !ESC![93;1m  RUNNING UNIT TESTS!ESC![0m
echo !ESC![93m ==============================================================!ESC![0m
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" -m pytest tests\agents\ tests\automator\ tests\zero_fail\ tests\aip\ -v --tb=short
pause
goto :menu

:view_config
cls
echo !ESC![97m ==============================================================!ESC![0m
echo !ESC![97;1m  UFO CONFIGURATION (agents.yaml)!ESC![0m
echo !ESC![97m ==============================================================!ESC![0m
if exist "%UFO_ROOT%\config\ufo\agents.yaml" type "%UFO_ROOT%\config\ufo\agents.yaml"
pause
goto :menu
