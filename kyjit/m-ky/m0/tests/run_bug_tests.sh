#!/usr/bin/env bash
# Bug test suite: run M0 on edge cases, verify no panics, clean verdicts.
set -u

DECIDE=./target/release/m0-decide
CHECK=./target/release/m0-check
BUGS=tests/bug_tests

crashes=0
accepts=0
rejects=0
unknown=0
errors=0

printf "%-35s %-12s %-10s %s\n" "TEST" "VERDICT" "CHECKER" "NOTE"
printf "%-35s %-12s %-10s %s\n" "----" "-------" "-------" "----"

for f in "$BUGS"/*.json; do
    name=$(basename "$f" .json)
    rm -f cert.json

    # Run decider with timeout
    out=$(timeout 3 "$DECIDE" "$f" 2>&1)
    rc=$?

    note=""
    verdict="-"
    chk="-"

    if [ $rc -eq 124 ]; then
        verdict="TIMEOUT"
        crashes=$((crashes+1))
        note="hang"
    elif [ $rc -eq 2 ]; then
        verdict="INVALID"
        note=$(echo "$out" | head -1 | cut -c1-40)
    elif [ $rc -ne 0 ]; then
        if echo "$out" | grep -q "panicked"; then
            verdict="PANIC"
            crashes=$((crashes+1))
            note=$(echo "$out" | grep -oP 'panicked.*' | head -1 | cut -c1-40)
        else
            verdict="ERROR"
            note=$(echo "$out" | head -1 | cut -c1-40)
        fi
    else
        verdict=$(echo "$out" | grep -oP 'overall: \K\w+' || echo "NONE")
        case "$verdict" in
            EXACT) accepts=$((accepts+1));;
            NONE) rejects=$((rejects+1));;
            UNKNOWN) unknown=$((unknown+1));;
        esac

        # If cert was written, check it
        if [ -f cert.json ]; then
            chkout=$(timeout 3 "$CHECK" "$f" cert.json 2>&1)
            chk=$(echo "$chkout" | head -1 | grep -oE '^(ACCEPT|REJECT)' || echo "PANIC")
            if [ "$chk" = "PANIC" ] || [ "$chk" = "REJECT" ]; then
                errors=$((errors+1))
                note="checker $chk"
            fi
            rm -f cert.json
        fi
    fi

    printf "%-35s %-12s %-10s %s\n" "$name" "$verdict" "$chk" "$note"
done

total=$(ls "$BUGS"/*.json | wc -l)
echo ""
echo "=== SUMMARY ==="
echo "Total tests:   $total"
echo "EXACT:         $accepts"
echo "NONE:          $rejects"
echo "UNKNOWN:       $unknown"
echo "Panics:        $crashes"
echo "Checker errors:$errors"
echo ""

if [ "$crashes" -eq 0 ] && [ "$errors" -eq 0 ]; then
    echo "RESULT: no panics, no checker errors. M0 handles all edge cases cleanly."
    exit 0
else
    echo "RESULT: bugs found. See above."
    exit 1
fi
