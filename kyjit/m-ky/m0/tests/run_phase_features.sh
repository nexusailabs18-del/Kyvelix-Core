#!/usr/bin/env bash
# Custom Phase 1/2/3 feature-verification suites.
# Each asserts hand-computed expected values against the public APIs, and the
# Phase 1/3 suites also drive the real m0-check binary end-to-end.
set -u
cd "$(dirname "$0")/.."

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

run_suite() {
    local name="$1"
    local out
    out="$(cargo test --release --test "$name" 2>&1)"
    local line
    line="$(echo "$out" | grep -E 'test result' | tail -1)"
    if echo "$out" | grep -qE '^error' ; then
        bad "$name  (compile error)"
        echo "$out" | grep -E '^error' -A4 | head -20
        return
    fi
    if echo "$line" | grep -q 'FAILED'; then
        bad "$name  ($line)"
        echo "$out" | grep -E 'FAILED|panicked' | head -10
    else
        ok "$name  ($line)"
    fi
}

echo "== Custom feature suites =="
run_suite phase1_features
run_suite phase2_features
run_suite phase3_features

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]