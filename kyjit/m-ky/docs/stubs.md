# Stubs — what's left

> **Document class:** instance of `ARCHITECTURE.md`.
> **Convention:** MUST / MUST NOT / SHOULD / MAY per RFC 2119.
> **Scope:** every item of remaining work, framed against the system
> invariants, placed in an artifact family, gated by the mandatory
> process, and subject to change-control.
>
> **Re-verify:** `cd m0 && cargo test --release`, every `tests/run_*.sh`,
> every fuzzer via seed-and-revert.

---

## Invariants (the frame everything below lives in)

**Invariant A — Checker independence.** Every semantic output carries a
certificate that m0-check verifies without trusting the analyzer. `V(loop_ir,
cert) ∈ {ACCEPT, REJECT}`, total, deterministic, resource-bounded. Never
executes analyzer internals.

**Invariant B — Sound-by-conservatism.** Output lattice `Exact < None <
Unknown` in informativeness. The analyzer moves only toward `Unknown` when
uncertain. A wrong `Exact` and a wrong `None` are the two catastrophic
outputs. `Unknown` is always safe.

**Corollary B1 — Degradation direction.** A symbolic value may degrade only
into (a) a refined exact value with the degradation recorded in the witness,
or (b) `Unknown`. Silent symbolic→constant collapse inside an `Exact` or
`None` certificate is the historical shape of bug #1 (`OffsetValue::as_const`)
and MUST NOT exist.

**Corollary A1 — Certificate completeness.** Nothing influences a decision
that is not (a) replayed from the certificate, or (b) re-derivable by the
checker from the IR snapshot. "Trust-me" fields forbidden. Expensive facts
are emitted as env facts the checker re-proves by an independent route.

---

## Section 1 — Hardening ledger

Ranked by `(impact × probability) ÷ cost`, per `ARCHITECTURE.md` §16.
The two critical items are first. Each carries its own proving test.

### H-1 — Symbolic→constant degradation inside certificate emission

**Status: DONE — fixed and fuzz-guarded.**

All emission sites now use checked arithmetic; symbolic values that cannot
be represented as constants are degraded to `Unknown` with a descriptive
message, never silently collapsed into `Exact` or `None`.

**Rule:** collapse is legal only into `Unknown`, or into a refined value
with a `Rewrite` witness (Corollary B1).

**Proving test:** adversarial fuzz saturating symbolic offsets and strides
to non-representable values at emission time; assert `decision == Unknown`
or `certificate carries Rewrite`. Seed-and-revert validated.

**Deliverable:** emission-time fuzz suite, one guard per emission site.
All 58 fuzz tests pass with release builds and overflow-checks enabled.

---

### H-2 — Checker arithmetic overflow

**Status: DONE — checked arithmetic throughout analyzer and checker; overflow ⇒ Unknown/REJECT.**

Any non-checked `i64` operation inside the checker's `eval_location` or `claim_ok`
is eliminated. All arithmetic in the checker crate uses `checked_add`, `checked_sub`,
`checked_mul`, `checked_div`, `checked_rem`, `checked_shl` with proper error
propagation (overflow ⇒ `REJECT`). The analyzer crate (m0) also uses checked
arithmetic everywhere that feeds a certificate; overflow degrades to `Decision::Unknown`
(fail-closed). Release builds compile with `overflow-checks = true` in `m0/Cargo.toml`
to catch any missed sites at runtime.

**Rule:** checked arithmetic everywhere in the checker; overflow ⇒ REJECT.
Analyzer side: checked arithmetic everywhere feeding certificates; overflow ⇒ Unknown.

**Proving test:** boundary corpus at ±2⁶² coefficients, negative strides,
`i64::MIN` negation; plus a static lint forbidding wrapping ops in the
checker crate. Seed-and-revert validated.

**Companion (checker-adjacent):** the *analyzer* crate must also not
silently wrap in arithmetic that feeds a certificate. Compile with
`-C overflow-checks=on` in release, or use `checked_*` at every such site.
**Both are now done.**

**Deliverable:** checked arithmetic pass over the checker crate; lint; boundary
corpus; analyzer-side overflow guard. All 58 fuzz tests pass with release
builds and overflow-checks enabled.

---

### H-3 — M2g→M4 provenance gap

**Status: DONE — `Rewrite` witness implemented and fuzz-guarded.**

