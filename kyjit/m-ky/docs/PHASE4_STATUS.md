# Phase 4 status — FUD consumers: IV detection, const-prop, scalar DD, trip count

Date: 2026-02-10 (supersedes the earlier draft of this file; §1, §5 and §8 were
rewritten after the Phase 9a evidence integration landed).
Spec: `phase4,5and6.txt` §4 (`refernce.txt` §4.1–§4.4).

**Verdict: §4.1, §4.3 and §4.4 are complete and fully verified, and the two
Phase 4 claim kinds now have real production producers: `m0-decide` emits
`TripCount` and `ScalarDependence` certificates for real loops and `m0-check`
verifies every certificate in the file (§5). §4.2 is **partly** done: M3 now
derives its IV through `induct`; M5 and M8 still do not, and for different
reasons — M5's chase chain is not expressible in the scalar IR until the Phase 9
frontend emits indexed loads, and M8's recurrence is not a class `induct`
computes at all. Read strictly — "Phase 4 complete" requires all three of
M3/M5/M8 to *consume* `induct` — Phase 4 is **not** complete. Read as "build and
certify the analysis layer" (which is what §4.1/§4.3/§4.4 and most of the §4.5
estimate describe), it is. This report does not paper over which reading applies.**
Every §4.1/§4.3/§4.4 deliverable is built, wired through the real checker, and
covered by tests that fail when the analysis is broken.

---

## 1. Final test totals

| Suite | Tests | Result |
| --- | --- | --- |
| `cargo test --release` (all targets) | **497** | **0 failed** |
| `src/lib.rs` unit tests | 360 | ok |
| `--lib fud::*` (the Phase 3+4 analysis core) | 54 | ok |
| `--lib decide::*` (the decision modules) | 197 | ok |
| `m0-check` unit tests | 21 | ok |
| `tests/phase4_scalardd.rs` (§4.1/§4.3 custom verification) | 9 | ok |
| `tests/phase4_fuzz.rs` (§4.4 differential + seeds) | 11 | ok |
| `tests/phase4_e2e.rs` (§4.3 checker arms) | 15 | ok |
| `tests/phase9a_evidence.rs` (producer → file → checker round trip, **new**) | 20 | ok |
| Phase 1 / 2 / 3 suites (regression) | 10 / 27 / 24 | ok |

Phase 3 closed at 362 passing tests and the Phase 4 analysis layer at 453; the
Phase 9a evidence integration adds **44**, to 497.

Harness suites, run **serially** (see §8):

| Script | Result |
| --- | --- |
| `tests/run_real.sh` | 1001 loops; 601 EXACT, 400 NONE, 0 UNKNOWN; checker **1001 ACCEPT / 0 REJECT**; 100.0% coverage |
| `tests/run_all.sh` | 10/10 ACCEPT, 0 REJECT, 0 UNKNOWN |
| `tests/run_negative.sh` | **11 pass / 0 fail** (was 8) — "checker is sound on all negative cases" |
| `tests/run_m3_tests.sh` | 8 PASS / 0 FAIL |
| `tests/run_m5_tests.sh` | 8 PASS / 0 FAIL |
| `tests/run_m8_tests.sh` | 3 PASS / 0 FAIL |
| `tests/run_m0_step_tests.sh` | 6 PASS / 0 FAIL (was 3 PASS / 3 FAIL — see §8) |
| every other `tests/run_m*_tests.sh` | 0 FAIL each (`run_m11_tests.sh` prints no tally; it exits 0) |
| `tests/fuzz.sh 3000` | `RESULT: fuzz clean` — 0 crashes, 0 checker failures, 0 unsound certs, 0 timeouts |

Two numbers in the earlier draft of this file were wrong and are corrected here.
`run_real.sh` counts **certificates accepted**, and since Phase 9a a loop with
evidence produces one file holding several certificates: the honest reading is
1001 files ACCEPT / 0 REJECT (the old "601 ACCEPT" was the pre-9a EXACT count,
which is not what the script counts). And `run_all.sh` covers `tests/loops`,
which is 10 loops, not 6 — its 6 ACCEPT was a pre-Phase-9 tally of EXACT verdicts.

The real corpus still certifies 601/1001 access-pair decisions with no checker
rejection — the Phase 4 analyses are wired in without regressing M0's coverage
or soundness.

---

## 2. §4.1 — the four analyses

All four are `pub` entry points re-exported from `src/fud/mod.rs`:

| Analysis | File | API | Spec |
| --- | --- | --- | --- |
| IV / recurrence detection | `src/fud/induct.rs` | `induct(cfg, chains) -> InductResult` | Wolfe Fig. 6.12 / §6.5 |
| Constant propagation | `src/fud/constprop.rs` | `constant_prop(cfg, chains) -> ConstPropResult` | §6.4 / §4.1 |
| Scalar dependence testing | `src/fud/scalardd.rs` | `scalar_dd(cfg, chains) -> ScalarDD` | Wolfe Fig. 6.21 / §6.7 |
| Trip count | `src/fud/tripcount.rs` | `trip_count(init, step, bound, bound_step) -> TripCount` | Wolfe §6.4.3 |

