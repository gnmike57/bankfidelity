#!/usr/bin/env bash
# Install BankFidelity + PyMuPDF (and PyMuPDF Pro if available) on the GX10.
# Run ON the GX10 from the repo root after the working tree has been copied over.
#
#   PYMUPDF_PRO_KEY is never read from or written to disk by this script.
#   To enable Pro, export it in your shell first:  read -rs PYMUPDF_PRO_KEY; export PYMUPDF_PRO_KEY
#
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO"

echo "== host"
uname -srm
[ "$(uname -m)" = "aarch64" ] || { echo "expected aarch64, got $(uname -m)"; exit 1; }
nvidia-smi --query-gpu=name,driver_version --format=csv,noheader || echo "WARN: nvidia-smi unavailable"
free -g | sed -n 2p
df -h "$REPO" | tail -1

echo "== system packages"
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev libfontconfig1-dev \
    libfreetype6-dev python3 python3-venv python3-pip git curl ca-certificates

echo "== rust 1.89.0"
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
rustup toolchain install 1.89.0 --profile minimal -c rustfmt -c clippy

echo "== python venv"
python3 -m venv .venv
. .venv/bin/activate
pip install --upgrade pip
pip install -r requirements-ci.txt fonttools opencv-python-headless pyyaml

echo "== PyMuPDF Pro (aarch64 wheel availability is unverified)"
if pip install "PyMuPDFPro==1.28.2"; then
  echo "PyMuPDFPro installed"
else
  echo "WARN: PyMuPDFPro has no installable aarch64 build; Pro edit path stays UNAVAILABLE (Pdfium fallback applies)"
fi
if [ -n "${PYMUPDF_PRO_KEY:-}" ]; then echo "PYMUPDF_PRO_KEY is set (${#PYMUPDF_PRO_KEY} chars)"; else echo "PYMUPDF_PRO_KEY is missing"; fi
python - <<'PY'
import importlib.metadata as m
for n in ("PyMuPDF", "PyMuPDFPro"):
    try: print(n, m.version(n))
    except m.PackageNotFoundError: print(n, "NOT INSTALLED")
PY

echo "== build + test"
export PYTHON_EXECUTABLE="$REPO/.venv/bin/python" PDFIUM_AUTO_DOWNLOAD=true
export DUAL_CORE_PASSPHRASE="${DUAL_CORE_PASSPHRASE:-gx10-install-check-passphrase}"
cargo build --release --locked --bin dual-core-pdf-pipeline
cargo test --lib --locked
( cd python && python -m unittest test_specialist_bridge -v )

echo "== done. binary: $REPO/target/release/dual-core-pdf-pipeline"