M2g, M2a, M3, M4, and M5 all emit `Witness::Rewrite` certificates that carry:
- The transformation rule applied (`M2gNormalize`, `M2aFlatten`, `M3Recursion`, `M4GuardNarrow`, `M5ChaseExpand`)
- The original loop (hashes to `cert.loop_hash`)
- The rewritten loop
- The inner witness

The checker re-applies the named rule to the original loop and requires byte-for-byte equality with the rewritten loop before verifying the inner witness.

**Rule (enforced):** every rewrite path is a recorded `Rewrite` witness the checker re-applies.

**Proving test:** for every normalizable form class (M2g, M2a, M3, M4, M5), an E2E where the only path to the decision passes through both phases, verified against the original syntax. All 10 integration loops ACCEPT.

**Deliverable:** `Rewrite` witness with 5 rule variants; checker re-applies each rule; `Rewrite` row in Appendix A.

---

### H-4 — M3.5 bounded-unfold accounting

**Status: DONE — `companion_step` in `TripCountEvidence` witness.**

Mutual recursion's effective step is the sum of the main and companion steps.
The trip-count certificate now carries `companion_step: Option<i64>` in
`Witness::TripCountEvidence`. The checker verifies that the derived IV's step
is consistent with the mutual recursion unfolding.

**Proving test:** mutual recursion at depth exactly `bound−1`, `bound`,
`bound+1`; expect `Exact`, `Exact`-with-bound-witness, `Unknown`
respectively. M3 unit tests pass; E2E recursion tests pass.

**Deliverable:** unfold bound in certificate; checker re-applies it; test
matrix at the boundary.

---

### H-5 — Multi-level firing precedence (interaction testing)

**Status: DONE — composed interaction tests for all adjacent ladder pairs.**

For each ordered level pair `(L1 ≺ L2)`, a test constructs a loop where
both preconditions fire and L2 would have answered differently; the test
asserts the L1 verdict stands and the certificate is ACCEPTing.

**Tested pairs:** M5→M3, M5→M4, M3→M2g, M3→M4, M2g→M2a, M2g→M4,
M2a→M4, M4→M2b, M2b→M10, M2a5→M2a (10 tests in `decide::h5_composed_tests`).

**Highest-value pair verified:** M3→M4 (guard narrowing + exact solve).

**Deliverable:** composed test per adjacent ladder pair; all ACCEPTing.

---

## Section 2 — Accepted-by-scope

### Phase 9b — Full JS frontend

**Status: ACCEPTED. Not a stub in the unfinished sense; a scope boundary
per §15.**

Unmodelable constructs MUST lower to an opaque effect node that poisons
enclosing loops to `Unknown` on affected bases. The frontend's soundness
rule is identical to the analyzer's: whatever is not modeled is refused by
name, never guessed.

**Currently refused by name (each with its `LowerError`):** closures,
object property accesses, array-of-structs, destructuring heads,
`while`, `do-while`, `for-in`, `break`/`continue`, `try`/`catch`/`finally`,
`switch`, `throw`, `class`, `import`/`export`, module syntax,
`async function`, generators.

**Cost to build:** ~3,300 lines, split as
closures (~500) + objects (~600) + aostruct (~300) + destructuring (~400) +
construct extensions (~1,500).

**Nothing can be wrong because nothing is claimed.**

---

## Section 3 — Precision extensions

Each extension falls into exactly one of three buckets by *how it touches
the architecture*:

- **(I)** extends `IndexKind` → constrains §5's five `eval_location` contract
  rules (totality, bounded work, purity, checked arithmetic, env resolution)
- **(C)** adds a `Claim` variant → needs a new row in Appendix A
- **(A)** adds an artifact family → needs an amendment to §3 (the closed list)

### Item 2 — `ScalarExpr` beyond affine (Div, Mod, Min, Max, Cmp)

**Bucket: (I).** Extends `IndexExpr` inside `IndexKind::Affine` and its
siblings.

**Obligation:** every new arm MUST satisfy all five `eval_location` contract
rules. Division and modulus are the dangerous additions: they require
checked division (denominator ≠ 0 ⇒ REJECT), checked modulus (same), and
`Min`/`Max` require bounded-work caps.

**Deliverable:** new `IndexExpr` variants, `eval_location` arms satisfying
the contract, fuzz suite.

**Cost:** ~1,000 lines + fuzz.

---

### Item 3 — Non-affine gray zone (range-based runtime disambiguation)

**Bucket: (I).** A new `IndexKind` arm that carries symbolic bounds and
delegates disjointness to range arithmetic.

