#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m12 2>&1 | grep -q "test result: ok"; then
    ok "decide::m12 unit tests"
else
    bad "decide::m12 unit tests"
fi

echo ""
echo "== Note =="
echo "  E2E for M12 requires a DynamicLoop input format (ivs with BoundValue)."
echo "  m0-decide currently reads standard Loop (i64 ivs). A dedicated"
echo "  m12-decide binary would be needed for full E2E. Unit tests cover"
echo "  the resolution + delegation + certificate wrapping."

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]
