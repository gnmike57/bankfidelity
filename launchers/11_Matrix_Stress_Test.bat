@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // CROSS-BANK TRANSFER MATRIX STRESS TEST
color 0B
chcp 65001 >nul

:: Set ESC character for ANSI colors

pushd "%~dp0.."
set "BF_DIR=%CD%"
popd

cls
echo ==============================================================================
echo                   CROSS-BANK TRANSFER MATRIX STRESS TEST
echo ==============================================================================
echo This test executes pairwise statement transfers across all Australian bank formats:
echo   • CommBank (SmartAccess)
echo   • Bankwest
echo   • Westpac (Choice)
echo   • Macquarie
echo   • ING (Orange)
echo   • ANZ Plus
echo.
echo Utilizes Reducto / Gemini AI layout-agnostic parsing + PyMuPDF Pro / Typst engines.
echo Audit logs and transformed output PDFs will be written to audit/transfer_tests/
echo ==============================================================================
echo.
pause

cd /d "%BF_DIR%"
cargo test --test au_transfer_stress -- --nocapture --ignored

echo.
echo ==============================================================================
echo Matrix Stress Test Execution Complete. Check audit/transfer_tests/ for JSON reports.
echo ==============================================================================
pause
