#!/usr/bin/env bash
# Run M0 on 100 real loops. Print pass/fail table and coverage summary.
set -u

# Resolve everything against the repo root, never the invocation CWD, and run
# the decider inside a per-run temp dir: `m0-decide` writes `cert.json` into
# its working directory, so two suites launched together (or a leftover from an
# aborted run) otherwise check one loop's certificate against another loop and
# report bogus REJECTs. `tests/fuzz.sh` uses the same pattern for the same
# reason; this is the discipline the Phase 4 report reproduces through.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DECIDE="$ROOT/target/release/m0-decide"
CHECK="$ROOT/target/release/m0-check"
LOOPS="$ROOT/tests/real_loops"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

exact=0
none=0
unknown=0
check_pass=0
check_fail=0
crashes=0

printf "%-10s %-10s %-8s %s\n" "LOOP" "VERDICT" "CHECK" "DISTANCE"
printf "%-10s %-10s %-8s %s\n" "----" "-------" "-----" "--------"

for f in "$LOOPS"/*.json; do
    name=$(basename "$f" .json)

    # Clear before deciding: the decider writes cert.json only when it has a
    # certificate, so a leftover from an earlier aborted run would be checked
    # against the wrong loop.
    rm -f "$TMP/cert.json"
    out=$( (cd "$TMP" && "$DECIDE" "$f") 2>&1)
    rc=$?

    if [ $rc -ne 0 ]; then
        crashes=$((crashes+1))
        printf "%-10s %-10s %-8s %s\n" "$name" "CRASH" "-" "-"
        continue
    fi

    overall=$(echo "$out" | grep -oP 'overall: \K\w+' || echo "ERROR")
    dist=$(echo "$out" | grep -oP 'EXACT distance \K\[[^]]*\]' | head -1 || echo "-")

    chk="-"
    if [ -f "$TMP/cert.json" ]; then
        chkout=$("$CHECK" "$f" "$TMP/cert.json" 2>&1)
        chk=$(echo "$chkout" | head -1 | grep -oE '^(ACCEPT|REJECT)' || echo "PANIC")
        rm -f "$TMP/cert.json"
        if [ "$chk" = "ACCEPT" ]; then check_pass=$((check_pass+1)); fi
        if [ "$chk" = "REJECT" ] || [ "$chk" = "PANIC" ]; then check_fail=$((check_fail+1)); fi
    fi

    case "$overall" in
        EXACT) exact=$((exact+1));;
        NONE) none=$((none+1));;
        UNKNOWN) unknown=$((unknown+1));;
    esac

    printf "%-10s %-10s %-8s %s\n" "$name" "$overall" "$chk" "$dist"
done

total=$(ls "$LOOPS"/*.json | wc -l)
echo ""
echo "=== SUMMARY ==="
echo "Total loops:        $total"
echo "EXACT:              $exact"
echo "NONE:               $none"
echo "UNKNOWN:            $unknown"
echo "Checker ACCEPT:     $check_pass"
echo "Checker REJECT:     $check_fail"
echo "Crashes:            $crashes"
echo ""
coverage=$(awk "BEGIN { printf \"%.1f\", 100.0 * ($exact + $none) / $total }")
echo "Coverage (EXACT+NONE / total): ${coverage}%"
