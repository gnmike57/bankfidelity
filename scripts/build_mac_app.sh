#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION=$(grep -E '^version\s*=' Cargo.toml | sed -E 's/version[[:space:]]*=[[:space:]]*"([^"]+)"/\1/')
TARGET="aarch64-apple-darwin"
REVISION="${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || printf unknown)}"
OUTPUT="${1:-target/release/portable/macos-aarch64}"

rustup target add "$TARGET"
cargo build --locked --release --target "$TARGET" --bin dual-core-pdf-pipeline
python3 scripts/build_portable_bundle.py \
  --platform macos-aarch64 \
  --binary "target/$TARGET/release/dual-core-pdf-pipeline" \
  --output "$OUTPUT" \
  --revision "$REVISION" \
  --version "$VERSION"

mkdir -p target/release/artifacts
cp "$OUTPUT/BankStatementFidelityEditor.app/Contents/MacOS/libpdfium.dylib" "$OUTPUT/BankStatementFidelityEditor.app/Contents/Resources/"
tar -C "$OUTPUT" -czf "target/release/artifacts/BankStatementFidelityEditor-${VERSION}-macos-aarch64.tar.gz" BankStatementFidelityEditor.app
ARCHIVE="target/release/artifacts/BankStatementFidelityEditor-${VERSION}-macos-aarch64.tar.gz"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$ARCHIVE" > "$ARCHIVE.sha256"
else
  shasum -a 256 "$ARCHIVE" > "$ARCHIVE.sha256"
fi
printf 'Created unsigned portable bundle: %s\n' "target/release/artifacts/BankStatementFidelityEditor-${VERSION}-macos-aarch64.tar.gz"
