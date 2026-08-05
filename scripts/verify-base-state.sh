#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
export PYO3_PYTHON="${PYO3_PYTHON:-$(command -v "$PYTHON_BIN")}"
export DUAL_CORE_PASSPHRASE="${DUAL_CORE_PASSPHRASE:-base-state-verification-only}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

echo "[1/8] rustfmt"
cargo fmt --all -- --check

echo "[2/8] Python production bridge"
"$PYTHON_BIN" python/smoke_test.py

echo "[3/8] all host targets"
cargo check --locked --all-targets --message-format=short

echo "[4/8] production lint"
cargo clippy --locked --lib --bins -- -D warnings

echo "[5/8] library tests"
cargo test --locked --lib --no-fail-fast

echo "[6/8] portable runtime smoke"
cargo test --locked --test runtime_smoke

echo "[7/8] configuration-free startup contract"
cargo test --locked --test cli_startup_contract

echo "[8/8] production executable"
cargo build --locked --bin dual-core-pdf-pipeline
BIN="target/debug/dual-core-pdf-pipeline"
"$BIN" --version
"$BIN" --help >/dev/null

if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
  echo "verification left the working tree dirty:" >&2
  git status --short >&2
  exit 1
fi

echo "BASE STATE: PASS"