All four take a φ-`annotated` CFG plus `build_fud_chains(cfg) -> (CFG,
FudChains)`, so the driver is always `build_cfg_from_loop` →
`build_fud_chains` first, then the analyses over its output — the same entry
point the checker's §4.3 arms and the Phase 9a emitters (§5) use. Before Phase
9a `build_cfg_from_loop` produced a three-block CFG with no back edge and no
scalar uses/defs, so none of the four had anything to chew on a real loop; that
builder now emits the five-block loop shape with an IV initialisation, a guard,
a latch increment and the back edge (`src/fud/cfg.rs:1-45`), which is what made
`induct` and `scalar_dd` usable on production input.

Notes on design choices that the spec left open:

- `induct` runs Wolfe's algorithm over the φ-annotated CFG's *chain graph*
  rather than the raw CFG, so it composes with `placement`/`chain` exactly as
  §4.1 requires and needs no second copy of the def/use numbering.
- `scalar_dd` is the "scalar" half of DD only: it emits the four §6.7.1
  situations (`Zero`/`One`/`AnyBefore`/`AnyAtOrBefore`) per `(def, use)` pair.
  Vector/`ZTest` distance testing on array subscripts is M2B's existing path and
  is untouched.
- `trip_count` is pure arithmetic on `(a, b, bound, bound_step)` and returns
  `Option`s: an uncountable loop (`b == bound_step`, never satisfying) and an
  overflowing count are both `None`, never a guessed number.
- `constant_prop` is a **meet-semilattice over the CFG** on the three-point
  chain `⊤ → Const(c) → ⊥` (Wolfe §6.6.1): `⊤` is no information, `Const(c)` is
  provably that value on every execution, `⊥` is over-defined. The meet of two
  *different* constants is `⊥`. `eval_phi` meets only operands that actually
  reach the φ, so a value defined on a path that cannot reach the merge cannot
  poison it. Height three means the worklist fixpoints in ≤ 3 × |ops| updates.
- `constant_prop` is **not consulted by `scalar_dd`** — grep confirms zero
  references between them — and has **no caller outside `src/fud/mod.rs`'s
  re-export** (`grep -rn constant_prop src/ | grep -v fud/constprop.rs` returns
  only that re-export). Const-prop is built, tested and certified-shaped but
  currently consumes nothing and produces nothing in production. The dependence
  facts the certificate asserts are derived from the chains and the reaching-def
  structure alone. That is the conservative direction: a const-prop bug cannot
  produce a wrong dependence, only a missed optimisation. Wiring it in as a
  pruner is Phase 5/6 work, not Phase 4.
---

## 3. §4.3 — certificate and checker wiring

Two new claim kinds and two new witness kinds, each verified by the **real**
`m0-check` binary:

| Certificate | Checker does | Rejects |
| --- | --- | --- |
| `Claim::ScalarDependence` + `Witness::ScalarDependenceEvidence { cfg, asserted_def, asserted_use }` | `cfg.validate()` → bounds-check both ids → claim/evidence must name the *same* link → rebuild chains → re-run `scalar_dd` → the `(def, use) → distance` triple must be in the re-derived `situations` table | malformed CFG, out-of-range ids, claim/evidence disagreement, mismatched claim kind, any distance the rebuild does not produce |
| `Claim::TripCount { count, exit_value }` + `Witness::TripCountEvidence { cfg, iv_var, loop_, bound, bound_step }` | `cfg.validate()` → rebuild chains → re-run `induct` → require `iv_var`/`loop_` to be a **reported** IV → recompute `trip_count(iv.a, iv.b, bound, bound_step)` → require `Some(count)` **and** `Some(exit_value)` | malformed CFG, a nominated variable that is not an IV, the wrong loop header, a `None` (uncountable/overflowed) recomputation, any count that disagrees |

The trust boundary is the same as Phase 3's: the certificate carries the CFG
(ground truth), and every derived fact is recomputed. `bound`/`bound_step` are
**inputs** to the recomputation, not trusted facts — a forged bound can only
make the certificate disagree with the checker, never forge agreement, because
`a` and `b` come from the checker's own `induct` run.

### Coverage gap found and closed

§4.3 asks for checker-arm tests. Grep showed that although both arms were
implemented, **no test anywhere constructed a `ScalarDependenceEvidence` or a
`TripCountEvidence`** — the arms were compiled and reachable but unexercised
end-to-end. Fixed with `tests/phase4_e2e.rs` (15 tests):

- Dependence: accepted at `(1)`; rejected for the wrong distance, for a CFG that
  carries no such dependence (straight-line code), on claim/evidence
  disagreement, on an out-of-range use id, on a mismatched claim kind, and on a
  malformed carried CFG.
- Trip count: accepted and recomputed; rejected off-by-one (the §4.4 seed's exact
  error shape), on a forged count, on nominating a non-IV variable, on
  nominating the wrong loop, on an uncountable loop (`s += 0`), on a mismatched
  claim kind, and on a malformed carried CFG.

