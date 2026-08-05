#!/bin/bash

# E2E Non-CI Smoke Harness (v0.5.0)
# Runs the full extraction -> balance -> verify -> export pipeline.
#
# Usage: ./tests/smoke/run_smoke.sh <sample.pdf> [--keep-output]

set -e

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <sample.pdf> [--keep-output]"
    exit 1
fi

SAMPLE_PDF="$1"
KEEP_OUTPUT=0
if [ "$2" == "--keep-output" ]; then
    KEEP_OUTPUT=1
fi

if [ ! -f "$SAMPLE_PDF" ]; then
    echo "Error: File $SAMPLE_PDF not found."
    exit 1
fi

TMP_DIR=$(mktemp -d)
OUT_PDF="$TMP_DIR/balanced.pdf"
JSON_OUT="$TMP_DIR/extracted.json"
VERIFY_DIR="$TMP_DIR/verify_report"

echo "============================================================"
echo " Starting dual-core-pdf-pipeline v0.5.0 Smoke Test"
echo "============================================================"
echo "[1/4] Extracting semantic data and geometry..."
cargo run --release --features dev -- extract --input "$SAMPLE_PDF" --output "$JSON_OUT"
if [ ! -s "$JSON_OUT" ]; then
    echo "❌ Extract failed or produced empty JSON."
    exit 1
fi

echo "[2/4] Running Smart Balance Engine (Document AI + Gemini)..."
cargo run --release --features dev -- balance --input "$SAMPLE_PDF" --output "$OUT_PDF" --auto-approve

echo "[3/4] Verifying visual fidelity and mathematical integrity..."
cargo run --release --features dev -- verify --original "$SAMPLE_PDF" --edited "$OUT_PDF" --output-dir "$VERIFY_DIR"

echo "[4/4] Exporting Change History..."
# We just use the latest audit log
LATEST_LOG=$(ls -t audit/*.log | head -n 1)
HISTORY_JSON="$TMP_DIR/history.json"
cargo run --release --features dev -- export-history --from-log "$LATEST_LOG" --output "$HISTORY_JSON"

if [ ! -s "$HISTORY_JSON" ]; then
    echo "❌ Export History failed or produced empty JSON."
    exit 1
fi

echo "============================================================"
echo " ✅ Smoke Test Completed Successfully."
echo "============================================================"

if [ $KEEP_OUTPUT -eq 1 ]; then
    echo "Outputs retained in: $TMP_DIR"
else
    rm -rf "$TMP_DIR"
    echo "Temporary outputs cleaned up."
fi
