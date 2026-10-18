# Phase 7 status — Points-to / Aliasing (Wolfe Chapter 8.2)

Date: 2026-10-03. Spec: `phase7,8and9.md` §7 (`refernce.txt` §8.2.1–§8.2.4).

**Verdict: Phase 7 is complete and verified.** All nine modules of §7.1 exist;
the points-to fixpoint, the alias query, dope-vector association, pointer
reconstruction and allocation dependence are implemented, wired through the
**real** `m0-check`, and covered by standalone feature tests, differential fuzz
with independent oracles, and end-to-end accept/reject round trips. Every fact a
Phase 7 certificate asserts is re-derived by the checker from the carried
pointer program — no analyzer conclusion crosses the certificate boundary.

---

## 1. Module inventory

| § | Module | Lines | Purpose |
| --- | --- | --- | --- |
| 7.2 | `src/points/relation.rs` | ~415 | `VarId`, `Definiteness`, `PtrRelation`, `PointsToGraph`, the lattice meet, must/may queries |
| 7.3 | `src/points/transfer.rs` | ~200 | the four Wolfe §8.2.1 assignment cases as six `Effect`s |
| 7.4 | `src/points/fixpoint.rs` | ~420 | `PtrProgram` (the pointer IR) and `analyze` |
| 7.5 | `src/points/dopvec.rs` | ~255 | Fortran 90 dope vectors and array-pointer association |
| 7.6 | `src/points/ptrarith.rs` | ~245 | `*(p + i·k + c)` → `A[i·k + c]` reconstruction |
| 7.7 | `src/points/allocdep.rs` | ~130 | allocation dependence (F90 vs C escape) |
| 7.8 | `src/points/alias.rs` | ~230 | Must/May/No alias query + brute-force oracle |
| 7.8 | `src/points/certificate.rs` | ~350 | `AliasClaim`, `AliasBasis`, the checker replay, emitters |
| — | `src/points/mod.rs` | ~70 | public API and module docs |

Source: 2,315 lines across the nine modules; standalone tests:
`tests/phase7_features.rs` (13), `tests/phase7_fuzz.rs` (6),
`tests/phase7_e2e.rs` (6), plus 56 `src/points/*` unit tests.

---

## 2. The trust boundary, restated

The spec's `Witness::AliasWitness { relation, basis }` carries the *relation the
analyzer computed*. That is precisely the trust this crate refuses for every
other claim kind, so Phase 7 carries `Witness::AliasEvidence { program, basis }`
instead: the pointer program (ground truth) plus a basis naming which part of it
the claim speaks about. `m0-check` re-runs
[`m0::points::analyze`](../m0/src/points/fixpoint.rs) over that program and
re-derives the answer. A forged basis cannot promote a wrong claim into an
accepted certificate, because the re-derived answer is a function of the
program, not of the witness. This is the one deliberate deviation from the
spec's witness types, and it is a strengthening.

---

## 3. Soundness decisions worth naming

- **`UNKNOWN_TARGET`.** Every unresolved pointer starts pointing at a synthetic
  `unknown` target that may alias anything. A pointer that reaches `unknown` is
  never reported `NoAlias`, so an analysis that gives up is conservative rather
  than wrong.
- **A shrinking meet.** The spec's `meet_into` only unions the two predecessor
  graphs, which keeps a target `Definite` even when one predecessor never
  established it — that would let `must` over-claim. `PointsToGraph::meet_with`
  instead keeps an edge only when *every* source agrees, and `Definite` only when
  every source is definite.
- **Monotone transfers.** Every `Effect` only weakens definiteness or adds
  targets, never strengthens, which is why `analyze` terminates on the finite
  lattice without a widening operator.
- **A `BTreeMap`, not the spec's `HashMap`.** The graph is compared for equality
  and serialised into a certificate; a canonical order makes both deterministic.

---

## 4. Verification

| Suite | Tests | Result |
| --- | --- | --- |
| `cargo test --release --lib points::` | 56 | ok |
| `tests/phase7_features.rs` | 13 | ok |
| `tests/phase7_fuzz.rs` (differential + seed-and-revert) | 6 | ok |
| `tests/phase7_e2e.rs` (real `m0-check`, accepts + tampers) | 6 | ok |
| `tests/run_phase7_tests.sh` (**new**) | — | green |

The fuzz drivers each carry an independent oracle: brute-force enumeration of
both index sets for the alias rule, and a concrete single-target interpreter for
the fixpoint. Both prove non-vacuity (the enumeration must actually see
disjoint / overlapping / identical pairs; the programmes must actually set a
pointer) and both are run with a deliberately-wrong oracle first, where the
violations must appear, then with the correct one, where they must vanish — so
neither oracle is trusted on faith.

The e2e suite drives the real `m0-check`: a real `MustAlias` / `NoAlias` /
`DisjointArrays` certificate is accepted, and a `must` the graph does not force,
a `no-alias` between two pointers that share a target, and a `disjoint-arrays`
claim for overlapping walks are all rejected by the checker's own recomputation.

---

## 5. How to reproduce

```sh
cd m0
cargo test --release --lib points::   # 56 passing
bash tests/run_phase7_tests.sh        # features + fuzz + e2e + lib units
cargo test --release                  # full suite
```

---

## 6. Honest scope notes

- **Input model.** The array IR (`crate::ir::Access`) has no pointer variable, so
  a `PtrProgram` is produced by a frontend that lowers pointers into it (Phase 9)
  rather than derived from a `Loop`. This mirrors Phase 6's `RegionProgram`: the
  analysis runs over the IR that actually models the thing being analysed.
- **Alias completeness.** `arrays_disjoint` is sound but not complete — it settles
  different arrays and constant references, and declines the strided same-array
  case. Completeness for arithmetic progressions belongs to the solver in
  `region::triplet`, which Phase 7 does not duplicate.
- **The `AllocationOrder` basis** can only ever *strengthen* a `NoAlias` that the
  graph already proves, because an escaping allocation result reaches `unknown`,
  which by itself defeats disjointness. The code path and its tests are kept so
  the argument is available when a future frontend tracks escapes more precisely.
- **Where the spec's line estimate went.** The nine modules total ~2,315 lines
  rather than the ~5,350 the spec estimated. Two of the spec's proposed modules
  (`alias.rs`, `certificate.rs`) collapsed to smaller helpers once the graph
  queries already existed, and the certificate vocabulary lives partly in
  `src/certificate/mod.rs` (the crate's single `Claim`/`Witness` home). Nothing
  in §7.1's *behaviour* was dropped.
