#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m5 2>&1 | grep -q "test result: ok"; then
    ok "decide::m5 unit tests"
else
    bad "decide::m5 unit tests"
fi

echo ""
echo "== E2E (7 loops) =="

e2e() {
    local file="$1" want="$2" m5="$3"
    if [ ! -f "tests/m5_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m5_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [[ "$m5" == "yes" ]]; then
        if [ ! -f cert.json ]; then
            bad "$file  no cert.json"; return
        fi
        if ! grep -q '"M5"' cert.json; then
            bad "$file  cert level not M5: $(grep -o '\"level\"[^,]*' cert.json | head -1)"; return
        fi
    fi
    if [ -f cert.json ]; then
        if ! ./target/release/m0-check "tests/m5_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
            bad "$file  checker rejected"; return
        fi
    fi

    ok "$file  $want"
}

e2e p001.json EXACT   yes
e2e p002.json EXACT   yes
e2e p003.json EXACT   yes
e2e p004.json UNKNOWN no
e2e p005.json UNKNOWN no
e2e p006.json EXACT   yes
e2e p007.json NONE    no

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
