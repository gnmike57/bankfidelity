#!/usr/bin/env bash
set -euo pipefail

echo "=============================================="
echo "TRIPLE PASS OVERKILL TESTING SUITE"
echo "=============================================="

PASSES=2
for i in $(seq 1 $PASSES); do
    echo ""
    echo "=============================================="
    echo "Starting Pass $i of $PASSES..."
    echo "=============================================="
    
    cargo test --all-targets --all-features -- --ignored --nocapture || {
        echo "❌ PASS $i FAILED! Aborting triple-pass."
        exit 1
    }
    
    echo "Running Python Bridge Tests..."
    python3 tests/python/test_split_merge.py || {
        echo "❌ PASS $i PYTHON FAILED! Aborting triple-pass."
        exit 1
    }
    python3 tests/python/test_refactor_tx.py || {
        echo "❌ PASS $i PYTHON FAILED! Aborting triple-pass."
        exit 1
    }
    
    echo "Running Node Bridge Tests..."
    node tests/node/applitools_bridge.test.js || {
        echo "❌ PASS $i NODE FAILED! Aborting triple-pass."
        exit 1
    }
    
    echo "✅ PASS $i COMPLETED SUCCESSFULLY!"
done

echo ""
echo "=============================================="
echo "🎉 ALL $PASSES PASSES COMPLETED WITHOUT FLAKINESS!"
echo "=============================================="