The CFG shapes and the facts the analyses produce on them were measured against
`src/fud` with a scratch probe test before the expectations were written, not
guessed (the probe has been deleted). Every negative test asserts on the
specific rejection message, so a rejection by an unrelated earlier gate cannot
make the test pass.

Those 15 tests drove the checker with **hand-written** certificates. The other
half of the gap — whether a real `m0-decide` run ever *produces* either claim
kind — is closed by the Phase 9a emitters and their suite, which is §5.

---

## 4. §4.4 — fuzzing and seeds

`tests/phase4_fuzz.rs`: 11 tests, all passing, ~9.8s in release.

The four drivers compare each analysis against an independent re-derivation over
random CFGs, and each is paired with a mutation that must be caught:

| Driver | Independent oracle | Seeded mutation |
| --- | --- | --- |
| `fuzz_induct_matches_analysis` | closed-form IV values walked from the chain structure | off-by-one base value `a` |
| `fuzz_constprop_matches_analysis` | fixed-point re-run on the same CFG | unsound meet of two different constants |
| `fuzz_scalar_dd_matches_analysis` | hand-computed carried facts | wrong carried distance (`One`→`Zero`) |
| `fuzz_trip_count_matches_enumeration` | brute-force iteration of the loop | `ceil` → `floor` |

Hardening added in the Phase 4 review:

- **`MAX_TRIPS` truncation is now loud, not silent.** The witness printer caps
  a trip list at `MAX_TRIPS = 8`. A silent clamp used to drop trip headers, so
  `Occ::trip_of` would answer `0` for the dropped occurrences and the distance
  check could read that `0` as a real trip index. Replaced with
  `assert!(trips.len() <= MAX_TRIPS, ...)`, keeping `let k = trips.len();` for
  downstream use. An invariant breach now fails the test instead of mis-witnessing.
- **Non-vacuity documentation corrected.** The floor is stated as a fraction of
  `CASES` with a hard `1,000` minimum; the comment now says plainly that lowering
  `CASES` makes non-vacuity *harder to prove by design*, rather than implying the
  floors auto-adapt, and records the measured counts so a future generator change
  is re-measured instead of re-padded.
- **`links_from` hardening** (`src/fud/scalardd.rs:596-634`). Two changes:
  the slot-per-predecessor lookup used to open with
  `if i >= links.len() { break }`, which on a chain shorter than the predecessor
  list silently abandoned *every* remaining edge — including the back edges that
  carry a loop's own definitions. That is a missing-dependence **unsoundness**,
  not a precision loss. It now `debug_assert!`s the slot count against the
  predecessor count, and a `link_matches_var` check skips (rather than trusts,
  in release builds) any slot that does not carry the φ's own variable, so a
  mis-built `FudChains` cannot smuggle another variable's definition into this
  loop's dependence set.
- **`constprop` contract tests** (`src/fud/constprop.rs`): the four unit tests
  pin the lattice (`meet_is_a_lattice_min`), the poison-on-`⊥` fold
  (`fold_adds_constants_and_poisons_on_bottom`), and the reachable-operand filter
  in `eval_phi` (`eval_phi_meets_only_reachable_operands`,
  `phi_meet_skips_operands_from_unreachable_blocks`).
- **Scalar-DD generator mix strengthened** toward loop-dense shapes, which is
  what produced the calibration numbers below.

### Seed-and-revert harness

`tests/seed_revert_phase4.sh` injects each of the four mutations, asserts each
fuzz driver reports violations, reverts, and asserts the drivers go quiet again.
Last run: **all four seeds fired, all four drivers clean after revert, and all
four analysis files byte-identical to their pristine copies.**

Hardening added in the Phase 4 review:

- `flock` guard on `/tmp/seed_revert_phase4.lock` — a second concurrent run is
  **refused** rather than corrupting the first (verified by launching two).
- Restore trap — seeds are reverted even if a step fails mid-run.
- Final **byte-for-byte** comparison against pristine backups via `cmp`. `git
  diff` is *not* sufficient here and the script says so: the Phase 4 analysis
  files are currently untracked, so `git diff` would report a clean tree even on
  a corrupted revert.

Measured violation counts (re-run with captured output):

| Seed | Violations reported |
| --- | --- |
| `constprop` | ~16.8k |
| `scalar_dd` | ~7.1k |
| `induct` | ~3.6k |
| `trip_count` | ~3.5k |

Calibration note: there is **one** non-vacuity floor, not four — 
`floor = (CASES / 2).max(1_000)`, i.e. 2,000 violations per oracle at the
current `CASES = 4_000` (`SEED_CASES` aliases `CASES`). The weakest seed
(`trip_count`, ~3.5k) clears it by 75%. The floor is a vacuity alarm, not a
regression target: if a generator change moves these numbers, the correct
response is to re-measure and restate them, not to pad the mix back out.
`TRIP_CAP = 3` and `WALK_CAP = 6_000` bound enumeration cost and are *not*
coupled to the floor — they cap how many claims a seeded oracle can refute, so
lowering them can sink a tally below the floor. Lowering `CASES` below ~1,200
cases puts the two arithmetic seeds (~0.88 violations per case) under the
`1,000` minimum, which is the intended reading: a sweep too small to produce
thousands of violations is too small to prove non-vacuity, and if it ever has to
run that small, the floor must be lowered with it, deliberately.
---

