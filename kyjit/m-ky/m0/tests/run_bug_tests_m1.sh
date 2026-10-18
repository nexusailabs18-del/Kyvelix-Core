#!/usr/bin/env bash
set -u

M1=./target/release/m1-distance
BUGS=tests/bug_tests_m1

panics=0
clean=0

printf "%-40s %-15s %s\n" "TEST" "RESULT" "NOTE"
printf "%-40s %-15s %s\n" "----" "------" "----"

for f in "$BUGS"/*.json; do
    name=$(basename "$f" .json)
    out=$(timeout 3 "$M1" "$f" 2>&1)
    rc=$?

    if [ $rc -eq 124 ]; then
        printf "%-40s %-15s %s\n" "$name" "TIMEOUT" "hang"
        panics=$((panics+1))
    elif [ $rc -ne 0 ]; then
        # Some tests are expected to fail (e.g., missing env)
        if echo "$out" | grep -q "panicked"; then
            printf "%-40s %-15s %s\n" "$name" "PANIC" "$(echo "$out" | grep -oP 'panicked.*' | cut -c1-30)"
            panics=$((panics+1))
        else
            printf "%-40s %-15s %s\n" "$name" "EXIT-$rc" "clean error"
            clean=$((clean+1))
        fi
    else
        result=$(echo "$out" | grep -oP '(CONST|FUNCTION|NONE|UNKNOWN)[^;]*' | head -1)
        printf "%-40s %-15s %s\n" "$name" "OK" "$result"
        clean=$((clean+1))
    fi
done

echo ""
echo "=== SUMMARY ==="
echo "Clean:  $clean"
echo "Panics: $panics"
echo ""

if [ "$panics" -eq 0 ]; then
    echo "RESULT: M1 handles all edge cases without panic."
    exit 0
else
    echo "RESULT: M1 has panics."
    exit 1
fi
