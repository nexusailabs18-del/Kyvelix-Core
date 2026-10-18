#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m4 2>&1 | grep -q "test result: ok"; then
    ok "decide::m4 unit tests"
else
    bad "decide::m4 unit tests"
fi

echo ""
echo "== E2E (7 loops) =="

e2e() {
    local file="$1" want="$2" m4="$3"
    if [ ! -f "tests/m4_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m4_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [[ "$m4" == "yes" ]]; then
        if [ ! -f cert.json ]; then
            bad "$file  no cert.json"; return
        fi
        if ! grep -q '"M4"' cert.json; then
            bad "$file  cert level not M4: $(grep -o '\"level\"[^,]*' cert.json | head -1)"; return
        fi
    fi
    if [ -f cert.json ]; then
        if ! ./target/release/m0-check "tests/m4_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
            bad "$file  checker rejected"; return
        fi
    fi

    ok "$file  $want"
}

e2e m001.json EXACT   yes
e2e m002.json EXACT   yes
e2e m003.json EXACT   yes
e2e m004.json NONE    yes
e2e m005.json NONE    yes
e2e m006.json UNKNOWN no
e2e m007.json EXACT   yes

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