## 5. §4.3 at production scale — the evidence emitters and the array file format

The analysis layer is now a **producer**, not only a checker target.

| Piece | Where | What it does |
| --- | --- | --- |
| `LoopEvidence::from_loop` | `src/certificate/emit.rs:46` | `build_cfg_from_loop` → `build_fud_chains` → `induct`; keeps the CFG and the IVs `induct` proved |
| `trip_count_certificate` | `src/certificate/emit.rs:71` | Rebuilds the CFG from scratch rather than reusing the builder's, re-runs `induct`, and requires its own earlier result to agree with the rebuild before emitting; `count`/`exit_value` come from `trip_count(iv.a, iv.b, bound, bound_step)` |
| `loop_trip_count_certificate` | `src/certificate/emit.rs:127` | The same claim for a loop whose `ivs` already are its real range (the M0, M5-expanded and M8 paths) |
| `scalar_dependence_certificate` | `src/certificate/emit.rs:150` | Runs `scalar_dd` over the loop's CFG and claims the first carried `(def, use) → distance` it finds |
| `decide_evidence` | `src/decide/mod.rs:232` | Attributes the evidence to the level that understands the loop: M3's recursion, M5's expansion, M8's recurrence index, else M0 |
| `m0-decide` | `src/main.rs:41-53, 115-121` | Writes decision certificate(s) plus evidence. **No evidence → a bare object, byte-identical to the pre-9a output; with evidence → `[decision_cert, evidence...]`** |
| `m0-check` | `src/checker/main.rs:996-1030` | Accepts both shapes; checks `loop_hash` for **every** element; a list is a conjunction — one bad element rejects the file; an empty list REJECTs instead of accepting vacuously |

`tests/phase9a_evidence.rs` (20 tests) pins the producer side by driving the
real binaries and the real file format: each of the four attribution paths emits
its claim and binds the **original file's** hash (so `m0-check` verifies an M3 or
M5 certificate against the unmodified input loop even though the claim was
computed on the transformed loop); production output is accepted; decision
certificates precede evidence in the array; and every tamper target — a
corrupted trip count, a forged distance, a stripped CFG, a nominated non-IV, a
forged bound, a wrong hash on an evidence element, a bad element anywhere in the
array, an empty array — REJECTs with the message naming the gate it tripped.

`total_phase4_production_round_trip` is the same requirement swept over every
loop the repo ships: each is decided by the real `m0-decide` and every
certificate it wrote is verified by the real `m0-check`. It prints its own
non-vacuity tally — `swept 87 corpus loops: 87 with evidence … 0 rejected`, and
1640 loops with `M0_PHASE9A_WIDE=1` — so a later green run cannot be mistaken
for an empty one.

### A bug this integration exposed

M5's certificate for a **reverse pair** carried the wrong expansion: the fix
(`src/decide/m5.rs`) now computes expanded-base offsets for *both* accesses from
the original body order. It was invisible until the checker began verifying
every element of the array instead of only `all_certs[0]` — the certificate M5
emitted for the pair it happened to compute was checked, its mirror image was
not. That is the concrete argument for conjunction semantics.

---

## 6. §4.2 — M3/M5/M8 rewiring: one of three done, the other two blocked, for different reasons

`grep -rl crate::fud src/decide/` now returns **`src/decide/m3.rs` only**.

**M3 — done.** `derive_iv` (`src/decide/m3.rs:123-164`) is the rewiring §4.2
asks for: build the loop's CFG, `build_fud_chains`, run `induct`, take the
recursion variable's header φ, require `Class::LinearIv`, and **refuse the loop**
if the closed form disagrees with the declared recursion or if the variable
`induct` found is not the recursion's own. `recursion_to_iv` (`:173`) is a thin
wrapper over it; the hand-coded pattern match on the recursion shape is gone.
The spec's sketch assumed a builder that did not exist:

```rust
let cfg = build_cfg_from_recursion(loop_)?;
let chains = m0::fud::construct_chains(&cfg, ...)?;
let induct = m0::fud::induct(&cfg, &chains);
let iv = induct.ivs.iter().find(|v| v.var == r.iv_name_idx)?;
```

`build_cfg_from_recursion` still does not exist; `derive_iv` uses
`build_cfg_from_loop` over a *provisional* loop that expresses the recursion's
range as an IV (`src/decide/m3.rs:127-139`). Until Phase 9a that builder emitted
three blocks — `entry(0) → body(1) → exit(2)` — with no back edge and zero
scalar uses/defs, so `induct` returned `[]` on every input and the rewire was
not writable. The five-block shape (§2) is what made it possible, and M3 is
where it is used.

One remainder in M3 is worth naming rather than glossing: the guard's **bound**
still comes from the recursion metadata, not from the CFG. The builder lowers the
guard as `Guard { cond: ScalarExpr::Iv(k) }` (`src/fud/cfg.rs:101-104`), so the
comparison's other operand and its relation are not present as scalar uses any
analysis can read. `guard_range` (`src/decide/m3.rs:23-62`) therefore still
supplies `upper`. §4.2's "M3 no longer pattern-matches on the recursion shape"
holds for the IV and not for the bound.

