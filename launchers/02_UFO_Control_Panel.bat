@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul
title UFO Master Control Panel
color 0B

set "BF_DIR=C:\bankfidelity\bankfidelity"
set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
set "PYTHONIOENCODING=utf-8"
set "PYTHONPATH=%UFO_ROOT%;%BF_DIR%"

if not exist "%PYTHON_EXE%" (
    echo [FATAL] python_env not found at %PYTHON_EXE%.
    pause
    exit /b 1
)

:menu
cls
echo.
echo  +==============================================================+
echo  |              UFO MASTER CONTROL PANEL                       |
echo  |          Vision-Based Windows UI Automation                 |
echo  +==============================================================+
echo  |                                                              |
echo  |   [1]  Run UFO Task (Interactive)                          |
echo  |   [2]  Run UFO Follower (Plan Execution)                   |
echo  |   [3]  UFO Preflight Check                                 |
echo  |   [4]  Launch AI Dream Team (Local Vision LLMs)            |
echo  |   [5]  Stop AI Dream Team (Local LLMs)                     |
echo  |   [6]  Run Sequential Architecture Audit                   |
echo  |   [7]  Run PyTest Unit Tests                               |
echo  |   [8]  View UFO Configuration (agents.yaml)                |
echo  |   [0]  Exit                                                |
echo  |                                                              |
echo  +==============================================================+
echo.
set "choice="
set /p "choice=  Select option [0-8]:> "

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
echo  ==============================================================
echo   RUN UFO TASK
echo  ==============================================================
set "task="
set /p "task=  Task:> "
if not "%task%"=="" (
    cd /d "%UFO_ROOT%"
    "%PYTHON_EXE%" -m ufo --task "%task%"
)
pause
goto :menu

:run_follower
cls
echo  ==============================================================
echo   RUN UFO FOLLOWER
echo  ==============================================================
set /p PLAN_PATH="  Enter absolute path to Plan file:> "
if not "%PLAN_PATH%"=="" (
    cd /d "%UFO_ROOT%"
    "%PYTHON_EXE%" -m ufo --mode follower --plan "%PLAN_PATH%"
)
pause
goto :menu

:preflight
cls
echo  ==============================================================
echo   UFO PREFLIGHT CHECK
echo  ==============================================================
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" -c "import sys; sys.path.insert(0, 'scripts/smoke_tests'); import smoke_test_e2e as s; print(s.check_foreground_window()); print(s.check_agents_yaml()); print(s.check_mcp_config()); print(s.check_session_import())"
pause
goto :menu

:start_llm
cls
echo  ==============================================================
echo   LAUNCHING AI DREAM TEAM
echo  ==============================================================
if exist "%UFO_ROOT%\scripts\setup_dream_team.bat" (
    call "%UFO_ROOT%\scripts\setup_dream_team.bat"
) else (
    echo [ERROR] setup_dream_team.bat not found.
)
pause
goto :menu

:stop_llm
cls
echo  ==============================================================
echo   STOPPING AI DREAM TEAM
echo  ==============================================================
if exist "%UFO_ROOT%\scripts\stop_local_llm.bat" (
    call "%UFO_ROOT%\scripts\stop_local_llm.bat"
) else (
    echo [ERROR] stop_local_llm.bat not found.
)
pause
goto :menu

:run_audit
cls
echo  ==============================================================
echo   RUNNING SEQUENTIAL ARCHITECTURE AUDIT
echo  ==============================================================
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
pause
goto :menu

:unit_tests
cls
echo  ==============================================================
echo   RUNNING UNIT TESTS
echo  ==============================================================
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" -m pytest tests/unit/ -v --tb=short
pause
goto :menu

:view_config
cls
echo  ==============================================================
echo   UFO CONFIGURATION (agents.yaml)
echo  ==============================================================
if exist "%UFO_ROOT%\config\ufo\agents.yaml" type "%UFO_ROOT%\config\ufo\agents.yaml"
pause
goto :menu
