# Phase 8 status — Interprocedural Analysis (Wolfe Chapter 8.5, with §8.3, §8.4)

Date: 2026-10-03. Spec: `phase7,8and9.md` §8 (`refernce.txt` §8.3–§8.5, §8.11).

**Verdict: Phase 8 is complete and verified.** All ten modules of §8.1 exist (plus
an eleventh, `program.rs`, that the spec's list does not name but its signatures
require). Call-graph construction with argument-flow triplets, I/O summaries,
interprocedural MOD, purity, the constant-propagation lattice, bottom-up
summaries, top-down context refinement, call-site composition and the certificate
replay are implemented, wired through the **real** `m0-check`, and covered by
standalone feature tests, differential fuzz against a concrete interpreter, and
end-to-end accept/reject round trips.

**Integration is complete as of 2026-10-10.** The `ProcSummary` now carries Phase
6 region and Phase 7 points-to effects (§8.7 cross-domain fields), and the array
IR has a `Call` statement kind (`StmtKind::Call`) enabling loops with calls to
be represented and decided. The `constant_prop` caller is wired in the decider
and results flow to M4 and the interprocedural lattice. Every Phase 8 claim
emitted by `m0-decide` is re-derived by `m0-check` from the carried
`IpaProgram`.

---

## 1. Module inventory

§8.1 lists ten distinct modules (eleven lines — it names `mod.rs` twice): the
public API plus nine analyses. We ship those ten **plus** `program.rs`, which the
list omits but every signature in it requires. The § column below follows the
numbering the code's own header comments use, in which purity is §8.4 and I/O is
§8.3; the spec's document headings swap the two (8.3 = purity, 8.4 = I/O).

| § | Module | Lines | Unit tests |
| --- | --- | --- | --- |
| 8.2 | `src/ipa/callgraph.rs` | 407 | 6 |
| 8.6 | `src/ipa/constprop.rs` | 662 | 11 |
| 8.4 | `src/ipa/pure.rs` | 305 | 7 |
| 8.5.2 | `src/ipa/mod_analysis.rs` | 289 | 5 |
| 8.7 | `src/ipa/summaries.rs` | 288 | 4 |
| 8.8 | `src/ipa/context.rs` | 245 | 5 |
| 8.9 | `src/ipa/compose.rs` | 120 | 4 |
| 8.3 | `src/ipa/io_effects.rs` | 213 | 3 |
| 8.10 | `src/ipa/certificate.rs` | 293 | 5 |
| — | `src/ipa/program.rs` | 253 | 0 (validated directly, used by all) |
| — | `src/ipa/mod.rs` | 23 | 0 |

Source: **3,098 lines** across the eleven modules. Standalone tests:
`tests/phase8_features.rs` (59), `tests/phase8_fuzz.rs` (20),
`tests/phase8_e2e.rs` (8), plus **50** `src/ipa/*` unit tests. The three test
files total 3,146 lines.

`program.rs` exists because the spec's §8.2/§8.5/§8.7 signatures all take a
`FunctionTable` that §8.1 never lists as a module. It is that table and its
invariants: procedures with their formals, locals and return expression, the
shared globals, the three statement kinds (`Assign { target, value }`,
`Call(Call)`, `Io { channel, kind }`), the expression grammar (`Read`/`AddrOf` of
a `Slot::Param`/`Slot::Local`/`Slot::Global`, literals, `Add`/`Sub`/`Mul`,
`FnAddr`), direct and indirect callee position, and `validate()`, which rejects a
malformed program before any analysis runs — the same guard Phase 7 uses, so a
certificate cannot be built over a program its own checker would refuse.

---

## 2. The trust boundary, restated

A Phase 8 certificate is `Claim::Interprocedural(IpaClaim)` witnessed by
`Witness::IpaEvidence { program }`: the claim names one fact, the witness carries
the **whole interprocedural program**, and `m0-check` (the `IpaEvidence` arm of
`src/checker/main.rs`) re-runs the relevant analysis and re-derives the fact.
Nothing the analyzer concluded crosses the boundary — the re-derived answer is a
function of the carried program alone, so a forged claim cannot be promoted into
an accepted certificate. The emitter, `interprocedural_certificate`, runs
`check_ipa_claim` itself first and returns `None` when the checker would reject,
so an unsound claim is never even written down. This is the same discipline
Phases 6 and 7 follow, and the same one deliberate strengthening of the spec's
witness types.

The five replayable facts cover the analyses of §8.3–§8.7:

| `IpaClaim` | Re-derived by |
| --- | --- |
| `Purity { proc, purity }` | `pure::classify` over the SCC-condensed call graph |
| `ModSet { proc, symbols }` | `mod_analysis::analyze` (binding graph) |
| `FormalValue { proc, formal, value }` | `constprop::analyze` (jump functions) |
| `Summary { proc, read, write }` | `summaries::compute` (bottom-up fixpoint) |
| `IoOrder { proc, first, second }` | `io_effects::must_serialize` |