**M5 — not rewired, and the spec's version is not expressible in this IR.** §4.2
asks M5 to treat a chase variable with a closed form as an IV and skip
enumeration. `PointerChase` carries a **literal `table: Vec<i64>`**
(`src/ir/mod.rs:63-74`), and the recurrence `p ← table[p]` is a lookup, not
arithmetic. `ScalarExpr` has only `Iv`/`Scalar`/`Const`/`Add`/`Mul` — **no
indexed load** — so the chase step cannot be lowered into the CFG at all; and
were it expressible, `induct` sends `ScalarRhs::Call(_)` to `Class::Complex`
(`src/fud/induct.rs:558`), which is precisely the non-linear case. The spec's own
fallback branch ("else fall back to the literal-chain enumeration") is therefore
the only one that can execute today. M5 does gain one real §4.2 artifact:
`decide_evidence` counts its expanded loop and emits a `TripCount` for it,
relabelled with the original file's hash (`src/decide/mod.rs:239-254`).

**M8 — not rewired, and the spec's premise is mistaken.** §4.2 sketches
`induct.ivs.iter().find(|v| v.var == recurrence_var)` and then
`trip_count(iv.a, iv.b, bound, 0)`, concluding "M8 no longer enumerates the
recurrence table". That conflates two objects. `induct` proves an *index
variable* linear in the loop counter (`value(i) = a + b·i`); M8's object is the
`RecurrenceId` sequence `x[k] = Σ coeffs[j]·x[k-1-j]` (`src/ir/mod.rs:389-392`) —
a linear **recurrence over values**, not a linear function of the IV. The IV M8
enumerates over is already in `loop_.ivs`, so there is nothing for `induct` to
discover, and `trip_count`'s iteration count is not what M8's decision needs: its
question is whether two recurrence *values* collide, which requires the closed
form of the sequence. M8's `ENUM_CAP = 4096` enumeration is doing work no §4.1
analysis performs. What M8 does gain is real `ivs` on its loop, and so a
`TripCount` certificate like any other counted loop (`src/decide/mod.rs:255-261`).

**So §4.2 is one of three rewired.** M5's remainder needs the frontend to emit
indexed loads — Phase 9 work. M8's remainder is **not** a Phase 9 blocker: it
needs a linear-recurrence solver (a closed form for
`x[k] = Σ c[j]·x[k-1-j]`), which is not among §4.1's four analyses and not in
Phase 4's spec at all. Recording either as "unfinished Phase 4 work" would be a
different kind of wrong from recording the M3 piece as done.

---

## 7. How to reproduce all of this

```bash
cd m0

# 497 tests
cargo test --release

# Phase 4, targeted
cargo test --release --test phase4_fuzz        # 11/11 (~10s)
cargo test --release --test phase4_scalardd    # 9/9
cargo test --release --test phase4_e2e         # 15/15 (real m0-check binary)
cargo test --release --lib fud::constprop      # 4/4
cargo test --release --lib fud::               # 54/54

# Phase 9a producer → file → checker round trip
cargo test --release --test phase9a_evidence                          # 20/20; sweeps 87 corpus loops
M0_PHASE9A_WIDE=1 cargo test --release --test phase9a_evidence \
    total_phase4_production_round_trip -- --nocapture                 # 1,640 loops, ~7s

# Seed-and-revert: all four seeds fire, then all four revert clean
bash tests/seed_revert_phase4.sh

# Harness suites — RUN THESE SERIALLY (see §8)
bash tests/run_real.sh           # 1001 loops, 100% coverage, 0 rejects
bash tests/run_all.sh            # 10/10 ACCEPT / 0 REJECT / 0 UNKNOWN
bash tests/run_negative.sh       # 11/0 (7 decision + 3 evidence + 1 baseline)
bash tests/run_m0_step_tests.sh  # 6/0
bash tests/fuzz.sh 3000          # RESULT: fuzz clean
```

---

## 8. Two harness hazards found and fixed

**(a) Concurrent suites corrupt each other's certificates.** Running
`run_real.sh` and `run_all.sh` **concurrently** produced 5 bogus checker REJECTs
on a corpus that is otherwise clean. Root cause: `m0-decide` writes `cert.json`
into its **working directory** (`src/main.rs:44`/`:50`), and both scripts used
relative paths from the same CWD, so one loop's certificate got checked against a
different loop. `tests/fuzz.sh` already avoided this by running the decider
inside a per-iteration temp dir; these two did not. Fixed in both: paths resolve
against the repo root via `BASH_SOURCE`, the decider runs inside a `mktemp -d`
dir cleaned by an `EXIT` trap, and the certificate is read from that dir.
Verified: identical results run concurrently and run serially.

**(b) The new certificate-array shape silently broke two harnesses.** The
sweep after the Phase 9a integration did not come back green, and the two
failures were real, not environmental:

