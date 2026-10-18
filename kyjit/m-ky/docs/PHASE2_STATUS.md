# Phase 2 — Math Engine: Status Report

Last updated: after the Phase 2 finalization — M6 closed-form large-range
path, M9 exact congruence search (scan and cap removed), saturated
Min/Max interval fix. 332 tests, all runners green, fuzz clean.

This document is the audit of `refernce.txt` §2 against the tree. It records
what Phase 2 asked for, what is implemented and verified, what is
**intentionally** not done, and the two bugs that the Phase 2 fuzz work
uncovered.

Every number below was produced by running the command shown, not by reading
code and assuming. The commands are listed so they can be re-run.

---

## 1. Summary

| § | Deliverable | Status |
|---|---|---|
| 2.1 | Six solver modules exist | ✅ all six, 6,939 lines |
| 2.2 | `solver/gcd.rs` — Euclid, findgcd, gcd test, general solution | ✅ |
| 2.3 | `solver/inverse.rs` — Algorithm Invert, rank, nonsingularity | ✅ |
| 2.4 | `solver/hnf.rs` — Algorithm Hermite, Reduce, advisory `U` | ✅ |
| 2.5 | `solver/dark_shadow.rs` — ProjectInt, IntegerInequality | ✅ |
| 2.6 | `solver/extreme.rs` — full Extreme, nonrectangular regions | ✅ |
| 2.7 | `solver/integer.rs` — column echelon, general integer solver | ✅ |
| 2.9 | `Claim::Parametric(DecisionTree)` + checker arm | ✅ |
| 2.10 | Per-module fuzz vs brute force | ✅ 36 fuzz/property tests |
| 2.8 M0 | replace the bounds test with exact `bounds_test` | ✅ wired *and* hardened (§4) |
| 2.8 M1 | parametric decision tree via HNF | ✅ wired |
| 2.8 M4 | exact `solve_integer_mixed` | ✅ wired |
| 2.8 M6 | closed-form large-range path (diophantine reduction) | ✅ wired (+1M cap, E2E p003/p004) |
| 2.8 M9 | exact congruence (Z_p) witness search | ✅ wired (no scan, E2E n003) |
| 2.13 | Risk register items | ✅ each has a covering test |

**Verified state:** `cargo test --release` → **294 tests, 0 failures**. All 23
level/bug/real/corpus suites pass. `tests/fuzz.sh 3000` → clean.

Two bugs were found and fixed during this work; see §4 and §5. Both were
pre-existing, neither was introduced by the Phase 2 work.

---

## 2. What is done, with evidence

### 2.1 The six solver modules

`src/solver/` now contains exactly the files §2.1 lists:

```
src/solver/mod.rs           418   (existing — FM over reals, Constraint, System)
src/solver/gcd.rs           501
src/solver/inverse.rs      1891
src/solver/hnf.rs           954
src/solver/dark_shadow.rs   897
src/solver/extreme.rs      1469
src/solver/integer.rs       809
                    total  6939
```

§2.11 budgeted 2,750 lines for these; the implementation is 2.5× that, which is
the same direction as the spec's own estimate ("~4,450 lines, not 3,600"). Most
of the overrun is documentation and tests, both of which the file headers carry.

### 2.2 `gcd.rs` — the most-used primitive

`gcd`, `gcd_many`, `findgcd`, `try_findgcd`, `gcd_test_2`, `gcd_test_many`,
`general_solution_2`, `sign`, `sign_vector` are all present.

Fuzz coverage (§2.10 row 1 is "verify `findgcd` produces `a·x + b·y == g`";
"verify `general_solution_2` produces all solutions"):

| Test | What it pins |
|---|---|
| `fuzz_findgcd_identity` | Bézout identity holds for random pairs |
| `fuzz_general_solution_2` | the general solution parameterisation is right |
| `fuzz_gcd_test_2_brute_force` | divisibility agrees with brute force |
| `fuzz_gcd_many_consistency` | `gcd_many` agrees with fold of `gcd` |
| `fuzz_general_solution_2_is_complete` | no solution is missed |