The claim vocabulary differs from the spec's §8.10 sketch, and the difference is
the same trust-boundary strengthening Phase 7 made. The spec proposes
`Witness::InterprocWitness { binding_graph, jump_functions, scc }` — carrying the
*structures the analyzer derived* — and four claims: `Summary` (carrying a whole
`ProcSummary`), `CallEdge`, `Purity` and `ModArg`. Carrying a derived binding
graph or SCC would put the analyzer's conclusion inside the certificate, which is
what this crate refuses everywhere else, so we carry the `IpaProgram` and
recompute. `IpaClaim` accordingly covers the spec's `Purity` and `Summary`
(decomposed to explicit read/write sets rather than an embedded `ProcSummary`, so
there is no structure to re-validate), adds `ModSet`, `FormalValue` and `IoOrder`,
and does **not** add the spec's `CallEdge` or `ModArg`: call-graph membership is
not asserted as a claim, and per-position modification is subsumed by `ModSet`,
which names the set of slots a call to a procedure may write. The call graph is
still verified — by the fuzz (`the_call_graph_sees_every_call_the_run_made`) and
by `callgraph_of` being the shared entry point the checker rebuilds through — just
not as a certificate the decider emits.

`IpaClaim` derives serde with adjacently-tagged `AbstractValue`/`JumpFn`, because
serde cannot put an internal tag on a newtype variant like `Const(i64)` — a
certificate carrying a formal value must round-trip through JSON.

---

## 3. Soundness decisions worth naming

- **MOD propagates through bindings, not through names.** A callee that writes
  its formal `k` has written whatever `k` was bound to in the caller. The binding
  graph is walked transitively, so a write visible in `b` surfaces two frames up
  in `main → a → b`, and globals are vertices untied to any one procedure because
  they are shared. Dropping that propagation is one of the seeded bugs and the
  fuzz catches it.
- **Purity is pessimistic and fixpoint-solved.** `PureSubroutine` is not an
  endpoint: a routine that is read-only *given* its callees' purity only becomes
  `Pure` once they do, so classification runs bottom-up over the SCC condensation
  instead of one pass. A `PureSubroutine` that writes through its third parameter
  makes `Impure` every caller that passes a global there. Recursion means a
  procedure can be its own indirect callee, which is why read/write sets and
  purity are solved as a fixpoint over the call graph rather than by the single
  reverse-topological sweep §8.7 sketches.
- **Indirect calls resolve through argument flow.** `callgraph.rs` builds both
  the textual edges `p → q` and Wolfe's triplets `⟨q, k, p⟩` — procedure `q`
  passed as the *k*-th argument into `p`. An indirect call through a formal
  resolves to the union of the procedures *ever* passed at that position, which
  is sound without any points-to information. A graph built from textual edges
  alone is a seeded bug.
- **`Top` is the identity of `meet`, which makes it dangerous at a call site.**
  A value of `Top` silently preserves an entry constant, so a write whose content
  the lattice cannot name must yield `NonConst`, never `Top`. See §4.
- **Context refinement only projects.** `context::refine` walks the call graph
  downward from the entry, deriving each callee's `ContextKey` from its argument
  *shapes* — `Const(c)` for a literal, the caller formal's own shape for
  `Read(Param(j))`, `Unknown` for anything else — and clones the summary per
  distinct context up to `MAX_CONTEXTS = 8`, past which the remaining contexts
  fall back to the unrefined bottom-up summary. A refined summary is therefore
  never more optimistic than the merged one it came from, which is what makes the
  cap safe; the fuzz asserts exactly that, and that the cap holds.
- **Composition at a call site translates symbol spaces.** `compose.rs` maps a
  callee formal to the actual expression bound at position `j`, and a global to
  itself, because globals are shared and that sharing is the entire reason this
  analysis is interprocedural. A callee local never translates — it cannot
  escape — and an argument the model cannot name translates to nothing, which
  drops the effect rather than inventing one.

---

## 4. The bug the fuzz found, and what it cost

`tests/phase8_fuzz.rs` compares each analysis against a concrete interpreter that
*runs* the generated program. Programs are generated as a DAG — a call may only
target a higher-indexed procedure — so the interpreter terminates without a depth
cap. Storage bindings follow the model's own rule: an actual that is a plain
`Read`/`AddrOf` of a slot aliases the caller's cell (call-by-reference, which is
what the binding graph encodes); anything else gets a private cell.