- `tests/run_m0_step_tests.sh` went **3 PASS / 3 FAIL**. Its inline
  `verify_lattice` python did `cert.get("witness", {})` on a document that is now
  a *list* — `AttributeError: 'list' object has no attribute 'get'`, reported as
  a FAIL on the three EXACT loops. Fixed: it selects the element carrying the
  `IntSolution` witness before checking the lattice (§5).
- `tests/run_negative.sh` reported **8 pass / 0 fail — and that number was
  lying.** Each of its seven corruption scripts was an inline `python3 -` heredoc
  that did `c["claim"]…` on a list, raised `TypeError`, exited non-zero, and
  because the script has `set -u` but no `set -e`, left the *previous* artifact
  in `tests/negative/` untouched for `m0-check` to reject. The REJECTs were real
  rejections of stale files, so every case "passed" without testing its own
  corruption. The traceback was printed and ignored. Fixed: the patches now live
  in one helper, `tests/neg_tamper.py`, which names the certificate each
  mutation targets, fails loudly (non-zero, no output file, counted as a harness
  FAIL) if that certificate is absent, and rewrites the file in the shape it
  arrived in. The suite also gained the three evidence tampers the old script
  could not express — a wrong `loop_hash` on the `TripCount` element, a forged
  trip count, and a forged scalar distance — so it is now **11 pass / 0 fail**,
  and the rejections are for the field each case claims to corrupt:
  `REJECT: recomputed trip count … disagrees with the claim (1001, 1000)` and
  `REJECT: rebuilt ScalarDD does not report AnyAtOrBefore for def 1 -> use 0`.

Lesson worth keeping: `run_negative.sh` was the suite that *justified* the claim
"the checker is sound", and it was passing vacuously while the corpus suites
around it stayed green. A harness that prints a tally without printing what it
actually inspected is not evidence.

Not fixed, and worth knowing: the other `tests/run_*.sh` scripts still use the
relative `cert.json` pattern. Each is correct when run alone, which is how they
are documented to be used, but none is safe against a concurrent sibling. They
were left alone deliberately — touching them all is a wider change than a Phase 4
report warrants, and nothing here runs them in parallel.

---

## 9. Honest statement of scope

**Delivered and verified:**

- §4.1: all four analyses, re-exported from `fud`; 54 `fud::*` unit tests plus 9
  custom `phase4_scalardd` tests.
- §4.3: both claims, both witnesses, both checker arms — each arm re-derives
  every fact from the carried CFG, proven by 15 e2e tests against the real
  `m0-check` binary, **and now with a real producer**: `m0-decide` emits
  `TripCount` and `ScalarDependence` certificates for decided loops
  (`src/decide/mod.rs:232`, `src/certificate/emit.rs`), `m0-check` verifies
  every certificate in the file, both shapes (§5), and `tests/phase9a_evidence.rs`
  (20 tests) covers both the production round trip and the tampers.
- §4.4: four differential fuzz drivers with non-vacuity floors, a seed-and-revert
  harness with a concurrency lock and byte-exact restore verification, plus the
  `MAX_TRIPS` loudness fix.
- One of the three §4.2 rewires: M3's IV now comes from `induct` over the loop's
  own CFG, and a variable `induct` cannot prove linear is refused rather than
  assumed (§6).
- Corpus sweeps that cannot pass vacuously: `total_phase4_production_round_trip`
  asserts both zero checker rejections **and** that real evidence was produced
  (87/87 loops narrow, 1,640 wide), and prints its tally.
- No regression: M0's 1,001-loop corpus still certifies 601 EXACT / 400 NONE with
  0 UNKNOWN and 0 checker rejections; the Phase 1–3 suites all still pass;
  `fuzz.sh 3000` is clean.

**Not complete, and why:**

- **§4.2 is one of three rewired.** M3 consumes `induct`; M5 and M8 do not, for
  different reasons — the chase recurrence is not expressible in `ScalarExpr`
  until the frontend emits indexed loads (Phase 9), and M8's linear recurrence
  over *values* is not a class `induct` computes (§6). **Read strictly —
  "Phase 4 complete" requires M3/M5/M8 to consume `induct` — Phase 4 is not
  complete.** Read as "build and certify the analysis layer" (which is what
  §4.1/§4.3/§4.4 and most of the §4.5 estimate describe), it is. This report does
  not paper over which reading applies.
- M3's rewiring has a named remainder: the guard's **bound** still comes from the
  recursion metadata because the CFG guard carries only the IV (`§6`).
- `constant_prop` is built, tested, and **not called by anything** except its own
  re-export (§2). It is the one Phase 4 analysis with neither a consumer nor a
  producer path.
- `induct` and `scalardd` have **no in-file `mod tests`** — 0 `#[test]` in
  `src/fud/induct.rs` and `src/fud/scalardd.rs` — while `cfg` (13),
  `dominators` (9), `chain` (7), `placement` (6), `constprop` (4), `frud` (5),
  `reaching` (5), `tripcount` (5) all carry them. Their coverage lives in
  `tests/phase4_scalardd.rs`, `tests/phase4_fuzz.rs` and the Phase 9a suites
  instead. A divergence from house style worth flagging.