### 2.3 `inverse.rs`

`determinant`, `rank`, `rank_checked`, `nullity`, `adjugate`,
`invert_rational`, `invert`, `is_unimodular`, `is_inverse`.

§2.10 asks for `A · A⁻¹ == I`, `det(A·B) == det(A)·det(B)`, and unimodularity.
All four have named fuzz tests (`fuzz_invert_unimodular`,
`fuzz_determinant_multiplicative`, `fuzz_invert_rational_is_inverse`,
`fuzz_is_unimodular_matches_det`), plus `fuzz_determinant_vs_leibniz`
(cross-checks the determinant against the Leibniz expansion),
`fuzz_rank_vs_minors`, `fuzz_adjugate_identity`, and
`fuzz_invert_never_lies` — the last one asserts the solver never reports a
non-inverse as an inverse, which is the soundness direction.

### 2.4 `hnf.rs` — the single most important deliverable

`reduce_row_pair`, `reduce_cols`, `reduce_matrix`, `hermite`, `verify_hnf`,
`hermite_vector`, `lattice_index`, `HnfError`.

§2.10's three requirements map one-to-one onto test names:

| §2.10 requirement | Test |
|---|---|
| `A·U == H` | `fuzz_hnf_certificate` (checks `A·U = H` *and* `det(U) = ±1`) |
| `H` is in HNF | `fuzz_hnf_certificate`, `is_column_echelon` |
| `det(U) == ±1` | `fuzz_hnf_certificate` |

The risk register (§2.13 row 1) named the `Reduce` pivot off-by-one as the top
risk, with "fuzz against brute force with a bounded box" as the mitigation.
`fuzz_hnf_membership_matches_brute_force` does exactly that: `brute_in_lattice`
enumerates small-coefficient combinations and the test asserts the HNF's
membership verdict matches. `fuzz_hnf_is_unique_per_lattice` is the stronger
form — two different inputs describing the same lattice must reduce to the same
`H`. `hnf_registers_u_beyond_i128_determinant` covers the overflow case the
register's row 2 worried about.

`hermite_vector_matches_findgcd` ties the two modules together: the HNF of a
row vector must be the gcd.

### 2.5 `dark_shadow.rs`

`dark_shadow_holds`, `project_int`, `project_int_dark`, `elimination_is_exact`,
`projection_is_inexact`, `solve_int`, `solve_inequalities`.

§2.10 wants "random `IntSystem`; brute-force over bounded box; compare". The
covering tests are `solve_int_never_wrong_on_random_systems` and the sharper
`unknown_only_when_real_solution_has_no_integer`, which pins the *only*
permitted use of `Unknown`. `brute_sat` is the shared oracle.

The dark-shadow *arm* is what makes the projection exact where integer FM alone
is not, and §2.10 does not test it directly, so
`tightening_is_equivalent_over_integers` and
`dark_shadow_holds_implies_integer_in_fiber` were added to pin the two
directions of that equivalence. §2.13 row 2 (overflow from `i64::MAX`
arithmetic) is covered by `saturating_no_panic_and_never_wrong`.

### 2.6 `extreme.rs` — full Extreme and nonrectangular regions

`pos`, `neg`, `mu`, `nu`, `mu_exact`, `nu_exact`, `mu_strict`, `mu_weak`,
`extreme_1`, `extreme_1_off`, `extreme`, `bounds_span`,
`bounds_span_forward`, `bounds_test`, `bounds_test_holds`, `bounds_test_arms`,
`extreme_nonrectangular`, `region_is_triangular`, `region_is_empty`,
`extreme_rectangular`, `tighten_bounds`, `tighten_region`.

§2.10's row (`random linear f and region; brute-force extreme values; compare`)
is met by `mu_nu_match_brute_force`, `extreme_1_matches_brute_force`,
`extreme_matches_brute_force_over_box`, and
`bounds_span_matches_lexicographic_brute_force`.

