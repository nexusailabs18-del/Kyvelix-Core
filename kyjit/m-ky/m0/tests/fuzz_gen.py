#!/usr/bin/env python3
"""Generate a random loop JSON for fuzzing M0 (and the table path M2a.5).

Two shapes are generated, chosen at random:

  * a plain affine loop (the original M0 fuzzer's shape), and
  * a depth-1 `A[B[i] + op]` vs `A[B[j] + oq]` loop with a literal table `B`,
    whose declared domain and whose IV lattice are deliberately allowed to
    disagree — the loop may start below or inside the domain, stop before or
    past it, and step by any of 1..4.

The second shape exists because M2a.5 was previously unfuzzed: nothing
generated a table, so a bug that only shows up when the loop's step lattice
skips a cell of the domain (a "phantom iteration") could pass every run.
"""
import json, random, sys

seed = int(sys.argv[1]) if len(sys.argv) > 1 else random.randint(0, 2**32)
random.seed(seed)


def gen_affine_loop():
    """The original M0 shape: depth 1..4, arbitrary affine subscripts."""
    depth = random.choice([1, 2, 3, 4])
    ivs = []
    for k in range(depth):
        lo = random.randint(0, 100)
        # Cap upper so the search space doesn't explode
        span = random.randint(1, 80)
        hi = lo + span
        step = random.choice([1, 2, 4])
        ivs.append({"name": chr(ord('i') + k), "lower": lo, "upper": hi, "step": step})

    n_acc = random.randint(2, 5)
    body = []
    for _ in range(n_acc):
        body.append({
            "kind": random.choice(["read", "write"]),
            "base": random.choice(["A", "B", "C"]),
            "index": [random.randint(-2, 2) for _ in range(depth)],
            "elem_size": random.choice([1, 2, 4, 8]),
            "offset": random.randint(-8, 8),
        })
    return {"depth": depth, "ivs": ivs, "body": body}


def gen_table_loop():
    """A depth-1 literal-table loop: `A[B[i] + op]` against `A[B[j] + oq]`.

    M2a.5 decides this shape, so every field it reads has to be realistic:
    an injective-on-domain table, matching `base`/`domain` on both accesses,
    a pure lookup subscript (`index == [0]`, no range, affine), constant
    offsets, and one of the two accesses a write. The IV lattice and the
    domain are chosen independently so the two filters M2a.5 applies — the
    step lattice, then the domain — are both exercised, including the case
    where the lattice skips every cell that would have collided.
    """
    # The loop's own lattice. `step` is weighted towards the values that make
    # a phantom iteration possible (anything but 1).
    lower = random.randint(-3, 5)
    upper = lower + random.randint(1, 12)
    step = random.choice([1, 2, 2, 3, 4, 5])
    ivs = [{"name": "i", "lower": lower, "upper": upper, "step": step}]

    # The table's declared domain, independent of the lattice above.
    domain_lo = random.randint(0, 3)
    domain_len = random.randint(1, 8)
    domain_hi = domain_lo + domain_len

    # The table itself. Sometimes the values live in a small range, so
    # collisions are common and the Exact path is hit; sometimes they are
    # spread out and the table is injective, so absence is decidable.
    if random.random() < 0.5:
        tbl = [random.randint(0, 6) for _ in range(domain_hi)]
    else:
        tbl = random.sample(range(0, 200), domain_hi)

    # Occasionally the declared domain runs past the table, which M2a.5 must
    # treat as "not injective" rather than trusting the declaration.
    if random.random() < 0.1:
        domain_hi += random.randint(1, 3)

    off_p = random.randint(-6, 6)
    off_q = random.randint(-6, 6)

    def access(kind, tbl, off):
        return {
            "kind": kind,
            "base": "A",
            "index": [0],
            "elem_size": 8,
            "offset": off,
            "indirect": {
                "base": "B",
                "index": [1],
                "table": tbl,
                "domain_lo": domain_lo,
                "domain_hi": domain_hi,
            },
        }

    # Both accesses must name the same table for M2a.5 to decide; give the
    # second one a different table now and then so the "tables differ" guard
    # is fuzzed too.
    tbl_q = tbl if random.random() < 0.85 else [v + 1 for v in tbl]

    w = access("write", tbl, off_p)
    r = access("read", tbl_q, off_q)
    # Keep the write first most of the time. Swapping puts the write second,
    # which exercises the decider's pair ordering without ever making both
    # accesses reads -- that pair is rejected upstream before M2a.5 runs.
    body = [w, r] if random.random() < 0.7 else [r, w]
    return {"depth": 1, "ivs": ivs, "body": body}


loop = gen_table_loop() if random.random() < 0.4 else gen_affine_loop()
print(json.dumps(loop))
