#!/usr/bin/env bash
# Fuzz M0: generate random loops, verify no crashes and no unsound decisions.
# Usage: ./tests/fuzz.sh [N] [--keep]
#   N       number of iterations (default 10000)
#   --keep  keep artifacts even on success
set -u

# Resolve everything against the repo root, never the invocation CWD: the
# decider drops `cert.json` wherever it is run, and a relative path there is
# how a stale certificate from a previous run got picked up and checked against
# an unrelated loop.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DECIDE="$ROOT/target/release/m0-decide"
CHECK="$ROOT/target/release/m0-check"
GEN="$ROOT/tests/fuzz_gen.py"
KEEP=0
N=10000
for arg in "$@"; do
    case "$arg" in
        --keep) KEEP=1 ;;
        *) N="$arg" ;;
    esac
done

ART="$ROOT/fuzz_artifacts/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$ART"

TMP=$(mktemp -d)
crashes=0
check_fails=0
unsound=0
timeouts=0
accepted=0
none=0
unknown=0
skipped=0

cleanup() {
    if [ "$KEEP" -eq 0 ] && [ "$crashes" -eq 0 ] && [ "$check_fails" -eq 0 ] && [ "$unsound" -eq 0 ] && [ "$timeouts" -eq 0 ]; then
        rm -rf "$ART"
        echo "(artifacts cleaned — all clean)"
    else
        echo "Artifacts: $ART"
    fi
    rm -rf "$TMP"
}
trap cleanup EXIT

printf "Fuzzing %d loops...\n" "$N"
printf "Artifacts dir: %s\n\n" "$ART"

for i in $(seq 1 "$N"); do
    python3 "$GEN" "$i" > "$TMP/loop.json"

    # The decider writes cert.json into its working directory, so it must run
    # inside the per-iteration temp dir; reading it from the invocation CWD
    # picks up a stale file from an unrelated loop (or a concurrent run).
    rm -f "$TMP/cert.json"
    ( cd "$TMP" && timeout 2 "$DECIDE" loop.json ) > "$TMP/decide.txt" 2>&1
    rc=$?

    if [ "$rc" -eq 124 ]; then
        timeouts=$((timeouts+1))
        cp "$TMP/loop.json" "$ART/timeout_${i}.json"
        cp "$TMP/decide.txt" "$ART/timeout_${i}.txt"
        continue
    fi
    if [ "$rc" -ne 0 ]; then
        crashes=$((crashes+1))
        cp "$TMP/loop.json" "$ART/crash_${i}.json"
        cp "$TMP/decide.txt" "$ART/crash_${i}.txt"
        continue
    fi

    # --- Classify the result ---
    if grep -q "^overall: EXACT" "$TMP/decide.txt"; then
        accepted=$((accepted+1))
    elif grep -q "^overall: NONE" "$TMP/decide.txt"; then
        none=$((none+1))
    elif grep -q "^overall: UNKNOWN" "$TMP/decide.txt"; then
        unknown=$((unknown+1))
    fi

    # --- If a certificate was written, run the checker ---
    if [ -f "$TMP/cert.json" ]; then

        timeout 2 "$CHECK" "$TMP/loop.json" "$TMP/cert.json" > "$TMP/check.txt" 2>&1
        crc=$?

        if [ "$crc" -eq 124 ]; then
            timeouts=$((timeouts+1))
            cp "$TMP/loop.json" "$ART/check_timeout_${i}.json"
            cp "$TMP/cert.json" "$ART/check_timeout_${i}.cert.json"
        elif [ "$crc" -ne 0 ]; then
            check_fails=$((check_fails+1))
            cp "$TMP/loop.json" "$ART/check_fail_${i}.json"
            cp "$TMP/cert.json" "$ART/check_fail_${i}.cert.json"
            cp "$TMP/check.txt" "$ART/check_fail_${i}.txt"
        else
            if ! grep -q "^ACCEPT" "$TMP/check.txt"; then
                unsound=$((unsound+1))
                cp "$TMP/loop.json" "$ART/unsound_${i}.json"
                cp "$TMP/cert.json" "$ART/unsound_${i}.cert.json"
                cp "$TMP/check.txt" "$ART/unsound_${i}.txt"
            fi
        fi
    fi

    if [ $((i % 1000)) -eq 0 ]; then
        printf "  %6d/%d — exact=%d none=%d unknown=%d crashes=%d check_fail=%d unsound=%d timeouts=%d\n" \
            "$i" "$N" "$accepted" "$none" "$unknown" "$crashes" "$check_fails" "$unsound" "$timeouts"
    fi
done

echo ""
echo "=== FUZZ RESULTS ==="
echo "Iterations:            $N"
echo "Overall EXACT:         $accepted"
echo "Overall NONE:          $none"
echo "Overall UNKNOWN:       $unknown"
echo "Decider crashes:       $crashes"
echo "Checker failures:      $check_fails"
echo "Unsound certificates:  $unsound"
echo "Timeouts:              $timeouts"
echo ""

if [ "$crashes" -eq 0 ] && [ "$check_fails" -eq 0 ] && [ "$unsound" -eq 0 ] && [ "$timeouts" -eq 0 ]; then
    echo "RESULT: fuzz clean."
    exit 0
else
    echo "RESULT: fuzz found bugs. See $ART/"
    exit 1
fi
