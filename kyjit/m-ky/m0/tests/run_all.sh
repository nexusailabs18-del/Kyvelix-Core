#!/usr/bin/env bash
# Run m0-decide + m0-check on every loop in tests/loops/
set -u

# Paths resolve against the repo root, not the invocation CWD, and the decider
# runs in a per-run temp dir because it writes `cert.json` into its working
# directory — see the header of `run_real.sh` for why that matters.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DECIDE="$ROOT/target/release/m0-decide"
CHECK="$ROOT/target/release/m0-check"
LOOPS="$ROOT/tests/loops"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0
unknown=0

printf "%-30s %-12s %-10s %s\n" "LOOP" "OVERALL" "CHECKER" "DISTANCE"
printf "%-30s %-12s %-10s %s\n" "----" "-------" "-------" "--------"

for f in "$LOOPS"/*.json; do
    name=$(basename "$f")
    # Clear before deciding: the decider writes cert.json only when it has a
    # certificate, so a leftover from an earlier aborted run would be checked
    # against the wrong loop.
    rm -f "$TMP/cert.json"
    out=$( (cd "$TMP" && "$DECIDE" "$f") 2>&1)
    overall=$(echo "$out" | grep -oP 'overall: \K\w+' || echo "ERROR")
    dist=$(echo "$out" | grep -oP 'EXACT distance \K\[[^]]*\]' | head -1 || echo "-")

    if [ -f "$TMP/cert.json" ]; then
        chk=$( (cd "$TMP" && "$CHECK" "$f" "$TMP/cert.json") 2>&1 | tail -1)
        rm -f "$TMP/cert.json"
    else
        chk="-"
    fi

    printf "%-30s %-12s %-10s %s\n" "$name" "$overall" "$chk" "$dist"

    case "$chk" in
        ACCEPT) pass=$((pass+1));;
        -)      ;;
        *)      fail=$((fail+1));;
    esac
    if [ "$overall" = "UNKNOWN" ]; then
        unknown=$((unknown+1))
    fi
done

echo ""
echo "Total loops: $(ls "$LOOPS"/*.json | wc -l)"
echo "Checker ACCEPT: $pass"
echo "Checker REJECT: $fail"
echo "Overall UNKNOWN: $unknown"
