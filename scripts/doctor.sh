#!/usr/bin/env bash
set -euo pipefail

echo "== Project Doctor =="

if [ ! -f "Cargo.toml" ]; then
  echo "ERROR: Cargo.toml not found. Run from the project root."
  exit 1
fi

cat > rust-toolchain.toml <<'TOOLCHAIN'
[toolchain]
channel = "1.88.0"
components = ["rustfmt", "clippy"]
TOOLCHAIN

if command -v rustup >/dev/null 2>&1; then
  rustup install 1.88.0
  rustup override unset || true
else
  echo "WARNING: rustup not found. Install from https://rustup.rs/"
fi

command -v rustc >/dev/null 2>&1 && rustc --version
command -v cargo >/dev/null 2>&1 && cargo --version

if command -v cargo >/dev/null 2>&1; then
  cargo check
else
  echo "WARNING: cargo not found. Skipping cargo check."
fi

echo "Project doctor completed."
