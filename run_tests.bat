@echo off
echo ========================================
echo Bank Statement Editor - Test Runner
echo ========================================
echo.

REM Set Python path for PyO3 dynamically
for /f "delims=" %%i in ('where python.exe 2^>nul') do (
    set "PYO3_PYTHON=%%i"
    goto :found_python
)
:found_python

REM Check if Python exists
if not exist "%PYO3_PYTHON%" (
    echo ERROR: Python not found at %PYO3_PYTHON%
    echo Please install Python or update the path in this script.
    pause
    exit /b 1
)

echo Python found: %PYO3_PYTHON%
echo.

REM Check if MinGW is installed (for GNU toolchain)
where dlltool.exe >nul 2>&1
if %errorlevel% neq 0 (
    echo WARNING: dlltool.exe not found in PATH
    echo The GNU toolchain requires MinGW-w64 to be installed.
    echo.
    echo Please install MinGW-w64 from: https://www.mingw-w64.org/
    echo   - Architecture: x86_64
    echo   - Threads: posix
    echo   - Exception: seh
    echo.
    echo After installation, add the bin directory to your PATH.
    echo Example: set PATH=C:\Program Files\mingw-w64\x86_64-14.2.0-posix-seh-msvcrt\mingw64\bin;%%PATH%%
    echo.
    pause
    exit /b 1
)

echo MinGW found in PATH
echo.

REM Navigate to project directory
cd /d "%~dp0"

echo Running cargo check...
cargo check
if %errorlevel% neq 0 (
    echo ERROR: cargo check failed
    pause
    exit /b 1
)

echo cargo check passed!
echo.

echo Running transfer module tests...
cargo test --lib -- transfer::tests
if %errorlevel% neq 0 (
    echo ERROR: Transfer tests failed
    pause
    exit /b 1
)

echo Transfer tests passed!
echo.

echo Running all library tests...
cargo test --lib
if %errorlevel% neq 0 (
    echo ERROR: Library tests failed
    pause
    exit /b 1
)

echo All library tests passed!
echo.

echo Running clippy...
cargo clippy --all-targets --all-features -- -D warnings
if %errorlevel% neq 0 (
    echo WARNING: Clippy found issues
)

echo.
echo ========================================
echo All checks completed!
echo ========================================
pause