The oracle attributes callee effects **by binding, never by value-diffing a
storage cell**. When a callee writes through a formal, the fact is pushed upward
through the actual that bound it, in the caller's own naming: `&g` makes it a
global write, `&x` a write of the caller's formal, `&t` a write of a local, and a
literal binds nothing so the write stops inside the callee.

Two bugs came from getting that wrong, and neither shows up at small case counts:

1. **The oracle over-attributed.** Diffing a cell's value before and after a
   frame runs, or following alias chains such as `p0 → p1(&G0) → p3(&G0)`,
   reported a callee as having modified slots it never wrote through. Two actuals
   can name one cell (`&x, &x`), so a write to the second formal was reported
   against the first too. A diff oracle also disagrees *structurally* with every
   analysis it checks, because those all name slots, never cells. The rewrite is
   write-through-binding, documented in the header of `tests/phase8_fuzz.rs`;
   `Extent::merge_callee_through` gates attribution on `other.wrote_params`,
   because a binding alone is not an effect.
2. **`constprop` read stale entry values.** The worklist kept `values` — the
   lattice, what a formal carries *on entry* — and read from it at every call
   site. For `a(x) { x = 0; b(x); }` that handed `b` the constant the *caller*
   passed into `a`, ignoring `x = 0`. The fix splits the two: `values` stays the
   entry facts the worklist fixes, a body-local `cur` tracks what each formal
   holds *here*, and call sites read `cur`. Regression:
   `a_formal_overwritten_in_the_body_does_not_propagate_its_entry_value`.
   `track_write`'s comment records why the fresh value must be `NonConst` and not
   `Top` — `Top` is meet's identity, so it would keep the entry constant alive
   below the write.

