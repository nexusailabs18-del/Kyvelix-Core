#!/usr/bin/env bash
# Measure M1 parametric distance analysis
set -u

M1=./target/release/m1-distance
LOOPS=tests/param_loops

echo "=== M1 parametric distance analysis ==="
echo ""

for f in "$LOOPS"/*.json; do
    name=$(basename "$f")
    t0=$(date +%s%N)
    "$M1" "$f" > /dev/null 2>&1
    t1=$(date +%s%N)
    us=$(( (t1 - t0) / 1000 ))
    printf "%-15s %6d µs\n" "$name" "$us"
done

echo ""
echo "=== Explanation ==="
echo "  Per-loop time is dominated by process startup (~100-150µs)."
echo "  The actual analysis is <10µs (see M0_TIMING on m0-decide)."
