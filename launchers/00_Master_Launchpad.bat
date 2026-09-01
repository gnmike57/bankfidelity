@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // MASTER SYSTEM ORCHESTRATOR
color 0B
chcp 65001 >nul

:: Set ESC character for ANSI colors

echo Initializing BankFidelity + UFO Dual-Core Orchestrator...
if exist "%~dp0BankFidelity_Matrix.ps1" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0BankFidelity_Matrix.ps1"
)

:menu
cls
echo.
echo ==============================================================================================================
echo                                  BANKFIDELITY // MASTER SYSTEM ORCHESTRATOR
echo                                 100%% Visual Fidelity • Dual-Core AI Architecture
echo ==============================================================================================================
echo.
echo   [1] BANKFIDELITY CORE GUI and TERMINAL      (Native Rust egui UI, Smart Balance Engine, REPL, Direct Edit)
echo   [2] CROSS-BANK TRANSFER STRESS TEST       (Real API Pairwise Matrix: CBA, Westpac, Bankwest, ING, Macquarie)
echo   [3] VISION AI SUB-PIXEL CALIBRATION       (300 DPI Rasterization, SSIM / PSNR Diffing, Iterative Correction)
echo   [4] UFO DUAL-CORE AGENT SURGERY           (Autonomous Desktop UI Agent, MCP Stdio Bridge, Task Dispatch)
echo   [5] DREAM TEAM LOCAL VISION STACK         (Offline Qwen-VL + Gemma-4 + LiteLLM :4000 Port Proxy)
echo   [6] FULL-LIFECYCLE CERTIFICATION          (Unattended 6-Gate End-to-End Test and Certification Gauntlet)
echo   [7] SUBSYSTEM HEALTH and DOCTOR             (Hardware, Memory, API Keys, Fallbacks, Template Validations)
echo   [8] SUPER E2E ARCHITECTURE AUDIT          (Full-Stack PyTest + Cargo Test + Notepad Live Automation)
echo   [9] CONFIGURATION and MASTER API KEYS       (Manage Reducto, Gemini, PyMuPDF Pro, Passphrases and Env)
echo.
echo   [X] EXIT ORCHESTRATOR
echo ==============================================================================================================
set /p choice="SYS_COMMAND_> "

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
