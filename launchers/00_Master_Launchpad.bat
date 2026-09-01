@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // MASTER SYSTEM ORCHESTRATOR
color 0B
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

echo !ESC![96mInitializing BankFidelity + UFO Dual-Core Orchestrator...!ESC![0m
if exist "%~dp0BankFidelity_Matrix.ps1" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0BankFidelity_Matrix.ps1"
)

:menu
cls
echo.
echo !ESC![90m==============================================================================================================!ESC![0m
echo !ESC![92;1m                                 BANKFIDELITY // MASTER SYSTEM ORCHESTRATOR!ESC![0m
echo !ESC![97m                                100%% Visual Fidelity • Dual-Core AI Architecture!ESC![0m
echo !ESC![90m==============================================================================================================!ESC![0m
echo.
echo   !ESC![97m[1]!ESC![0m !ESC![92mBANKFIDELITY CORE GUI & TERMINAL!ESC![0m      (Native Rust egui UI, Smart Balance Engine, REPL, Direct Edit)
echo   !ESC![97m[2]!ESC![0m !ESC![93mCROSS-BANK TRANSFER STRESS TEST!ESC![0m       (Real API Pairwise Matrix: CBA, Westpac, Bankwest, ING, Macquarie)
echo   !ESC![97m[3]!ESC![0m !ESC![95mVISION AI SUB-PIXEL CALIBRATION!ESC![0m       (300 DPI Rasterization, SSIM / PSNR Diffing, Iterative Correction)
echo   !ESC![97m[4]!ESC![0m !ESC![96mUFO DUAL-CORE AGENT SURGERY!ESC![0m           (Autonomous Desktop UI Agent, MCP Stdio Bridge, Task Dispatch)
echo   !ESC![97m[5]!ESC![0m !ESC![94mDREAM TEAM LOCAL VISION STACK!ESC![0m         (Offline Qwen-VL + Gemma-4 + LiteLLM :4000 Port Proxy)
echo   !ESC![97m[6]!ESC![0m !ESC![92mFULL-LIFECYCLE CERTIFICATION!ESC![0m          (Unattended 6-Gate End-to-End Test & Certification Gauntlet)
echo   !ESC![97m[7]!ESC![0m !ESC![95mSUBSYSTEM HEALTH & DOCTOR!ESC![0m             (Hardware, Memory, API Keys, Fallbacks, Template Validations)
echo   !ESC![97m[8]!ESC![0m !ESC![91mSUPER E2E ARCHITECTURE AUDIT!ESC![0m          (Full-Stack PyTest + Cargo Test + Notepad Live Automation)
echo   !ESC![97m[9]!ESC![0m !ESC![97mCONFIGURATION & MASTER API KEYS!ESC![0m       (Manage Reducto, Gemini, PyMuPDF Pro, Passphrases & Env)
echo.
echo   !ESC![97m[X]!ESC![0m !ESC![90mEXIT ORCHESTRATOR!ESC![0m
echo !ESC![90m==============================================================================================================!ESC![0m
set /p choice="!ESC![92mSYS_COMMAND_>!ESC![0m "

if /i "!choice!"=="1" start "" "%~dp001_BankFidelity_Terminal.bat"
if /i "!choice!"=="2" start "" "%~dp011_Matrix_Stress_Test.bat"
if /i "!choice!"=="3" start "" "%~dp012_Vision_AI_Calibration.bat"
if /i "!choice!"=="4" start "" "%~dp002_UFO_Control_Panel.bat"
if /i "!choice!"=="5" start "" "%~dp003_AI_Dream_Team_Launcher.bat"
if /i "!choice!"=="6" start "" powershell.exe -NoProfile -ExecutionPolicy Bypass -File "C:\bankfidelity\bankfidelity\scripts\run_lifecycle_certification.ps1"
if /i "!choice!"=="7" start "" "%~dp004_E2E_Diagnostics.bat"
if /i "!choice!"=="8" start "" "%~dp008_Super_E2E_Test.bat"
if /i "!choice!"=="9" start "" "%~dp007_Configuration_Dashboard.bat"
if /i "!choice!"=="X" exit /b 0

goto menu