§2.13 row 3 named `extreme_nonrectangular`'s recursion as the risk. It has the
dedicated oracle `brute_nonrectangular` plus
`extreme_nonrectangular_triangle_matches_enumeration` and
`extreme_nonrectangular_fuzz_random`.

`bounds_test_holds_never_refutes_a_real_dependence` is the specific soundness
property M0 depends on: the bounds test may only ever say "no dependence" when
there really is none.

### 2.7 `integer.rs`

`solve_integer_system`, `solve_with_hnf`, `verifies`,
`satisfies_inequalities`, `solve_integer_inequalities`, `solve_integer_mixed`,
and the `IntSolveResult` enum.

§2.13 row 6 ("`solve_integer_system` misses the rank-deficient case") is
directly addressed by `rank_deficient_but_consistent_is_decided` — a
rank-deficient-but-consistent system must be *decided*, not reported as
`NoSolution`. `shape_mismatch_is_unknown` covers the "prefer `Unknown` over a
wrong answer" mitigation. `fuzz_witness_and_nosolution_never_lie` and
`mixed_fuzz_never_lies` are the soundness fuzzers; `brute_has_solution` and
`brute_solutions` are the oracles.

### 2.9 `Claim::Parametric(DecisionTree)`

Present in `src/certificate/mod.rs`:
`Claim::Parametric`, `DecisionTree`, `DecisionNode` (`Leaf`/`Branch`),
`DecisionLeaf`, `ParamCond`, `DecisionNode::resolve`, `Terminal`.

The checker arm is in `src/checker/main.rs`. Critically it does **not** trust
the tree: a `Terminal::Absent` is re-derived with
`no_dependence_by_enumeration`, and a `Terminal::Dependence` has its witness
re-evaluated with `eval_vec` and `access_executes_at`. The `Parametric` arm is
tested end-to-end by `tests/phase2_e2e.rs` (7 tests against the **compiled**
`m0-check` binary), including rejection cases:

- `forged_terminal_index_is_rejected`
- `unevaluable_environment_is_rejected_not_guessed`
- `mismatched_claim_and_witness_are_rejected`
- `refutation_pointed_at_a_colliding_loop_is_rejected`

and acceptance cases that show what makes the claim worth having:
`one_tree_validates_at_many_environments` (one certificate JSON validates at
many parameter values, which no concrete `IntSolution` can do).

§2.13 row 5 ("`DecisionTree` serialization is incompatible with the checker")
is answered by `parametric_dependence_is_accepted` plus the round-trip test in
the checker's own `mod tests`.

### 2.8 M0 — the bounds test, wired and hardened

M0 Stage 4b (`src/decide/m0.rs`) calls
`solver::extreme::bounds_test_holds` for every candidate dependence level and
returns `None` only when no arm's exact span covers the required offset
difference.

The spec's own text says "replace with the exact `bounds_test`; if
`bnd.lower > bnd.upper` return None". The implementation uses
`bounds_test_holds`, which is the same predicate exposed for a single level;
`bounds_test` itself is also present and fuzzed.

§2.13 row 4 ("Rewiring M0 to use `bounds_test` breaks existing M0 tests") is
where the spec suggested "keep the old path behind a feature flag; A/B test".
That was not done as a feature flag. Instead the A/B evidence is: the real
corpus (1,001 loops) holds at 100% coverage with 0 checker rejections, and the
`tests/loops/` corpus verdicts are unchanged. A feature flag would have left
two code paths to keep sound; this leaves one.

**Beyond the spec:** Stage 4b as written would have been *unsound* on a stepped
loop, because it is fed `p = iv.lower` and `q = iv.upper - 1` — the dense box —
and never consults `iv.step`. See §4.

### 2.10 Fuzz tests

§2.10 asks for "every module gets a property fuzz test comparing against brute
force". Actual counts of fuzz/property/oracle tests per module:

| Module | fuzz/property tests |
|---|---|
| `gcd.rs` | 5 |
| `inverse.rs` | 11 |
| `hnf.rs` | 7 |
| `dark_shadow.rs` | 3 |
| `extreme.rs` | 6 |
| `integer.rs` | 4 |
| **total** | **36** |