- §4.5's ~4,700-line estimate is not met. The §4.1 analysis files total 2,169
  (`induct` 745, `scalardd` 924, `constprop` 351, `tripcount` 149) and the four
  Phase 4/9a test files 3,027, against an estimate that assumed 900 lines of
  M3/M5/M8 rewiring — 600 of which were writable (§6) and 300 of which
  correspond to a `build_cfg_from_recursion` and an M8 closed-form solver that
  the spec never specified well enough to build.
- The harness scripts other than `run_real.sh` / `run_all.sh` still use the
  relative `cert.json` pattern and are only safe when run serially (§8).

**Correction of the earlier draft of this file.** It stated that the two new
claim kinds had "no production producer" and that `grep -rn fud src/decide/`
returned nothing. Both are **stale and were removed**: the emitters exist, are
wired through `src/main.rs`, and are exercised on the whole shipped corpus; and
`src/decide/m3.rs` now imports `crate::fud`. It also reported `run_real.sh` as
"601 ACCEPT" and `run_negative.sh` as "8 pass": the first conflated the EXACT
verdict count with the checker tally (the true figure is 1001 files ACCEPT,
0 REJECT), and the second was a vacuous pass (§8b), now 11 genuine rejections.



---

# Phase 5 + 6 status — subsystems, array regions, and the checker round trip

Date: 2026-10-03. Spec: `phase4,5and6.txt` §5 (11 subsystems) and §6 (4 analyses).

**Verdict: Phase 5's 11 subsystems and Phase 6's 4 array-region analyses are
implemented, wired through the real `m0-check`, and covered by standalone
feature, fuzz and end-to-end suites. §5.9 (runtime testing) is now split: the
certificate-verifiable half — the run-time predicate `NonStraddling || ByteEqual`
for `A[i]=A[i-K]+B[i]` — is implemented, emitted and re-derived by the checker
(`tests/phase5_runtime.rs`, 6 tests); only its data-conditional half (a runtime,
a format extension) remains an inert vocabulary stub (§14). Every other §5.1 and
§6.1 deliverable is derived by an analyzer and *re-derived* by the checker, so no
claim carries analyzer trust.**

## 10. The checker-replay discipline, restated

Every new claim carries the loop it was computed from in its witness
(`Witness::Phase5Evidence` for `Reduction`/`Aligned`/`LoopKind`;
`Witness::ArrayRegionEvidence` for `ArrayRegion`/`FunctionSummary`). `m0-check`
never trusts the analyzer's value: it rebuilds the loop's dependence set, re-runs
the reduction detector / loop-kind classifier / alignment solver / region
derivation / summary fold, and compares. An analyzer that mis-derives anything
produces a certificate its own checker rejects.

## 11. Phase 5 — subsystem inventory

| § | Subsystem | Module | Unit tests | Custom / e2e |
| --- | --- | --- | --- | --- |
| 5.1.1 | Storage association (row/col-major, `EQUIVALENCE`) | `src/ir/layout.rs` | 6 | ✓ |
| 5.1.3 | I/O dependence (`IoChannels`, `must_serialize`) | `src/ir/effect.rs` | 5 | ✓ |
| 5.1.4 | Purity lattice (`Pure`/`PureSubroutine`/`Impure`) | `src/ir/purity.rs` | 7 | ✓ |
| 5.1.5 | Symbolic pointer chase | `src/decide/m5.rs` (`decide_symbolic_chase`) | 2 | ✓ |
| 5.1.6 | Dynamic (parametric) bounds | `src/decide/m12.rs` (`decide_dynamic`) | — | ✓ |
| 5.1.7 | Cubic+ index degrees | `src/decide/m6.rs` (`MAX_DEGREE = 8`) | — | ✓ |
| 5.1.8 | Reduction detection | `src/ir/reduction.rs` | 5 | ✓ |
| 5.1.9 | Alignment (weighted union-find) | `src/fud/alignment.rs` | 5 | ✓ |
| 5.1.10 | Cross-loop distance (fusion) | `src/fud/crossloop.rs` | 4 | ✓ |
| 5.1.11 | Loop kind (`doall`…`sequential`) | `src/ir/loopkind.rs` | 7 | ✓ |
| 5.9 | Runtime testing (verifiable half) | `src/runtime/mod.rs`, `src/certificate/{mod,emit}.rs` | 6 (`phase5_runtime`) | ✓ (emitted-code half: **stub (§14)**) |

Supporting vocabulary: `src/ir/dependence.rs` (`Dependence`, and the new
`Dependence::unknown` flag that turns an undecided pair into a conservative
carried edge, so a classifier can never over-claim parallelism).

New claim kinds (`src/certificate/mod.rs`): `Claim::ArrayRegion`,
`Claim::FunctionSummary`, `Claim::Reduction { var, loop_, kind }`,
`Claim::Aligned(Vec<Alignment>)`, `Claim::LoopKind(_)`; witnesses
`Witness::ArrayRegionEvidence` and `Witness::Phase5Evidence`. Producers in
`src/certificate/emit.rs`; checker arms in `src/checker/main.rs`. §5.1.10 is
served by Phase 1's existing `Claim::CrossLoop` plus the new `crossloop` module
rather than a duplicated claim.

