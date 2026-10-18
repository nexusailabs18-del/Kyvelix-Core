#!/usr/bin/env bash
# Phase 5 §5.1.7 (cubic-plus) end-to-end: two degree-6 Product-index loops that
# the old `MAX_DEGREE = 4` cap refused. Each must decide EXACT, emit an M6
# certificate, and round-trip through `m0-check`.
set -u
cd "$(dirname "$0")/.."
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Cubic+ (degree-6) E2E =="
e2e() {
    local file="$1" want="$2"
    if [ ! -f "tests/m6c5_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m6c5_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [ ! -f cert.json ]; then
        bad "$file  no cert.json"; return
    fi
    if ! grep -q '"M6"' cert.json; then
        bad "$file  cert has no M6 level"; return
    fi
    if ! ./target/release/m0-check "tests/m6c5_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
        bad "$file  checker rejected"; return
    fi
    ok "$file  $want  (M6 cert, checker ACCEPT)"
}

e2e c001.json EXACT
e2e c002.json EXACT

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]