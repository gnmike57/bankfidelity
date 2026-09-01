@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // CROSS-BANK TRANSFER MATRIX STRESS TEST
color 0B
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

pushd "%~dp0.."
set "BF_DIR=%CD%"
popd

cls
echo !ESC![95m==============================================================================!ESC![0m
echo !ESC![95;1m                  CROSS-BANK TRANSFER MATRIX STRESS TEST!ESC![0m
echo !ESC![95m==============================================================================!ESC![0m
echo !ESC![97mThis test executes pairwise statement transfers across all Australian bank formats:!ESC![0m
echo !ESC![90m  • CommBank (SmartAccess)!ESC![0m
echo !ESC![90m  • Bankwest!ESC![0m
echo !ESC![90m  • Westpac (Choice)!ESC![0m
echo !ESC![90m  • Macquarie!ESC![0m
echo !ESC![90m  • ING (Orange)!ESC![0m
echo !ESC![90m  • ANZ Plus!ESC![0m
echo.
echo !ESC![93mUtilizes Reducto / Gemini AI layout-agnostic parsing + PyMuPDF Pro / Typst engines.!ESC![0m
echo !ESC![90mAudit logs and transformed output PDFs will be written to audit/transfer_tests/!ESC![0m
echo !ESC![95m==============================================================================!ESC![0m
echo.
pause

cd /d "%BF_DIR%"
cargo test --test au_transfer_stress -- --nocapture --ignored

echo.
echo !ESC![92m==============================================================================!ESC![0m
echo !ESC![92mMatrix Stress Test Execution Complete. Check audit/transfer_tests/ for JSON reports.!ESC![0m
echo !ESC![92m==============================================================================!ESC![0m
pause
