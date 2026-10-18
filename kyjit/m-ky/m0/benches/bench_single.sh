#!/usr/bin/env bash
# Measure per-loop latency using M0_TIMING (in-process timing)
set -u

LOOPS=tests/real_loops
DECIDE=./target/release/m0-decide

echo "=== Single-loop in-process timing (M0_TIMING) ==="
echo ""

# Warm up
"$DECIDE" "$(ls $LOOPS/*.json | head -1)" > /dev/null 2>&1

for f in r001.json r002.json r020.json r051.json r076.json r613.json; do
    if [ ! -f "$LOOPS/$f" ]; then
        echo "  (missing $f)"
        continue
    fi
    printf "%-15s " "$f"
    M0_TIMING=1 "$DECIDE" "$LOOPS/$f" 2>&1 | grep "timing:" | sed 's/timing: //'
done

echo ""
echo "=== Explain the timing ==="
echo "  read    — JSON file I/O"
echo "  parse   — serde_json deserialization"
echo "  validate— loop well-formedness"
echo "  total   — full wall-clock (includes decide())"