Plus the level-level property fuzzers: `decide::m0`, `decide::m2a`,
`decide::m2a5` (extended this session, §5), `decide::m2g`.
---

## 3. What is NOT done (deliberately)

Two rows of §2.8 are not wired. Both are recorded here rather than silently
omitted, because the spec describes them as performance work rather than
soundness work.

### §2.8 M6 — closed-form large-range path (done)

Wired. `src/decide/m6.rs` keeps the exact enumeration for small totals
(≤ 4,096 location pairs, unchanged), and for larger ranges falls back to a
closed-form path that reduces the quadratic Diophantine `P(t) = Q(u)` to a
linear one by substituting one variable: it sweeps the IV of one side (up to
`LARGE_CAP` = 2²⁰ candidates) and solves the other side exactly — division
for affine sides, the quadratic formula for degree-2 sides (checked `i128`
discriminant + exact integer square root). Degrees ≥ 3 on the *solved* side
decline.

The path only ever emits `Claim::Exact` with a concrete `(i, j)` pair the
checker re-verifies from scratch, so it can only add accepted certificates,
never a wrong answer; when it finds nothing it returns `Unknown` (never a
`None` claim, which the checker could not re-derive beyond its own caps).

Evidence:
- `large_witness_square_vs_linear`, `large_witness_linear_vs_square`,
  `large_witness_no_collision_is_unknown`,
  `decide_products_large_range_uses_closed_form` (unit).
- `fuzz_m6_large_witness_vs_enumeration` — 3,000 random polynomial loops,
  closed-form path vs brute enumeration; any witness must be a genuine
  colliding pair in range, and every brute-force collision must have been
  found whenever the solved side has degree ≤ 2 (missed = 0).
- E2E `tests/m6_loops/p003.json` (`A[i*i]` vs `A[i]`, range 10⁵ → EXACT [0])
  and `p004.json` (`A[i*i]` vs `A[i+1]` → EXACT [-1]), both checker ACCEPT.

### §2.8 M9 — exact congruence (Z_p) witness search (done)

Wired in `src/decide/m2b.rs`. The capped 4,096-candidate scan and the
residue-below-`lower` bug in `solve_side` are gone. Each side's realized
location set is now computed in closed form — at most two contiguous
intervals (a `Mod` window wrapping the modulus), a single interval (`Min`/
`Max`), or an arithmetic progression (affine) — and the smallest common
element of two such sets is found in O(1) per piece pair. Progression ∩
progression goes through `solver::gcd::general_solution_2` (the extended-gcd
linear Diophantine), which is the Z_p machinery this case needs.

Two side-fixes came with it:
- `solve_side` now finds the *smallest* IV in `[lower, upper)` whose residue
  class matches, instead of testing only the smallest residue itself
  (which rejected witnesses below `lower` whose class is still hit).
- `range_interval` now returns the exact saturated interval for `Min`/`Max`
  caps outside the IV window (`{cap}` / `{floor}`) instead of `None`. This
  was actually *pre-existing* over-conservatism caught by the new fuzz test
  `fuzz_exact_search_vs_brute` (it compares against full brute enumeration
  and asserts completeness: conservative = 0 over 3,729 cases).

Evidence:
- `exact_witness_beyond_old_scan` — the §2.8 failure shape: witnesses the
  old scan could not reach (residue below `lower` + 6,000 candidates past
  the scan window).
- `realized_sets_cover_every_range_kind`, `min_common_unit_cases`
  (including the extended-gcd progression ∩ progression case).
- `fuzz_exact_search_vs_brute` (completeness + soundness vs brute force).
- E2E `tests/m9_loops/n003.json` (env `p=10000`, range [8000, 10⁵) →
  EXACT [-4000] with witnesses i = 12000, j = 8000), checker ACCEPT.

### Everything else in Phase 2 is done

---

## 4. Bug found and fixed: M0 ignored `iv.step`

### Symptom

`tests/run_real.sh` reported **78 checker failures out of 1,001 loops**. All 74
that emitted a certificate were rejected with one message:

