#!/usr/bin/env bash
# Measure batch throughput for various thread counts
set -u

PACK=tests/loops.pack
PACKED=./target/release/m0-packed

if [ ! -f "$PACK" ]; then
    echo "No $PACK — run: ./target/release/m0-pack tests/real_loops_bin $PACK"
    exit 1
fi

echo "=== Batch throughput — 5 runs per thread count, median ==="
echo ""
printf "%-10s %-15s %-15s %s\n" "THREADS" "TOTAL" "PER LOOP" "NOTE"
printf "%-10s %-15s %-15s %s\n" "-------" "-----" "--------" "----"

for n in 1 2 4 6 8 12 16; do
    times=()
    for i in 1 2 3 4 5; do
        out=$($PACKED --threads=$n "$PACK" 2>/dev/null | grep "Per loop:")
        ns=$(echo "$out" | awk '{print $3}')
        times+=("$ns")
    done
    median=$(printf '%s\n' "${times[@]}" | sort -n | awk 'NR==3 {print $1}')
    total=$($PACKED --threads=$n "$PACK" 2>/dev/null | grep "Total:" | awk '{print $2}')
    printf "%-10s %-15s %-15s %s\n" "$n" "$total" "${median}ns" ""
done

echo ""
echo "=== Single-thread serial cost ==="
$PACKED --threads=1 "$PACK" | tail -3
