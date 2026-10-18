#!/usr/bin/env bash
# Phase 1 (certificate foundations): Claim::Direction / Claim::Level /
# Claim::CrossLoop and their witness carriers.
set -u
cd "$(dirname "$0")/.."
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib certificate::tests 2>&1 | grep -q "test result: ok"; then
    ok "certificate unit tests (Dir / level / serde)"
else
    bad "certificate unit tests (Dir / level / serde)"
fi
if cargo test --quiet --bin m0-check 2>&1 | grep -q "test result: ok"; then
    ok "m0-check unit tests (claim_ok)"
else
    bad "m0-check unit tests (claim_ok)"
fi

cargo build --release --quiet --bins 2>/dev/null || { bad "cargo build --release"; exit 1; }
CHECK=./target/release/m0-check
LOOP=tests/loops/01_distance4.json
CERTS=tests/phase1

echo ""
echo "== E2E: accept (6) =="
accept() {
    local name="$1"
    local out
    out="$($CHECK "$LOOP" "$CERTS/$name" 2>&1 | tail -1)"
    case "$out" in
        ACCEPT*) ok "$name  ($out)";;
        *)       bad "$name  got: $out";;
    esac
}

# IntSolution carries Direction / Level.
accept direction_ok.json
accept level_ok.json
# The same claims flow through the other carriers.
accept with_loop_direction.json
accept with_env_level.json
accept assertion_chain_direction.json
# CrossLoopSolution carries a CrossLoop distance.
accept cross_loop_ok.json

echo ""
echo "== E2E: reject (5) =="
reject() {
    local name="$1" want="$2"
    local out
    out="$($CHECK "$LOOP" "$CERTS/$name" 2>&1 | tail -1)"
    case "$out" in
        REJECT*) if [[ "$out" == *"$want"* ]]; then
                     ok "$name  ($out)"
                 else
                     bad "$name  want msg '$want'  got: $out"
                 fi;;
        *)       bad "$name  expected REJECT  got: $out";;
    esac
}

reject direction_bad.json          "direction mismatch"
reject level_bad.json              "level mismatch"
reject cross_loop_bad_distance.json "cross-loop distance mismatch"
reject cross_loop_bad_claim.json   "non-CrossLoop claim"
reject cross_loop_bad_hash.json    "first_loop does not match"

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