Source: the eight new analysis modules (`layout`, `effect`, `purity`,
`reduction`, `loopkind`, `dependence`, `alignment`, `crossloop`) total 1,762
lines; the §5.9 runtime stub adds 73; the new standalone tests add 1,489. A later
pass added §5.9's verifiable half (`Claim::RuntimeCheck`, `RuntimeCheckLeg`,
`emit::runtime_check_certificate`, the checker arm) and `tests/phase5_runtime.rs`.


## 12. Phase 6 — array-region analyses

| § | Analysis | Module | Custom tests |
| --- | --- | --- | --- |
| 6.1.1 | `MayUse` / `MayMod` / `Kill` dataflow | `src/region/dataflow.rs` | ✓ |
| 6.1.2 | Simple bounds (union / intersection) | `src/region/bounds.rs` | ✓ |
| 6.1.3 | Triplet notation `[l:u:s]` (exact union/intersection) | `src/region/triplet.rs` | ✓ |
| 6.1.4 | Array kill analysis (rank + coverage) | `src/region/kill.rs` | ✓ |

`src/region/mod.rs` documents a real deviation the spec does not: the scalar
`CFG` models scalar uses/defs only, so an array access lowers to a statement with
no uses and no defs — running the region dataflow over a `CFG` alone would return
empty sets that *look* like a proved kill. `RegionProgram` is therefore the flow
graph **with** the accesses attached, and `RegionProgram::from_loop` builds the
Phase 9a five-block shape. `loop_summary` is the whole-array triple that does not
need a graph at all. 2,017 lines.

## 13. Verification

| Suite | Tests | Result |
| --- | --- | --- |
| `cargo test --release` (all targets) | **611** | **0 failed** |
| `tests/phase5_features.rs` | 15 | ok |
| `tests/phase5_fuzz.rs` (differential + 5 seeds) | 6 | ok |
| `tests/phase5_e2e.rs` (real `m0-check`, accepts + tampers) | 8 | ok |
| `tests/phase6_features.rs` | 14 | ok |
| `tests/phase6_fuzz.rs` (differential + 3 seeds) | 4 | ok |
| `tests/phase6_e2e.rs` (real `m0-check`, accepts + tampers) | 5 | ok |

The fuzz suites assert soundness, not completeness, and every oracle carries a
`bug` flag: `seed_and_revert_reports_violations` proves each seed fires on a
deliberately-wrong oracle and goes quiet on the correct one, so no oracle is
vacuous. The e2e suites drive the real `m0-check` binary: a real certificate is
accepted and a forged one (wrong reduction kind, wrong loop kind, tampered
alignment, under-approximated region, empty summary) is rejected.

Harness suites (run serially — see Phase 4 §8):

| Script | Result |
| --- | --- |
| `tests/run_phase5_6_tests.sh` (**new**) | 11 PASS / 0 FAIL |
| `tests/run_m6c5_tests.sh` (**new**, degree-6 fixtures) | 2 PASS / 0 FAIL |
| `tests/run_all.sh` / `run_negative.sh` / `run_real.sh` | no regression |
| every `tests/run_m*_tests.sh` | exit 0 |
| `tests/fuzz.sh 3000` | `RESULT: fuzz clean` |

## 14. §5.9 runtime checking — the verifiable half is done

`src/runtime/mod.rs` keeps the *emitted-code* vocabulary (`RuntimeCheckKind`:
`DisjointArrays`, `NoDistanceK`, `GuardCheck`) for a future backend, but **no
analyzer constructs those and no checker arm accepts them** — a certificate
carrying a `test_loop`/`parallel_version`/`serial_version` shape with no runtime
to observe it would be exactly the unverifiable trust the certificate layer
forbids, so they remain inert.

What is now implemented is the half the replay architecture *can* check:
`Claim::RuntimeCheck { legs }` and `Witness::RuntimeCheckEvidence`, produced by
`emit::runtime_check_certificate`. For the canonical §7.6 shape
`A[i] = A[i-K] + B[i]` the emitter derives the run-time predicate
`NonStraddling || ByteEqual` (`RuntimeCheckLeg`) and the `m0-check` arm re-derives
the same legs from the loop on disk, rejecting any certificate whose legs, pair or
shape disagree. The claim is careful: it does **not** assert the accesses are
independent (they are not); it asserts that this is the run-time test that
licenses the versioned loop — a fact the checker re-derives. `tests/phase5_runtime.rs`
(6 tests) proves accept + four rejection modes end to end through the real binary.
What is still missing is the data-conditional half (a runtime to observe the test,
a format extension to carry the observation), which remains out of scope.

## 15. How to reproduce

```sh
cd m0
cargo test --release              # 611 passing, 0 failed
bash tests/run_phase5_6_tests.sh  # 11 PASS / 0 FAIL
bash tests/run_m6c5_tests.sh      # 2 PASS / 0 FAIL
bash tests/fuzz.sh 3000           # RESULT: fuzz clean
```

