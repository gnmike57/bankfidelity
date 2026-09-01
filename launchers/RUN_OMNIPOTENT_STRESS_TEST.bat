@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // OMNIPOTENT 1000% STRESS TEST GAUNTLET
color 0B
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

pushd "C:\bankfidelity\bankfidelity"

cls
echo !ESC![95m==================================================================================!ESC![0m
echo !ESC![95;1m              BANKFIDELITY OMNIPOTENT 1000%% STRESS TEST GAUNTLET!ESC![0m
echo !ESC![97m                   36-Pair Permutation Cross-Bank Transfer Matrix!ESC![0m
echo !ESC![95m==================================================================================!ESC![0m
echo.
echo !ESC![97mThis test executes the full 36-combination cross-bank transfer matrix across:!ESC![0m
echo !ESC![90m  • Commonwealth Bank (SmartAccess)!ESC![0m
echo !ESC![90m  • Bankwest (Classic Qantas)!ESC![0m
echo !ESC![90m  • ING (Orange Everyday)!ESC![0m
echo !ESC![90m  • Macquarie Bank (Transaction)!ESC![0m
echo !ESC![90m  • Westpac (Choice Basic)!ESC![0m
echo !ESC![90m  • ANZ Plus (Everyday)!ESC![0m
echo.
echo !ESC![93mKey Engine Features:!ESC![0m
echo !ESC![90m  1. Real Cloud API Calls (Reducto, Gemini, PyMuPDF Pro, Typst, Document AI)!ESC![0m
echo !ESC![90m  2. 300+ DPI Dual-Page Rasterization & Pure-NumPy SSIM / PSNR / MSE Heatmaps!ESC![0m
echo !ESC![90m  3. 100%% Mathematical Running Balance Reconciliation!ESC![0m
echo !ESC![90m  4. Continuous Self-Healing (Continues past any edge anomaly without halting)!ESC![0m
echo !ESC![90m  5. Dual Delivery: Generates OMNIPOTENT_STRESS_TEST_REPORT.md on your Desktop!ESC![0m
echo.
echo !ESC![92mPress ANY KEY to begin the full 1000%% automated stress test run...!ESC![0m
echo !ESC![95m==================================================================================!ESC![0m
pause

python scripts\omnipotent_stress_test.py

echo.
echo !ESC![92m==================================================================================!ESC![0m
echo !ESC![92mStress Test Gauntlet Complete! Review OMNIPOTENT_STRESS_TEST_REPORT.md on Desktop.!ESC![0m
echo !ESC![92m==================================================================================!ESC![0m
pause
