@echo off
setlocal EnableDelayedExpansion
set "PYTHONIOENCODING=utf-8"
title BankFidelity // Terminal
chcp 65001 >nul
color 0a

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

:: 1. Force directory context to script root
pushd "%~dp0"
if !ERRORLEVEL! NEQ 0 (
    echo !ESC![91m[CRITICAL ERROR] Failed to switch working directory context to "%~dp0"!ESC![0m
    pause
    exit /b 1
)

:: Check admin status
net session >nul 2>&1
if !ERRORLEVEL! NEQ 0 (
    title BankFidelity // Terminal [NOT ADMIN]
) else (
    title BankFidelity // Terminal [ADMIN]
)

:: Validate BankFidelity path
set "BF_DIR=C:\bankfidelity\bankfidelity"
if not exist "%BF_DIR%\" (
    echo !ESC![91m[ERROR] BankFidelity directory not found at %BF_DIR%!ESC![0m
    pause
    exit /b 1
)

:: Validate Python Environment
set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
if not exist "%PYTHON_EXE%" (
    for /f "delims=" %%P in ('where python.exe 2^>nul') do set "PYTHON_EXE=%%P"
)

if not defined PYTHON_EXE (
    echo !ESC![91m[ERROR] Python executable not found.!ESC![0m
    pause
    exit /b 1
)

:: Initialize Python paths
for %%P in ("!PYTHON_EXE!") do set "PYTHON_DIR=%%~dpP"
set "PYO3_PYTHON=!PYTHON_EXE!"
set "PYTHON_SYS_EXECUTABLE=!PYTHON_EXE!"
set "PYTHONHOME=!PYTHON_DIR!"
set "PYTHONPATH=!PYTHON_DIR!Lib\site-packages;!PYTHON_DIR!Lib;!PYTHON_DIR!DLLs;%UFO_ROOT%"
set "PATH=!PYTHON_DIR!;C:\msys64\mingw64\bin;%USERPROFILE%\.cargo\bin;C:\Program Files\nodejs;!PATH!"

:: Optional Matrix Intro
if not defined SKIP_MATRIX (
    if exist "%~dp0BankFidelity_Matrix.ps1" (
        powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Maximized -File "%~dp0BankFidelity_Matrix.ps1"
    )
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
echo  !ESC![97m[1]!ESC![0m !ESC![92mLaunch BankFidelity GUI (PowerShell)!ESC![0m
echo  !ESC![97m[2]!ESC![0m !ESC![92mLaunch BankFidelity GUI (Bash)!ESC![0m
echo  !ESC![97m[3]!ESC![0m !ESC![92mLaunch PDF_SITCH Terminal (CMD)!ESC![0m
echo  !ESC![97m[4]!ESC![0m !ESC![96mBoot Headless HTTP Server!ESC![0m
echo  !ESC![97m[5]!ESC![0m !ESC![96mBoot MCP Server!ESC![0m
echo  !ESC![97m[6]!ESC![0m !ESC![93mLocal LLM Chat Orchestrator!ESC![0m
echo  !ESC![97m[7]!ESC![0m !ESC![95mSystem Health ^& Diagnostics (Doctor)!ESC![0m
echo  !ESC![97m[8]!ESC![0m !ESC![95mRust Clippy ^& Formatting Check!ESC![0m
echo  !ESC![97m[9]!ESC![0m !ESC![96mUFO Sequential E2E Architecture Audit!ESC![0m
echo  !ESC![97m[0]!ESC![0m !ESC![91mExit System!ESC![0m
echo.
set /p CHOICE="!ESC![92mSYS_REQ_>!ESC![0m "

if "%CHOICE%"=="1" goto LAUNCH_GUI_PS
if "%CHOICE%"=="2" goto LAUNCH_GUI_BASH
if "%CHOICE%"=="3" goto LAUNCH_PDF_SITCH
if "%CHOICE%"=="4" goto BOOT_HTTP
if "%CHOICE%"=="5" goto BOOT_MCP
if "%CHOICE%"=="6" goto LLM_CHAT
if "%CHOICE%"=="7" goto SYS_DOCTOR
if "%CHOICE%"=="8" goto TS_CLIPPY
if "%CHOICE%"=="9" goto RUN_AUDIT
if "%CHOICE%"=="0" exit /b 0

goto MAIN_MENU


:LAUNCH_GUI_PS
echo !ESC![96m[launch] Starting BankFidelity GUI (PowerShell)...!ESC![0m
start "BankFidelity GUI" powershell.exe -ExecutionPolicy Bypass -File "launch.ps1"
goto MAIN_MENU

:LAUNCH_GUI_BASH
echo !ESC![96m[launch] Starting BankFidelity GUI (Bash)...!ESC![0m
start "BankFidelity Bash GUI" bash "launch.sh"
goto MAIN_MENU

:LAUNCH_PDF_SITCH
echo !ESC![96m[launch] Starting PDF_SITCH Terminal UI...!ESC![0m
start "PDF_SITCH Terminal UI" cmd /k "cargo run --release"
goto MAIN_MENU

:BOOT_HTTP
echo !ESC![95m   STARTING HEADLESS HTTP SERVER!ESC![0m
cargo run --release -- serve
pause
goto MAIN_MENU

:BOOT_MCP
echo !ESC![95m   STARTING MCP SERVER!ESC![0m
cargo run --release -- mcp
pause
goto MAIN_MENU

:LLM_CHAT
echo !ESC![95m   LOCAL LLM CHAT ORCHESTRATOR!ESC![0m
set /p CHAT_PROMPT="!ESC![92mEnter Instruction for AI:>!ESC![0m "
if not "!CHAT_PROMPT!"=="" (
    cargo run --release -- chat -i "!CHAT_PROMPT!"
    pause
)
goto MAIN_MENU

:SYS_DOCTOR
echo !ESC![93m   RUNNING SYSTEM HEALTH ^& DIAGNOSTICS!ESC![0m
call cargo run --release -- doctor
if !ERRORLEVEL! NEQ 0 ( echo !ESC![91m[ERROR] Doctor command failed.!ESC![0m & pause & goto MAIN_MENU )
call cargo run --release -- verify-api-keys
pause
goto MAIN_MENU

:TS_CLIPPY
echo !ESC![93m   RUNNING RUSTFMT ^& CLIPPY!ESC![0m
call cargo fmt --check
if !ERRORLEVEL! NEQ 0 ( echo !ESC![91m[ERROR] Rustfmt failed.!ESC![0m & pause & goto MAIN_MENU )
call cargo clippy --all-targets
pause
goto MAIN_MENU

:RUN_AUDIT
echo !ESC![96m   RUNNING UFO SEQUENTIAL ARCHITECTURE AUDIT!ESC![0m
cd /d "%UFO_ROOT%"
"%PYTHON_EXE%" "%UFO_ROOT%\scripts\audit_e2e_sequential.py"
pause
goto MAIN_MENU
