#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m2g 2>&1 | grep -q "test result: ok"; then
    ok "decide::m2g unit tests"
else
    bad "decide::m2g unit tests"
fi

echo ""
echo "== E2E (7 loops) =="

# $1 file   $2 EXACT|UNKNOWN   $3 m2g-cert-required(yes|no)
e2e() {
    local file="$1" want="$2" m2g="$3"
    if [ ! -f "tests/m2g_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m2g_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [[ "$m2g" == "yes" ]]; then
        if [ ! -f cert.json ]; then
            bad "$file  no cert.json"; return
        fi
        if ! grep -q '"M2g"' cert.json; then
            bad "$file  cert level not M2g: $(grep -o '"level"[^,]*' cert.json | head -1)"; return
        fi
    fi
    if [ -f cert.json ]; then
        if ! ./target/release/m0-check "tests/m2g_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
            bad "$file  checker rejected"; return
        fi
    fi

    ok "$file  $want  (M2g)"
}

e2e g001.json EXACT   yes
e2e g002.json EXACT   yes
e2e g003.json EXACT   yes
e2e g004.json UNKNOWN no
e2e g005.json EXACT   yes
e2e g006.json UNKNOWN no
e2e g007.json UNKNOWN no

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