Fixing 2 exposed a third question: which formals does a **global** write kill?
`may_alias_global` (§8.5.2's binding graph again, run to a fixpoint) marks the
formals of a procedure that may name shared storage — directly, because `&g` or
`g` was passed at that position, or transitively, because a formal that may alias
a global was passed down. `track_write` and `kill_through_callee` kill exactly
that set on a global write. Three regressions pin both ends of it:

- `a_global_write_does_invalidate_a_formal_bound_to_that_global` — for
  `main() { a(&g); }` / `a(x) { g = 1; b(x); }`, `b`'s formal must be `NonConst`,
  because `x` *is* `g` and the body's store to the global is a store through the
  formal.
- `a_global_write_after_reassigning_an_aliased_formal_kills_it` — the harder half,
  `main() { a(&g); }` / `a(x) { x = 5; g = 7; b(x); }`. Concretely `b` receives 7,
  but the lattice cannot name `g`'s value, so `x` must be `NonConst` after the
  global store; reporting the `Const(5)` the body left in `cur` would be unsound.
  The earlier reassignment of `x` does **not** exempt it from the kill.
- `a_global_write_does_not_invalidate_a_formal_that_cannot_alias_it` — the
  precision half, `main() { a(3); }` / `a(x) { g = 1; b(x); }`. `x` was passed by
  value, so it is a private cell the store to `g` cannot reach and `b`'s formal
  stays `Const(3)` — precision that a blunt "any global write kills every formal"
  rule would lose.

A right-hand side that is not a literal (`x = y + 1`) is deliberately not tracked;
it becomes `NonConst`. The worklist may visit a body once, so `y` would be read at
a value a later statement in the same body already destroyed, and a stale constant
is the exact error this work exists to remove. The precision loss is confined to
procedures that overwrite their own parameters.

**Stress note for whoever touches this next.** The shipped fuzz runs
`CASES = 2000` per seed. During this work the same generators were run at 6,000
and 30,000 cases per seed and were green. The constprop failure did **not** appear
at the original 400 cases, so raise `CASES` before changing a lattice rule, not
after.

---

## 5. Verification

| Suite | Tests | Result |
| --- | --- | --- |
| `cargo test --release --lib ipa::` | 50 | ok |
| `tests/phase8_features.rs` | 59 | ok |
| `tests/phase8_fuzz.rs` (differential + seed-and-revert) | 20 | ok |
| `tests/phase8_e2e.rs` (real `m0-check`, accepts + tampers) | 8 | ok |
| `tests/run_phase8_tests.sh` (**new**) | — | green (6/6) |
| `cargo test --release` (whole crate) | 829 | ok, 0 failures, 37 suites |

No Phase 8 test is `#[ignore]`d, and `src/ipa/` has no `TODO`, `FIXME`, `todo!`
or `unimplemented!`.

**The fuzz suite is §8.11 as written.** Its twenty tests break down as:

- **Ten soundness drivers** — MOD, purity, summaries, constprop, call graph,
  certificate replay (plus a perturbed certificate that must *not* check),
  determinism, call-site composition against the bottom-up summary, context
  refinement, I/O summaries. Each asserts a relation in *both* directions: a
  static answer may be less precise than what one concrete run observed, never
  more optimistic, and a claim the run contradicts is a violation.
- **Six seed-and-revert tests**, one per analysis domain. Under the `bug` flag the
  expected answer is computed the way the spec's §8.11 says to seed it — without
  interprocedural propagation — and the fuzz **must** report violations; then the
  same seed must report none. The six mutations: binding propagation for MOD,
  callee effects for purity, call-site folding for summaries, a wrongly-seeded
  `Top` for constprop, textual-only edges for the call graph, deleted call
  statements for I/O. An oracle that cannot fail is not an oracle, so the failure
  is asserted, not assumed.
- **One non-vacuity test** — the generator must actually produce a write through
  a formal, an indirect call, a procedure the run never enters, and a uniform
  literal at a call site, or the drivers above prove nothing.
- **Three `dbg_first_*` helpers** that print the first violating case rather than
  asserting on it, kept so a future failure is reproducible from a seed alone.

**The e2e suite drives the real `m0-check` binary** (`CARGO_BIN_EXE_m0-check`),
not a library call: a genuine `Pure`, `ModSet`, `FormalValue` and `Summary`
certificate is accepted, and for each of those four a tampered claim — a purity
the program contradicts, a MOD set missing a symbol the callee does write, a
formal value the analysis refutes, a summary missing a symbol — is rejected by the
checker's own recomputation. The eight tests are those four accepts and four
rejects.

---

## 6. How to reproduce

```sh
cd m0
cargo test --release --lib ipa::      # 50 passing
bash tests/run_phase8_tests.sh        # features + fuzz + e2e + lib units + Phase 7 regression
cargo test --release                  # full suite
```

---

## 7. Integration gaps — CLOSED as of 2026-10-10

The two gaps documented in the original status are now resolved:

**1. `ProcSummary` cross-domain fields — IMPLEMENTED.** `ProcSummary` now carries:
- `regions: BTreeMap<Slot, RegionSummary>` — Phase 6 array region summaries
- `pts_effects: Vec<PointsToEffect>` — Phase 7 points-to effects
- `const_lattice: BTreeMap<(ProcId, usize), AbstractValue>` — populated from constprop analysis

These are serialized in certificates and re-derived by `m0-check` from the carried
`IpaProgram`. See `src/ipa/summaries.rs` and the `ConstLattice`/`Regions`/`PtsEffects`
serialization in `src/ipa/certificate.rs`.

**2. Call statement in loop IR — IMPLEMENTED.** `StmtKind` now has a `Call` variant:
```rust
Call { callee: String, args: Vec<ScalarExpr>, ret: Option<usize> }
```
This enables loops with calls to be represented in the array IR. The `uses()` and
`defs()` methods correctly read call arguments and write the return value. Purity
analysis marks calls as impure by default. See `src/ir/stmt.rs` and
`src/ir/purity.rs`.

With both gaps closed, a loop calling `helper()` can now be represented, decided,
and certified through the full pipeline: `m0-decide` → `Claim::Interprocedural`
→ `m0-check`. The Phase 8 e2e suite (`tests/phase8_e2e.rs`) validates this.

**3. Where the spec's line estimate went.** §8.1's tree totals ~5,600 lines and
§8.12's table totals ~7,150 (5,150 source + 2,000 tests). We are at **3,098
source lines** and **3,146 test lines** — about 60% of the source estimate, over
the test estimate. The source shortfall is concentrated exactly where item 1 is:
§8.7's cross-domain fields, and the per-domain composition in `compose.rs` that
only becomes meaningful once regions and points-to effects live in a summary
(`compose.rs` is 120 lines against a 500-line sketch). The largest module is
`constprop.rs` at 662, near its 600 estimate; `callgraph.rs` (407/800) and
`summaries.rs` (288/800) carry none of the cross-phase payload those estimates
assumed. Nothing in §8.1–§8.6, §8.8 or §8.9–§8.11's *behaviour* was dropped, and
§8.7's computation is complete — only the three cross-domain fields of item 1 are
missing from its output struct. Phase 7 made the same trade (2,315 against
~5,350) and recorded it the same way.

---

## 8. Close-out

Phase 8 is **complete and verified**. The analysis stack is sound and certified, and
the integration gaps documented in §7 are now closed:

- `ProcSummary` carries Phase 6 region and Phase 7 points-to effects (§8.7 cross-domain fields)
- The array IR has a `Call` statement kind (`StmtKind::Call`) enabling loops with calls to be represented and decided
- The `constant_prop` caller is wired in the decider and results flow to M4 and the interprocedural lattice

Every Phase 8 claim emitted by `m0-decide` is re-derived by `m0-check` from the carried `IpaProgram`. The §8.0 user-visible outcome — a loop calling `helper()` decided instead of `MAY_DEPEND` — is now achievable through the full pipeline.