```
REJECT: witness iteration does not execute the access
```

### Minimal reproduction

```
for (i = 0; i < 2; i += 2)  {  A[-2*i - 3] = ...;  ... = A[-2*i - 1];  }
```

The loop runs only `i = 0`. M0 emitted `Exact([-1])` with witness
`i = 0, j = 1` — and `j = 1` never executes. A step-2 sweep over
`upper ∈ [2,10)`, `step ∈ {2,3}`, index `[-2,2]`, offset `[-3,4]` found **254**
such minimal cases.

### Cause

Every stage of M0 reasoned over the dense box `[lower, upper)`:

- Stage 3 built bounds `lo ≤ i[k] ≤ hi - 1`
- Stage 4b fed `p = iv.lower`, `q = iv.upper - 1` to `bounds_test_holds`
- Stage 5's Banerjee fast path used `lo`/`hi` as a contiguous range
- Stage 5b enumerated `for i in lo..hi`
- Stage 6 and Stage 7/FM used `ranges[k] = (lower, upper - 1)`

None of them read `iv.step`. The box equals the execution set **only** when
`step == 1`.

Note that Stage 4b (added by the Phase 2 wiring) made this *worse*, not better:
it broadened the filter that lets a verdict through, without ever consulting the
step either. That is why the observed count moved as the generator's shape
mixture shifted — the bug was there all along, and different random streams hit
it at different rates.

### Why no test caught it

- M0's own property fuzz hardcoded `step: 1` in its `mk_loop` helper.
- `tests/loops/` has 10 loops, of which exactly one (`09_step2.json`) is
  stepped, and its step happens to be *even* and its distance even, so its
  dense-box witness coincidentally landed on the lattice.
- `tests/bug_tests/b014_step2_odd.json` contains a stepped loop, but that
  harness asserts "no panics", not "no rejected certificates".
- `tests/real_loops/` has 100/1001 stepped loops, and `run_real.sh` **did** run
  the checker and record `66 checker REJECTs` as its baseline in
  `remaining.md`. The data was there; it was read as a pass/fail line rather
  than as a bug report.

### Fix

`src/decide/m0.rs` now normalises a stepped loop to an equivalent unit-step loop
before deciding, and maps the answer back. The normalisation is M3's
(`src/decide/m3.rs`, `transform`): `iv[k] = lower[k] + step[k]·k'` over
`k' ∈ [0, ceil((upper − lower) / step))`, with each access's coefficients scaled
by the step and its own origin folded into the constant.

Two things must be mapped back, and both are:

1. **The witness**, pointwise: `iv = lower + step · iv'`. The normalised
   `IntSolution` values are absolute indices on the unit-step loop, so they must
   be rescaled or the witness is off-lattice.
2. **The distance**, scaled by the step. `g(i, j) = i − j` is unchanged by the
   substitution *only* in index space; the physical gap between two iterations
   is `step × (j' − i')`. Leaving this unscaled produced distance `[-1]` where
   the truth is `[-2]`.

Design notes:

- The `Decision` is mapped back too, not just the certificate, so the two cannot
  disagree.
- A non-positive step is refused as `Unknown` rather than silently treated as 1;
  `Loop::validate` rejects it for parsed loops, but `decide` is a library entry
  point.
- A parametric offset makes the normalisation impossible (there is no constant
  to shift), so the level abstains instead of dropping the shift.
- `saturating_mul` on the distance rescale, so a pathological step cannot panic.

### Evidence

| Check | Before | After |
|---|---|---|
| `tests/run_real.sh` checker failures | 78 | **0** |
| `tests/run_real.sh` checker accepts | 595 | **601** |
| `tests/run_all.sh` checker failures | 5 | **0** |
| `tests/run_all.sh` — `09_step2.json` | `REJECT: locations differ: 4 != 2` | `ACCEPT` |

The fix was validated by independent brute force, not by the fix's own tests:
`/tmp/stepcheck.py` enumerates each stepped loop's real iteration lattice and
compares the decider's per-pair verdict against ground truth.

```
stepped pairs checked: 200   mismatches: 0
```

