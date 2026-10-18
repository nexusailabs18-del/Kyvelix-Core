#!/usr/bin/env bash
# M0 stepped loops: a loop's IV runs `lower, lower + step, ...` below `upper`,
# which is *not* the dense interval [lower, upper) unless step == 1.
#
# Every stage of M0 before this suite reasoned over the dense box, so a stepped
# loop could be witnessed at an iteration the program never performs: for
# `for (i = 0; i < 2; i += 2)` the box [0, 2) also holds i = 1. The result was a
# certificate the checker rejected ("witness iteration does not execute the
# access"), which the real-loop corpus showed on all 74 of its stepped loops.
#
# These cases pin the fix: presence of a collision is decided on the lattice,
# the witness lies on it, and the reported distance is rescaled by the step.
set -u
cd "$(dirname "$0")/.."

DECIDE=./target/release/m0-decide
CHECK=./target/release/m0-check
LOOPS=tests/m0_step_loops
PASS=0
FAIL=0
ok()  { PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
bad() { FAIL=$((FAIL+1)); printf "  FAIL  %s\n" "$1"; }

echo "== Unit tests =="
if cargo test --quiet --lib decide::m0 2>&1 | grep -q "test result: ok"; then
    ok "decide::m0 unit tests (incl. the stepped-loop cases)"
else
    bad "decide::m0 unit tests"
fi

echo ""
echo "== E2E (5 loops) =="

# Verify every witness iteration is one the loop actually performs, and that the
# certificate's claim is the witness's own distance. The checker already rejects
# a witness off the lattice; this catches the weaker failure of an
# on-the-lattice witness whose claim was not rescaled by the step.
verify_lattice() {
    local loop="$1" cert="$2"
    python3 - "$loop" "$cert" <<'PY'
import json, sys
loop = json.load(open(sys.argv[1]))
cert = json.load(open(sys.argv[2]))
# Phase 9a: m0-decide writes [decision_cert, evidence...] when the loop also
# warrants a trip-count / scalar-dependence claim, and the bare object
# otherwise. The lattice check concerns the access-pair decision only, so pick
# the certificate that carries an IntSolution witness.
if isinstance(cert, list):
    cert = next((c for c in cert
                 if (c.get("witness") or {}).get("type") == "IntSolution"), cert[0])
w = cert.get("witness", {})
if w.get("type") != "IntSolution":
    print("witness is not an IntSolution: %r" % (w.get("type"),))
    sys.exit(1)
i, j = w["i"], w["j"]
for name, vec in (("i", i), ("j", j)):
    for k, v in enumerate(vec):
        iv = loop["ivs"][k]
        step = iv.get("step", 1)
        if not (iv["lower"] <= v < iv["upper"]) or (v - iv["lower"]) % step != 0:
            print("%s[%d] = %d is not on the lattice of iv %r" % (name, k, v, iv))
            sys.exit(1)
delta = [j[k] - i[k] for k in range(len(i))]
claim = cert.get("claim", {})
if claim.get("kind") != "Exact" or claim.get("value") != delta:
    print("claim %r != j - i %r" % (claim, delta))
    sys.exit(1)
PY
}

e2e() {
    local file="$1" want="$2" note="$3"
    if [ ! -f "$LOOPS/$file" ]; then bad "$file (missing)"; return; fi
    rm -f cert.json
    local out
    out="$("$DECIDE" "$LOOPS/$file" 2>&1 | tr -d '\r')"
    local overall
    overall="$(echo "$out" | grep -oP 'overall: \K\w+' || echo "ERROR")"
    if [ "$overall" != "$want" ]; then
        bad "$file  want=$want got=$overall  ($note)"; return
    fi
    if [ -f cert.json ]; then
        if ! "$CHECK" "$LOOPS/$file" cert.json 2>&1 | grep -q ACCEPT; then
            bad "$file  checker rejected the certificate"; return
        fi
        if [ "$want" = "EXACT" ]; then
            local why
            why="$(verify_lattice "$LOOPS/$file" cert.json 2>&1)"
            if [ -n "$why" ]; then
                bad "$file  $why"
                return
            fi
        fi
    elif [ "$want" = "EXACT" ]; then
        bad "$file  EXACT without a certificate"; return
    fi
    ok "$file  $want  (checker ACCEPT, witness on-lattice)  -- $note"
}

# The write lands on the read's location at i = 1 and i = 3, but stepping by 2
# from 0 runs neither.
e2e s001_skipped_collision.json  NONE  "the only collisions are on skipped iterations"
# The same accesses with step 1 do reach them, so the dependence is real.
e2e s002_reached_collision.json  EXACT "step=1 reaches the colliding iterations"
# A location gap of 2 elements is one step of 2, not one iteration.
e2e s003_distance_rescaled.json  EXACT "distance rescaled by the step"
# A stepped lattice per level: (2,3) from (0,0).
e2e s004_2d_stepped.json         EXACT "2-D stepped lattices stay on-lattice"
# Wide and stepped: the distance is still the exact same-stride one.
e2e s005_wide_stepped.json       NONE  "stepped parity refutes the adjacent offset"

echo ""
echo "======================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "======================================"
[ "$FAIL" -eq 0 ]