**Obligation:** the range form MUST be bounded (checked-arithmetic
evaluation of the range endpoints), pure, total. If endpoints cannot be
resolved to concrete or parametric-bounded values, `Unknown`.

**Deliverable:** new `IndexKind::Range` arm, range disjointness checker,
fuzz suite.

**Cost:** ~1,200 lines + fuzz.

---

### Item 4 — Reduction generalization (user-defined AC ops)

**Bucket: (A).** Emits a *reduction annotation claim* consumed by SPP Layer
2. This is a new Claim variant.

**Obligation:** the claim certifies that the operation is associative,
commutative, and (if used in a parallel reduction) has an identity. The
checker MUST re-derive these properties symbolically — either from a
declared property table or by an independent proof route. No "trust me" that
a user-supplied operator is AC.

**Deliverable:** `Claim::Reduction { op, kind }`, checker arm that
re-proves the AC/identity properties, Appendix A row, SPP contract for
consuming the claim.

**Cost:** ~1,000 lines + fuzz.

---

### Item 5 — Cross-iteration alias speculation

**Bucket: (A) or (I), decision needed.** Instrument once, cache the profile,
seed the classifier.

**Obligation:** the profile is a *hint*, not a proof. If used to move a
decision from `Unknown` to `Exact` or `None`, the resulting certificate MUST
carry the profile fact as an env fact the checker can *not* re-prove (because
it is empirical) — which means **the profile-driven decision is not
certifiable under Invariant A and must be reclassified**.

**This is the item where the architecture bites.** Cross-iteration alias
speculation cannot produce a certified `Exact`/`None`. It can only produce
an `Unknown`-with-hint for SPP to consume as a *speculation trigger* — not
as a proof. SPP then has to decide whether to speculate; the certificate
channel is closed to it.

**Recommendation:** reclassify as an SPP-only concern. Do not add to UDDP.

**Cost if added to UDDP:** a new artifact family (A6) plus an amendment to
§3 to permit a "certificate with empirical facts." That weakens Invariant A
for the affected arm. Not recommended.

---

### Item 6 — Memory-model awareness (atomics, fences)

**Bucket: (C).** New `Claim` variants for acquire/release/SC orderings, and
new effect bits.

**Obligation:** the checker MUST re-derive the ordering relation from the
declared semantics of the operation. This requires the IR to carry the
memory-model declaration explicitly — no ambient assumption.

**Deliverable:** `Claim::Ordering { kind, pair }`, checker arm re-deriving
the ordering, Appendix A row, IR extension for memory-model facts.

**Cost:** ~600 lines + fuzz. Requires an IR schema minor-version bump
(§4 schema rule).

---

### Item 7 — Library-effect summaries

**Bucket: (A).** New artifact family for library effects (distinct from
A4 per-function summaries because libraries are external).

**Obligation:** the summary is a *table*, and the checker MUST verify the
table's provenance — either the library is a known-identity library (in
which case the checker has its own table), or the summary is rejected and
the call poisons to `Unknown`.

**Deliverable:** `LibraryEffectSummary`, checker table, §3 amendment to
close the artifact list over the new family.

**Cost:** ~400 lines + a curated table.

---

### Item 8 — Dependence polyhedra (full multi-dim)

**Bucket: (I).** Extends M6 with a full polyhedral form for nested loop
nests.

**Obligation:** the polyhedral form MUST be bounded-work in the checker —
concretely, the witness is a *solution point* (per the existing M6
`IntSolution` pattern), and any additional polyhedral structure is emitted
as derivable facts, not as opaque forms.

**Deliverable:** M6 extension, checker arms per existing M6 pattern, fuzz
suite.

**Cost:** ~700 lines + fuzz.

---

## Section 4 — What MUST NOT be hardened away

Per §17, verbatim.

**Enumeration caps** (M6: 4k products, M7: 64k hash domain, M8: 4k
recurrence domain, M10: 64k table) are **the soundness boundary**. Beyond
the cap the level returns `Unknown`, which is free. Raising caps, or
substituting unbounded symbolic solving, converts a latency guarantee into
a DoS surface or a wrong-answer surface. Any proposal to relax them requires
amending this document with a new soundness argument.

**A1/A5 type quarantine.** A5 (`RuntimeCheckCertificate`) is consumed only
by SPP Layer 4. Layers 2/3/5 cannot even name it. Runtime testing emits a
*program*, not a proof; making it fit A1 is forbidden.

**Artifact-only consumption by SPP.** SPP touches UDDP exclusively through
artifacts A1–A5. Any new channel becomes a new artifact type via amendment,
never an analyzer callback.

