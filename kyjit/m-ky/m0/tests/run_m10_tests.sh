#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m10 2>&1 | grep -q "test result: ok"; then
    ok "decide::m10 unit tests"
else
    bad "decide::m10 unit tests"
fi

echo ""
echo "== E2E (2 loops) =="
e2e() {
    local file="$1" want="$2"
    if [ ! -f "tests/m10_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m10_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [ ! -f cert.json ]; then
        bad "$file  no cert.json"; return
    fi
    if ! grep -q '"M10"' cert.json; then
        bad "$file  cert level not M10"; return
    fi
    if ! ./target/release/m0-check "tests/m10_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
        bad "$file  checker rejected"; return
    fi
    ok "$file  $want  (checker ACCEPT)"
}

e2e f001.json EXACT
e2e f002.json NONE

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
