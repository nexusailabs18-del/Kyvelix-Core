#!/usr/bin/env bash
# Measure per-loop wall-clock time for m0-decide.
set -u

DECIDE=./target/release/m0-decide
LOOPS=tests/real_loops

if [ ! -d "$LOOPS" ]; then
    echo "No real_loops directory. Run the 1K generation first."
    exit 1
fi

echo "Timing $(ls $LOOPS/*.json | wc -l) loops..."
echo ""

# Warmup
"$DECIDE" "$(ls $LOOPS/*.json | head -1)" > /dev/null 2>&1

# Time all loops
start=$(date +%s%N)
count=0
for f in "$LOOPS"/*.json; do
    "$DECIDE" "$f" > /dev/null 2>&1
    count=$((count+1))
done
end=$(date +%s%N)

total_ns=$((end - start))
total_us=$((total_ns / 1000))
per_loop_ns=$((total_ns / count))
per_loop_us=$((per_loop_ns / 1000))

echo "Total loops:         $count"
echo "Total time:          $total_us µs"
echo "Per loop:            $per_loop_ns ns ($per_loop_us µs)"
echo ""

if [ "$per_loop_ns" -lt 100000 ]; then
    echo "RESULT: PASS (<100µs per loop)"
    exit 0
else
    echo "RESULT: FAIL (>=100µs per loop)"
    exit 1
fi