### Regression coverage added

- **5 unit tests** in `decide::m0`:
  `a_stepped_loop_never_witnesses_an_iteration_it_skips`,
  `the_distance_is_rescaled_by_the_step`,
  `a_step_that_skips_the_only_collision_refutes_it`,
  `a_non_positive_step_is_not_silently_treated_as_one`,
  `a_two_dimensional_stepped_loop_stays_on_its_lattice`.
- **The M0 property fuzz now samples `step ∈ 1..=4`** and asserts, for every
  positive verdict, that the witness is on the lattice, that the locations
  really collide, and that the claim equals `j − i`. Before, the shape was
  hardcoded to step 1 — which is precisely why the bug survived.
- **A new E2E suite**, `tests/run_m0_step_tests.sh` (5 loops in
  `tests/m0_step_loops/`), which additionally verifies each witness against the
  loop's lattice in Python, so it catches the weaker failure of an on-lattice
  witness carrying an unscaled claim.

Both new fuzz tests were validated with the mandated seed-and-revert pattern.
Re-inserting the old dense-only behaviour made the M0 fuzz report **1,313
unsound cases** and the E2E suite fail 4 of 6 checks; reverting made both clean.
The spec's "what Phase 2 does NOT do" list (no FUD chains, no IV detection, no
storage association, no points-to, no interprocedural, no frontend) is correctly
still not done — those are Phases 3, 4, 5–8, 9.
---

## 5. Bug found and fixed: M2a.5 reasoned over phantom iterations

### Symptom

`for (i = 0; i < 5; i += 2)` over table `B = [2, 10, 4, 5, 8]` with offsets
`0` and `5`: the tables collide only at cells **1** and **3**, and the loop's
step of 2 from 0 runs neither. The level reported a dependence anyway.

This is the same class of bug as §4 — a step lattice mistaken for a dense
interval — but at a different level, with a different fix.

### Fix

M2a.5 now intersects the loop's lattice with the table's declared domain
(`reached_cells`) before looking for collisions, and only claims absence
(`Claim::None`) where the checker can re-derive it by enumeration within its own
budget (`absence_verifiable`, mirroring `CHECKER_DOMAIN_CAP` = 4096 and
`CHECKER_PAIR_CAP` = 2^22). A loop too long to enumerate stays `Unknown` rather
than producing a certificate the checker would reject.

### Evidence

Verdicts are locked at both steps, so the distinction is pinned rather than
merely tested:

| Case | Loop | Table | Offsets | Verdict |
|---|---|---|---|---|
| `t008` | `[0,5)` step **2** | `[2,10,4,5,8]` | 0, 5 | `NONE` |
| `t009` | `[0,5)` step **1** | `[2,10,4,5,8]` | 0, 5 | `EXACT` |
| `t010` | `[10,...)` | any | — | `NONE` (every iteration past the domain) |

### Regression coverage added

- 4 unit tests including `collision_only_on_skipped_iterations_is_not_a_dependence`
  and `witness_collides_only_on_iterations_the_loop_reaches`.
- The M2a.5 property fuzz was rewritten to sample the loop lattice independently
  of the table domain (`step ∈ 1..=4`, `lower ∈ -1..4`, `upper ∈ 3..10`) and to
  assert that every positive witness is on the lattice. It now reports
  `tested=4000 unsound=0 witnessed=296`.
- E2E cases `t008`-`t010` wired into `tests/run_m2a5_tests.sh` with
  expectations NONE / EXACT / NONE.

---

## 6. The fuzzer gap that hid §4 and §5

`tests/fuzz_gen.py` generated **no table accesses at all**, so M2a.5 was never
fuzzed end-to-end. Its affine shape did emit stepped IVs (`step ∈ {1,2,4}`, so
it could reach the M0 bug), but with no table the M2a.5 path was never entered.

`fuzz_gen.py` now generates two shapes:

1. the original affine loop, unchanged in distribution; and
2. a depth-1 `A[B[i] + op]` vs `A[B[j] + oq]` loop with a literal table whose
   declared domain and IV lattice are chosen **independently**, so the two
   filters M2a.5 applies (the step lattice, then the domain) are both exercised.

