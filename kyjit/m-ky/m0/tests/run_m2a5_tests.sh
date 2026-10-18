#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m2a5 2>&1 | grep -q "test result: ok"; then
    ok "decide::m2a5 unit tests"
else
    bad "decide::m2a5 unit tests"
fi

echo ""
echo "== E2E (7 loops) =="

e2e() {
    local file="$1" want="$2"
    if [ ! -f "tests/m2a5_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m2a5_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [ -f cert.json ]; then
        if ! ./target/release/m0-check "tests/m2a5_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
            bad "$file  checker rejected"; return
        fi
    fi

    ok "$file  $want"
}

e2e t001.json EXACT
e2e t002.json NONE
e2e t003.json UNKNOWN
e2e t004.json UNKNOWN
e2e t005.json UNKNOWN
e2e t006.json NONE
e2e t007.json NONE
# Stepped / offset lattices: a collision that only occurs on an iteration the
# loop never performs must not become a dependence, while the same collision at
# step 1 is real.
e2e t008.json NONE
e2e t009.json EXACT
# lower = 10 pushes every iteration past the table's domain, so no cell is
# ever named and the analysis is vacuous rather than a dependence.
e2e t010.json NONE

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
