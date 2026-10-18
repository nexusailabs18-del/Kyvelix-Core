#!/usr/bin/env bash
# Negative tests: verify the checker REJECTs deliberately corrupted certificates.
set -u
cd "$(dirname "$0")/.."

DECIDE=./target/release/m0-decide
CHECK=./target/release/m0-check
NEG=tests/negative
LOOP=tests/loops/01_distance4.json

pass=0
fail=0

printf "%-30s %-10s %s\n" "CASE" "EXPECTED" "GOT"
printf "%-30s %-10s %s\n" "----" "--------" "---"

run_case() {
    local name="$1"
    local cert="$2"
    local expected="$3"
    local raw got
    raw=$("$CHECK" "$LOOP" "$cert" 2>&1 | tail -1)
    # Extract first word (ACCEPT or REJECT)
    got=$(echo "$raw" | grep -oE '^(ACCEPT|REJECT)' || echo "PANIC")
    printf "%-30s %-10s %s\n" "$name" "$expected" "$got"
    if [ "$got" = "$expected" ]; then
        pass=$((pass+1))
    else
        fail=$((fail+1))
    fi
}

# Generate a valid certificate as the base
rm -f cert.json
"$DECIDE" "$LOOP" > /dev/null 2>&1
mv cert.json "$NEG/valid.json"
BASE="$NEG/valid.json"

# Phase 9a made m0-decide emit `[decision_cert, evidence...]` for loops that
# also warrant a trip-count / scalar-dependence claim, and the bare object
# otherwise. These cases tamper with the access-pair decision, so the helper
# below addresses the element carrying an IntSolution witness and rewrites the
# file in whatever shape it arrived in. A missing element is a hard error: it
# used to be that the patch script simply crashed on a list, the `python3 -`
# heredocs were non-fatal under `set -u`, and the stale artifact from the
# previous loop was then checked — a REJECT that tested nothing.
TAMPER=tests/neg_tamper.py

run_tamper() {
    local name="$1" out="$2"
    rm -f "$out"
    if ! python3 "$TAMPER" "$BASE" "$out" "$name"; then
        fail=$((fail+1))
        printf "%-30s %-10s %s\n" "$3" "$4" "TAMPER-ERROR (no certificate to corrupt)"
        return
    fi
    run_case "$3" "$out" "$4"
}

run_case "valid_baseline"      "$BASE"                    "ACCEPT"

run_tamper loop_hash        "$NEG/bad_hash.json"      "bad_loop_hash"      "REJECT"
run_tamper distance         "$NEG/bad_distance.json"  "bad_distance_value" "REJECT"
run_tamper witness_i        "$NEG/bad_i.json"         "bad_witness_i"      "REJECT"
run_tamper witness_j        "$NEG/bad_j.json"         "bad_witness_j"      "REJECT"
run_tamper witness_mismatch "$NEG/mismatch.json"      "witness_mismatch"   "REJECT"
run_tamper witness_oob      "$NEG/oob.json"           "witness_oob"        "REJECT"
run_tamper claim_kind       "$NEG/wrong_kind.json"    "wrong_claim_kind"   "REJECT"

# Phase 9a evidence certificates, tampered inside the array. The checker treats
# a certificate array as a conjunction, so one bad element rejects the file.
run_tamper evidence_wrong_hash      "$NEG/bad_evidence_hash.json"     "evidence_wrong_hash"      "REJECT"
run_tamper evidence_trip_count      "$NEG/bad_evidence_trip.json"     "evidence_trip_count"      "REJECT"
run_tamper evidence_scalar_distance "$NEG/bad_evidence_dd.json"       "evidence_scalar_distance" "REJECT"

echo ""
echo "Pass: $pass"
echo "Fail: $fail"
if [ "$fail" -eq 0 ]; then
    echo "RESULT: checker is sound on all negative cases."
    exit 0
else
    echo "RESULT: checker has soundness gaps."
    exit 1
fi
