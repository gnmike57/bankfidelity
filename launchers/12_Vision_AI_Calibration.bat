@echo off
setlocal EnableDelayedExpansion
title BANKFIDELITY // VISION AI SUB-PIXEL CALIBRATION & CORRECTION
color 0D
chcp 65001 >nul

:: Set ESC character for ANSI colors
for /f %%a in ('echo prompt $E^| cmd') do set "ESC=%%a"

pushd "%~dp0.."
set "BF_DIR=%CD%"
popd

set "UFO_ROOT=C:\ufo\ufo"
set "PYTHON_EXE=%UFO_ROOT%\python_env\python.exe"
if not exist "%PYTHON_EXE%" (
    for /f "delims=" %%P in ('where python.exe 2^>nul') do set "PYTHON_EXE=%%P"
)

cls
echo !ESC![95m==============================================================================!ESC![0m
echo !ESC![95;1m              VISION AI SUB-PIXEL CALIBRATION & CORRECTION LOOP!ESC![0m
echo !ESC![95m==============================================================================!ESC![0m
echo !ESC![97mExecutes closed-loop visual verification across real bank statements:!ESC![0m
echo !ESC![90m  1. 300+ DPI High-Resolution Dual-Page Rasterization!ESC![0m
echo !ESC![90m  2. Structural & Perceptual Diffing (SSIM, PSNR, Pixel MSE)!ESC![0m
echo !ESC![90m  3. Optical Kerning & Bounding Box Sub-Pixel Calibration!ESC![0m
echo !ESC![90m  4. Closed-Loop Iterative Layout Correction until SSIM >= 0.998!ESC![0m
echo !ESC![90m  5. Heatmap Visual Artifact Generation in audit-evidence/vision-calibration/!ESC![0m
echo.
echo !ESC![95m==============================================================================!ESC![0m
echo.
pause

cd /d "%BF_DIR%"
"%PYTHON_EXE%" "%BF_DIR%\scripts\vision_ai_calibration.py"

echo.
echo !ESC![92m==============================================================================!ESC![0m
echo !ESC![92mCalibration & Verification Loop Complete. Evidence saved to audit-evidence/!ESC![0m
echo !ESC![92m==============================================================================!ESC![0m
pause