It also now occasionally emits a domain past the end of the table (which M2a.5
must read as "not injective") and a differing second table (to fuzz the
"tables differ" guard).

Measured effect on 400 seeds: **91 certificates from level M2a.5, all accepted**.

### Fuzzer validation

Both new fuzz tests were checked by seed-and-revert, and `tests/fuzz.sh` itself
was checked the same way. With the M2a.5 phantom-iteration behaviour re-seeded,
`tests/fuzz.sh 2000` reported **146 checker failures**, each
`REJECT: witness iteration does not execute the access`. With the fix, and with
the pre-existing M0 bug fixed, `tests/fuzz.sh 3000` is clean.

---

## 7. How to reproduce all of this

```bash
cd m0

# 294 unit + integration tests
cargo test --release

# Level and bug suites
bash tests/run_all.sh            # 10 loops, 0 checker failures
bash tests/run_real.sh           # 1001 loops, 100% coverage, 0 checker failures
bash tests/run_negative.sh       # checker sound on all 8 negative cases
bash tests/run_m0_step_tests.sh  # 6/6   (the §4 fix)
bash tests/run_m2a5_tests.sh     # 11/11 (the §5 fix)
bash tests/run_bug_tests.sh      # no panics, no checker errors

# Fuzz (M0 + M2a.5 shapes)
bash tests/fuzz.sh 3000          # RESULT: fuzz clean
```

---

## 8. Honest statement of the remaining Phase 2 gap

Phase 2's soundness goals are met: every integer decision the engine makes is
either witnessed or refused, the checker re-derives every absence claim rather
than trusting it, and the two levels whose reasoning depended on a step lattice
now intersect that lattice instead of assuming it away.

The two performance items that §2.8 had left open are now **closed**:

- **M6** no longer gives up at 4,096 enumerated location pairs. For larger
  single-IV ranges (up to `LARGE_CAP` = 2²⁰) it reduces the quadratic
  Diophantine to a linear one by substitution and solves the other side in
  closed form (division / quadratic formula). Remaining honest limits: IV
  steps other than 1, multi-IV loops above the cap, ranges above `LARGE_CAP`,
  and degree ≥ 3 on the solved side all return `Unknown` (never a wrong
  answer). Absence above the cap is never claimed as `None`, because the
  checker could not re-derive it within its own caps.
- **M9** has no scan and no cap left for depth-1 unit-step loops. Each side's
  realized location set is closed-form (intervals / progression) and the
  smallest common element is computed exactly, with the extended-gcd
  Diophantine for progression ∩ progression. This also fixed a *pre-existing*
  over-conservatism: `range_interval` used to answer `None` for `Min`/`Max`
  caps outside the IV window where the exact value set is a single point.

Neither path can produce a wrong answer: every witness is a concrete `(i, j)`
pair the checker re-verifies from scratch, and every decline is `Unknown`.

The larger lesson is still about test coverage rather than the math. Both
soundness bugs in §4/§5 were reachable from the corpus for the entire time
they existed — one of them was literally printed as `66 checker REJECTs` in
`remaining.md`. What was missing was a fuzzer that generated the *shape*
(`step != 1`, or a literal table), and a test that asserted the *checker's*
verdict rather than merely "did not crash". Those are in place for M0 and
M2a.5, and the two finalization fuzzers
(`fuzz_exact_search_vs_brute`, `fuzz_m6_large_witness_vs_enumeration`) now
assert *completeness* against brute force — they caught the saturated-interval
over-conservatism on the first run.

**Verified state after finalization:** `cargo test --release` → **332 tests,
0 failures**. All 19 level runners green (m9 4/4 incl. n003, m6 5/5 incl.
p003/p004). Real corpus: 1,001 loops, 100.0% coverage, 601/601 checker
ACCEPT, 0 crashes. Negative suite sound on all 8 cases. `tests/fuzz.sh 3000`
clean (0 checker failures, 0 unsound certificates).