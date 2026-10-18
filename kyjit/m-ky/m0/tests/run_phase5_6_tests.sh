#!/usr/bin/env bash
# Phase 5 + Phase 6 standalone verification driver: unit-level feature tests,
# differential fuzz tests, and the pure-toolchain e2e suites, all run through
# `cargo test`. Mirrors the fixtures assembled in
# `tests/phase5_{features,fuzz,e2e}.rs` and `tests/phase6_{features,fuzz,e2e}.rs`.
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

echo "== Phase 5 + Phase 6 test binaries =="
run "phase5_features" --test phase5_features
run "phase5_fuzz"     --test phase5_fuzz
run "phase5_e2e"      --test phase5_e2e
run "phase5_runtime"  --test phase5_runtime
run "phase6_features" --test phase6_features
run "phase6_fuzz"     --test phase6_fuzz
run "phase6_e2e"      --test phase6_e2e

echo ""
echo "== Phase 5 + 6 library units =="
run "lib region"  --lib region
run "lib ir (phase 5 modules)" --lib '::ir::'
run "lib fud alignment/crossloop" --lib alignment
run "lib fud crossloop" --lib crossloop

echo ""
echo "== Cubic+ degree-6 E2E =="
if bash tests/run_m6c5_tests.sh >/dev/null 2>&1; then
    ok "run_m6c5_tests.sh"
else
    bad "run_m6c5_tests.sh"
fi

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]