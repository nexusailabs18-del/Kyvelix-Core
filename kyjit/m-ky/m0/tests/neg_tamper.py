#!/usr/bin/env python3
"""Corrupt one certificate for `tests/run_negative.sh`.

Usage: neg_tamper.py <base-cert> <out-cert> <mutation>

Phase 9a made `m0-decide` write `[decision_cert, evidence...]` for loops that
also warrant a trip-count / scalar-dependence claim, and the bare object
otherwise. The older inline patch scripts here assumed the bare-object shape: on
an array they raised `TypeError`, exited non-zero, and — because the script had
`set -u` but no error handling — left the *previous* run's artifact in place for
`m0-check` to reject. Seven of the eight cases were therefore passing vacuously,
"REJECT"-ing a stale file instead of the intended corruption.

Every mutation now names the certificate it targets, and a missing target is a
hard error (non-zero exit, no output file) so the harness reports a broken case
rather than a spurious pass. The document is rewritten in the shape it arrived
in, so the checker still sees a conjunction of decision *and* evidence
certificates and rejects only because of the tampered field.
"""
import json
import sys


def pick(doc, pred, what):
    """The first certificate matching `pred`, in a document of either shape."""
    certs = doc if isinstance(doc, list) else [doc]
    for c in certs:
        if pred(c):
            return c
    raise SystemExit("no %s certificate in the document" % what)


def claim_kind(c, kind):
    return (c.get("claim") or {}).get("kind") == kind


def witness_type(c, ty):
    return (c.get("witness") or {}).get("type") == ty


DECISION = lambda doc: pick(doc, lambda c: witness_type(c, "IntSolution"), "IntSolution")
TRIP = lambda doc: pick(doc, lambda c: claim_kind(c, "TripCount"), "TripCount")
SCALAR_DD = lambda doc: pick(doc, lambda c: claim_kind(c, "ScalarDependence"),
                             "ScalarDependence")

MUTATIONS = {
    # ---- the access-pair decision certificate ----
    "loop_hash": (DECISION, lambda c: c.__setitem__("loop_hash", "deadbeef" * 8)),
    "distance": (DECISION, lambda c: c["claim"].__setitem__("value", [999])),
    "witness_i": (DECISION, lambda c: c["witness"].__setitem__("i", [999999])),
    "witness_j": (DECISION, lambda c: c["witness"].__setitem__("j", [-999999])),
    "witness_mismatch": (
        DECISION,
        lambda c: c["witness"].__setitem__("j", [x + 1 for x in c["witness"]["j"]]),
    ),
    "witness_oob": (DECISION, lambda c: c["witness"].__setitem__("i", [10_000_000])),
    "claim_kind": (DECISION, lambda c: c["claim"].__setitem__("kind", "None")),
    # ---- Phase 9a loop-level evidence certificates ----
    "evidence_wrong_hash": (TRIP, lambda c: c.__setitem__("loop_hash", "deadbeef" * 8)),
    "evidence_trip_count": (
        TRIP,
        lambda c: c["claim"]["value"].__setitem__("count", c["claim"]["value"]["count"] + 1),
    ),
    "evidence_scalar_distance": (
        SCALAR_DD,
        lambda c: c["claim"]["value"].__setitem__("distance", "AnyAtOrBefore"),
    ),
}


def main():
    base_path, out_path, name = sys.argv[1], sys.argv[2], sys.argv[3]
    try:
        select, mutate = MUTATIONS[name]
    except KeyError:
        raise SystemExit("unknown mutation %r (have: %s)" % (name, sorted(MUTATIONS)))
    doc = json.load(open(base_path))
    mutate(select(doc))
    json.dump(doc, open(out_path, "w"), indent=2)


main()

