#!/usr/bin/env bash
# launch.sh — Bank Statement Fidelity Editor launcher for Bash/MSYS2/WSL
# Starts the GUI with required PyO3 and Python runtime environment.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Resolve Python environment
PYTHON_EXE=""
if [ -f "/c/ufo/ufo/python_env/python.exe" ]; then
    PYTHON_EXE="/c/ufo/ufo/python_env/python.exe"
    PYTHON_DIR="/c/ufo/ufo/python_env"
elif [ -f "C:/ufo/ufo/python_env/python.exe" ]; then
    PYTHON_EXE="C:/ufo/ufo/python_env/python.exe"
    PYTHON_DIR="C:/ufo/ufo/python_env"
elif command -v python3 &>/dev/null; then
    PYTHON_EXE="$(command -v python3)"
    PYTHON_DIR="$(dirname "$PYTHON_EXE")"
elif command -v python &>/dev/null; then
    PYTHON_EXE="$(command -v python)"
    PYTHON_DIR="$(dirname "$PYTHON_EXE")"
fi

if [ -n "$PYTHON_DIR" ]; then
    export PATH="$PYTHON_DIR:$PYTHON_DIR/Scripts:/c/msys64/mingw64/bin:$HOME/.cargo/bin:$PATH"
    export PYO3_PYTHON="$PYTHON_EXE"
    export PYTHON_SYS_EXECUTABLE="$PYTHON_EXE"
    export PYTHONHOME="$PYTHON_DIR"
    # Python paths: try Linux-style first, then Windows-style
    PYTHON_LIB=""
    for pyver in $(ls -d "$PYTHON_DIR/lib"* 2>/dev/null | head -5); do
        if [ -d "$pyver/site-packages" ]; then
            PYTHON_LIB="$pyver"
            break
        fi
    done
    if [ -z "$PYTHON_LIB" ]; then
        PYTHON_LIB="$PYTHON_DIR/lib"
    fi
    export PYTHONPATH="$PYTHON_LIB/site-packages:$PYTHON_LIB:$PYTHON_DIR/lib/python3/site-packages:$PYTHON_DIR/Lib/site-packages:$PYTHON_DIR/Lib:$PYTHON_DIR/DLLs"
fi

# Export .env
if [ -f "$SCRIPT_DIR/.env" ]; then
    set -a
    source "$SCRIPT_DIR/.env" 2>/dev/null || true
    set +a
fi

# Locate binary (Windows + Linux targets)
BIN=""
if [ -f "$SCRIPT_DIR/target/x86_64-pc-windows-gnu/release/dual-core-pdf-pipeline.exe" ]; then
    BIN="$SCRIPT_DIR/target/x86_64-pc-windows-gnu/release/dual-core-pdf-pipeline.exe"
elif [ -f "$SCRIPT_DIR/target/release/dual-core-pdf-pipeline.exe" ]; then
    BIN="$SCRIPT_DIR/target/release/dual-core-pdf-pipeline.exe"
elif [ -f "$SCRIPT_DIR/target/x86_64-pc-windows-gnu/debug/dual-core-pdf-pipeline.exe" ]; then
    BIN="$SCRIPT_DIR/target/x86_64-pc-windows-gnu/debug/dual-core-pdf-pipeline.exe"
elif [ -f "$SCRIPT_DIR/target/debug/dual-core-pdf-pipeline.exe" ]; then
    BIN="$SCRIPT_DIR/target/debug/dual-core-pdf-pipeline.exe"
elif [ -f "$SCRIPT_DIR/BankFidelity_Stable.exe" ]; then
    BIN="$SCRIPT_DIR/BankFidelity_Stable.exe"
# Linux binary targets (for Linux migration / WSL2 / Docker-less builds)
elif [ -f "$SCRIPT_DIR/target/release/dual-core-pdf-pipeline" ]; then
    BIN="$SCRIPT_DIR/target/release/dual-core-pdf-pipeline"
elif [ -f "$SCRIPT_DIR/target/debug/dual-core-pdf-pipeline" ]; then
    BIN="$SCRIPT_DIR/target/debug/dual-core-pdf-pipeline"
elif [ -f "$SCRIPT_DIR/target/x86_64-unknown-linux-gnu/release/dual-core-pdf-pipeline" ]; then
    BIN="$SCRIPT_DIR/target/x86_64-unknown-linux-gnu/release/dual-core-pdf-pipeline"
elif [ -f "$SCRIPT_DIR/target/x86_64-unknown-linux-gnu/debug/dual-core-pdf-pipeline" ]; then
    BIN="$SCRIPT_DIR/target/x86_64-unknown-linux-gnu/debug/dual-core-pdf-pipeline"
fi

if [ -z "$BIN" ]; then
    echo "[ERROR] No dual-core-pdf-pipeline binary found. Run 'cargo build --release' first."
    exit 1
fi

echo "[launch] Starting Bank Statement Fidelity Editor GUI..."
exec "$BIN" gui "$@"
