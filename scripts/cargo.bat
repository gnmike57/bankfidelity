@echo off
setlocal

where cargo >nul 2>nul
if errorlevel 1 (
  echo Cargo was not found on PATH. Install Rust 1.89.0 with rustup and retry. 1>&2
  exit /b 1
)

cargo %*
exit /b %errorlevel%
