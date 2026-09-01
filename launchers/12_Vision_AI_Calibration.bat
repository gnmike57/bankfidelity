@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // VISION AI SUB-PIXEL CALIBRATION & CORRECTION
color 0D
chcp 65001 >nul

:: Set ESC character for ANSI colors

pushd "%~dp0.."
set "BF_DIR=%CD%"
popd

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
if not exist "%PYTHON_EXE%" (
    for /f "delims=" %%P in ('where python.exe 2^>nul') do set "PYTHON_EXE=%%P"
)

cls
echo ==============================================================================
echo               VISION AI SUB-PIXEL CALIBRATION and CORRECTION LOOP
echo ==============================================================================
echo Executes closed-loop visual verification across real bank statements:
echo   1. 300+ DPI High-Resolution Dual-Page Rasterization
echo   2. Structural and Perceptual Diffing (SSIM, PSNR, Pixel MSE)
echo   3. Optical Kerning and Bounding Box Sub-Pixel Calibration
echo   4. Closed-Loop Iterative Layout Correction until SSIM >= 0.998
echo   5. Heatmap Visual Artifact Generation in audit-evidence/vision-calibration/
echo.
echo ==============================================================================
echo.
pause

cd /d "%BF_DIR%"
"%PYTHON_EXE%" "%BF_DIR%\scripts\vision_ai_calibration.py"

echo.
echo ==============================================================================
echo Calibration and Verification Loop Complete. Evidence saved to audit-evidence/
echo ==============================================================================
pause
