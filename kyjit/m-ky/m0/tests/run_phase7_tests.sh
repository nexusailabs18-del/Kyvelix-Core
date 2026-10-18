#!/usr/bin/env bash
# Phase 7 standalone verification driver: unit-level feature tests, differential
# fuzz tests, and the real-toolchain e2e suite, all run through `cargo test`.
# Mirrors the fixtures in `tests/phase7_{features,fuzz,e2e}.rs` and the
# `src/points/*` unit tests.
set -u
cd "$(dirname "$0")/.."
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

run() {
    local name="$1"; shift
    if cargo test --release "$@" 2>&1 | grep -q "test result: ok"; then
        ok "$name"
    else
        bad "$name"
    fi
}

echo "== Phase 7 test binaries =="
run "phase7_features" --test phase7_features
run "phase7_fuzz"     --test phase7_fuzz
run "phase7_e2e"      --test phase7_e2e

echo ""
echo "== Phase 7 library units =="
run "lib points::" --lib 'points::'

echo ""
echo "== Regression: the Phase 5/6 suites and the checker =="
if bash tests/run_phase5_6_tests.sh >/dev/null 2>&1; then
    ok "run_phase5_6_tests.sh"
else
    bad "run_phase5_6_tests.sh"
fi

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
