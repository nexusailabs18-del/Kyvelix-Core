#!/usr/bin/env bash
# Phase 8 standalone verification driver: unit-level feature tests, differential
# fuzz against the concrete interpreter, and the real-toolchain e2e suite, all run
# through `cargo test`. Mirrors the fixtures in `tests/phase8_{features,fuzz,e2e}.rs`
# and the `src/ipa/*` unit tests.
#
# The fuzz binary is the slow one: 2,000 generated programs per seed across ten
# drivers, so it is given its own line and the rest are cheap by comparison.
# Release mode throughout -- the interpreter and the fixpoints are pure Rust and
# the debug build multiplies the fuzz wall time for no extra signal.
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

echo "== Phase 8 test binaries =="
run "phase8_features" --test phase8_features
run "phase8_fuzz"     --test phase8_fuzz
run "phase8_e2e"      --test phase8_e2e

echo ""
echo "== Phase 8 library units =="
run "lib ipa::" --lib 'ipa::'

echo ""
echo "== Regression: the Phase 7 suite (and transitively 5/6) =="
if bash tests/run_phase7_tests.sh >/dev/null 2>&1; then
    ok "run_phase7_tests.sh"
else
    bad "run_phase7_tests.sh"
fi

echo ""
echo "== Certificate boundary: the checker binary's own units =="
# The e2e suite above drives the real m0-check binary end to end; this is the
# cheap guard that the checker's replay logic has not drifted underneath Phase 8.
# Note the filter: `--lib checker` matches nothing, because those tests live in
# the `m0-check` *bin*, not in the library.
run "bin m0-check" --bin m0-check

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
