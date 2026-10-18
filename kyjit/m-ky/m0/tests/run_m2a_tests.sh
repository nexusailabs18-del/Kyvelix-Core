#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m2a 2>&1 | grep -q "test result: ok"; then
    ok "decide::m2a unit tests"
else
    bad "decide::m2a unit tests"
fi

echo ""
echo "== E2E (8 loops) =="

# $1 file   $2 EXACT|NONE|UNKNOWN   $3 m2a-cert-required(yes|no)
e2e() {
    local file="$1" want="$2" m2a="$3"
    if [ ! -f "tests/m2a_loops/$file" ]; then
        bad "$file (missing)"; return
    fi
    rm -f cert.json
    local out
    out="$(./target/release/m0-decide "tests/m2a_loops/$file" 2>/dev/null | tr -d '\r')"
    if [[ "$out" != *"$want"* ]]; then
        bad "$file  want=$want  got: $(echo "$out" | head -1)"; return
    fi
    if [[ "$m2a" == "yes" ]]; then
        if [ ! -f cert.json ]; then
            bad "$file  no cert.json"; return
        fi
        if ! grep -qE '"M2a(\.5)?"' cert.json; then
            bad "$file  cert level not M2a or M2a.5: $(grep -o '\"level\"[^,]*' cert.json | head -1)"; return
        fi
    fi
    if [ -f cert.json ]; then
        if ! ./target/release/m0-check "tests/m2a_loops/$file" cert.json 2>&1 | grep -q ACCEPT; then
            bad "$file  checker rejected"; return
        fi
    fi

    ok "$file  $want"
}

# --- canonical 7 ---
e2e a001.json EXACT   yes    # A[B[i]+1] = A[B[i]+1]  -> flat A[3i+2] = A[3i+2], dist 0
e2e a002.json NONE    no     # A[B[i]+1] = A[B[i]+2]  -> flat A[3i+2] vs A[3i+3], gcd 3 ∤ 1
e2e a003.json EXACT   yes    # 2D B[i,j]=2i+j
e2e a004.json EXACT   yes    # A[B[i]+5] = A[B[i]+5], B[i]=4i
e2e a005.json UNKNOWN no     # constant B[i]=0, not injective
e2e a006.json UNKNOWN no     # mixed direct/indirect
e2e a007.json EXACT   yes    # table, now handled by M2a.5

# --- extra: positive-distance case ---
e2e a008.json EXACT   yes    # A[B[i]+1] = A[B[i]+4]  -> flat A[3i+2] vs A[3i+5], dist -1

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