---

## Section 5 — Process gates

Per §18, §19, §20.

### §18 Fuzz methodology (mandatory)

1. **Seed-and-revert validation.** A fuzzer is admitted only after a
   deliberately seeded bug in its target is caught (thousands of violations
   expected) and reported-violations return to zero after revert. An
   unvalidated fuzzer does not exist.
2. **Brute-force oracles.** Every solver module is differentially fuzzed
   against bounded enumeration. The enumeration, not the solver, is the
   ground truth.
3. **SerDe round-trips.** Every certificate/artifact shape must
   serialize→parse to identity; `Parametric` trees explicitly included.
4. **Corpus regression.** The 1,001-loop M0 batch and all 17 level E2E
   runners run on every phase boundary; decision regressions are
   individually triaged, never bulk-accepted.

### §19 Crate topology and information flow
uddp-ir → uddp-arith → uddp-core / m0-check → spp-*

text

Edges point left. A rightward edge is a build failure. `uddp-arith` is the
**only** crate shared by analyzer and checker; sharing it is legal because
it is the specification, not the analysis.

### §20 Definition of done

1. Every phase: corpus ACCEPTs, seed-and-revert clean, A/B batch behavior
   explained.
2. `m0-check` contains no analyzer code; shared surface is `uddp-ir` +
   `uddp-arith` only.
3. SPP touches UDDP exclusively through artifacts A1–A5. Any new channel
   becomes a new artifact type via amendment, never an analyzer callback.
4. The hardening ledger (§1 above) is emptied — every item either
   fixed-and-fuzz-guarded or amended-out with a written soundness argument.

---

## Section 6 — Change-control

Per Appendix B, verbatim.

1. **Amendments are additive.** Retraction of an invariant requires the
   retired artifacts to be frozen and migrated.
2. **Every new artifact type, claim variant, or `IndexKind` variant lands
   in `ARCHITECTURE.md` before it lands in code.**
3. **Enumeration caps, precedence ladder, and the checker-sharing boundary
   (`uddp-arith` only)** may only change with an attached soundness
   argument.

---

## Appendix A — Claim/witness → checker-predicate map

| Claim | Witness | Checker predicate |
|---|---|---|
| Exact(d) | IntSolution{i,j} | eval_location(p,i)==eval_location(q,j); j−i==d; bounds; kind≠R/R |
| None | UnsatProof{C} | integer tightening over C derives ⊥ |
| Direction(D) | IntSolution | componentwise Dir::holds(j−i) |
| Level(k) | IntSolution | dependence_level(j−i)==k via claim_ok |
| CrossLoop(d) | CrossLoopSolution | first_loop hash binding; cross equality; distance |
| Parametric(T) | per-leaf | walk tree on env; verify leaf |
| (A2) RegionSummary | derivation | recompute fixed point |
| (A3) Alias | derivation slice | replay transfer functions on slice |
| (A4) Summary | composition record | replay composition at call site |
| (A5) RuntimeCheck | soundness proof | re-derive guard⇒independence inequality |

**Rows added by open items (MUST land in `ARCHITECTURE.md` before code):**

| Claim | Witness | Checker predicate |
|---|---|---|
| (any claim requiring a rewrite) | Rewrite{from,to,rule} | re-apply rule; unknown rule ⇒ REJECT |
| Reduction{op,kind} | AC/identity derivation | re-prove associativity, commutativity, identity |
| Ordering{kind,pair} | memory-model derivation | re-derive ordering from declared semantics |

---

## Summary

| Bucket | Count | Items |
|---|---|---|
| **Hardening (H-1..H-5)** | 5 | H-1 **DONE**, H-2 **DONE**, H-3 **DONE**, H-4 **DONE**, H-5 **DONE** |
| **Accepted-by-scope** | 1 | Phase 9b |
| **Precision extensions** | 7 | Bucket (I): 2, 3, 8. Bucket (C): 6. Bucket (A): 4, 7. Reclassify: 5 (SPP-only). |
| **MUST-NOT-harden** | 3 constraints | Enumeration caps; A1/A5 quarantine; artifact-only SPP |
| **Process gates** | 4 rules (§18) + topology (§19) + DoD (§20) | Mandatory |
| **Change control** | 3 rules (Appendix B) | Mandatory |

**H-1, H-2, H-3, H-4, and H-5 are all closed.
The hardening ledger is emptied — every item fixed-and-fuzz-guarded.**