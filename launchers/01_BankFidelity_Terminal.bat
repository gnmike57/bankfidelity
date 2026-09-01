@echo off
setlocal EnableDelayedExpansion
set "PYTHONIOENCODING=utf-8"
title BankFidelity // Terminal
chcp 65001 >nul
color 0a

:: Set ESC character for ANSI colors

pushd "%~dp0.."
set "BF_DIR=%CD%"
popd

:: Validate Python Environment
set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
if not exist "%PYTHON_EXE%" (
    for /f "delims=" %%P in ('where python.exe 2^>nul') do set "PYTHON_EXE=%%P"
)

:MAIN_MENU
cd /d "%BF_DIR%"
cls
echo.
echo ===============================================================
echo               BANKFIDELITY SYSTEM TERMINAL
echo                  ORCHESTRATOR  //  MASTER
echo ===============================================================
echo.
echo  [1] Launch BankFidelity GUI (Release)
echo  [2] Launch BankFidelity GUI (Debug)
echo  [3] Boot Headless HTTP Server
echo  [4] Boot MCP Server (stdio)
echo  [5] Local AI Chat (CLI NLU)
echo  [6] Subsystem Diagnostics and Doctor
echo  [7] API Key Status Verification
echo  [8] Run Full Lifecycle Certification Gauntlet
echo  [0] Exit
echo.
set /p CHOICE="SYS_REQ_> "

if "%CHOICE%"=="1" (
    echo aunch] Starting BankFidelity GUI (Release)...
    cargo run --release -- gui
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="2" (
    echo aunch] Starting BankFidelity GUI (Debug)...
    cargo run -- gui
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="3" (
    echo aunch] Booting HTTP Server on port 8080...
    cargo run -- serve
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="4" (
    echo aunch] Starting MCP Server stdio loop...
    cargo run -- mcp
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="5" (
    set /p PROMPT_TXT="Enter natural language instruction: "
    cargo run -- chat "!PROMPT_TXT!"
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="6" (
    cargo run -- doctor
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="7" (
    cargo run -- verify-api-keys
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="8" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%BF_DIR%\scripts\run_lifecycle_certification.ps1"
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="0" exit /b 0

goto MAIN_MENU
