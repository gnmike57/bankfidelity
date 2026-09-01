@echo off
setlocal EnableDelayedExpansion
set "PYTHONIOENCODING=utf-8"
title BankFidelity // Terminal
chcp 65001 >nul
color 0a

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

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
echo !ESC![90m===============================================================!ESC![0m
echo !ESC![92;1m              BANKFIDELITY SYSTEM TERMINAL!ESC![0m
echo !ESC![97m                 ORCHESTRATOR  //  MASTER!ESC![0m
echo !ESC![90m===============================================================!ESC![0m
echo.
echo  !ESC![97m[1]!ESC![0m !ESC![92mLaunch BankFidelity GUI (Release)!ESC![0m
echo  !ESC![97m[2]!ESC![0m !ESC![92mLaunch BankFidelity GUI (Debug)!ESC![0m
echo  !ESC![97m[3]!ESC![0m !ESC![96mBoot Headless HTTP Server!ESC![0m
echo  !ESC![97m[4]!ESC![0m !ESC![96mBoot MCP Server (stdio)!ESC![0m
echo  !ESC![97m[5]!ESC![0m !ESC![93mLocal AI Chat (CLI NLU)!ESC![0m
echo  !ESC![97m[6]!ESC![0m !ESC![95mSubsystem Diagnostics & Doctor!ESC![0m
echo  !ESC![97m[7]!ESC![0m !ESC![95mAPI Key Status Verification!ESC![0m
echo  !ESC![97m[8]!ESC![0m !ESC![96mRun Full Lifecycle Certification Gauntlet!ESC![0m
echo  !ESC![97m[0]!ESC![0m !ESC![91mExit!ESC![0m
echo.
set /p CHOICE="!ESC![92mSYS_REQ_>!ESC![0m "

if "%CHOICE%"=="1" (
    echo !ESC![96m[launch] Starting BankFidelity GUI (Release)...!ESC![0m
    cargo run --release -- gui
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="2" (
    echo !ESC![96m[launch] Starting BankFidelity GUI (Debug)...!ESC![0m
    cargo run -- gui
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="3" (
    echo !ESC![96m[launch] Booting HTTP Server on port 8080...!ESC![0m
    cargo run -- serve
    pause
    goto MAIN_MENU
)
if "%CHOICE%"=="4" (
    echo !ESC![96m[launch] Starting MCP Server stdio loop...!ESC![0m
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
