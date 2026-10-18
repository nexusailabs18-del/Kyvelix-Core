#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m2b 2>&1 | grep -q "test result: ok"; then
    ok "decide::m2b unit tests (incl. decide_param)"
else
    bad "decide::m2b unit tests"
fi

echo ""
echo "== E2E (3 loops) =="

e2e() {
    local file="$1" want="$2" env="${3:-p=8}"
    if [ ! -f "tests/m9_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(M0_ENV="$env" ./target/release/m0-decide "tests/m9_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [ ! -f cert.json ]; then
        bad "$file  no cert.json"; return
    fi
    if ! grep -q '"M9"' cert.json; then
        bad "$file  cert level not M9"; return
    fi
    if ! ./target/release/m0-check "tests/m9_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
        bad "$file  checker rejected the certificate"; return
    fi
    ok "$file  $want  (checker ACCEPT)"
}

e2e n001.json EXACT
e2e n002.json EXACT
e2e n003.json EXACT "p=10000"

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
