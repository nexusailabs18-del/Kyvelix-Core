# Phase 9 — status

**Scope.** Phase 9 is the frontend: the module that makes everything else reachable
from source. It splits in two, and the split has always been load-bearing:

* **9a — lexer, parser, AST** (done earlier, unchanged here): `src/frontend/lexer.rs`,
  `parser.rs`, `ast.rs`, with a depth-guarded recursive-descent parser
  (`MAX_DEPTH = 256`, run on an 8 MiB thread) so deeply nested input is refused
  rather than crashing.
* **9b — lowering and the rest of the frontend** (`§9.4`–`§9.14`): this pass.
  `lower.rs` now lowers the analyzable subset, `ctrl_edges.rs` and `loopkinds.rs`
  exist, and the whole chain is verified end to end.

**Headline.** A `.js` file now reaches the analyzers: `tests/phase9_e2e.rs` lowers a
kernel, hands the IR to the **real** `m0-decide` binary, and replays the certificate
it wrote in the **real** `m0-check` binary — `ACCEPT`. Two tampering cases must
`REJECT`. That is the claim Phase 9 exists to make, and it is tested rather than
asserted in prose.

---

## 1. Module inventory

| Module | § | Lines | Tests in file | Where its tests live |
|---|---|---|---|---|
| `src/frontend/lexer.rs` | §9.2 | ~800 | 14 | in-file |
| `src/frontend/parser.rs` | §9.1, §9.3 | ~1.9k | — | `tests/phase9_parser.rs`, `tests/phase9_kernels.rs` |
| `src/frontend/ast.rs` | §9.3 | ~470 | — | via the parser tests |
| `src/frontend/lower.rs` | §9.4–§9.7, §9.14 | ~1.5k | — | `tests/phase9_lower.rs` (20), `tests/phase9_fuzz.rs` (4) |
| `src/frontend/ctrl_edges.rs` | §9.11 | ~770 | 12 | in-file |
| `src/frontend/loopkinds.rs` | §9.10 | ~300 | 5 | in-file |

Nested `FunctionDecl` bodies are walked for loops (both modules) but control
dependence is intraprocedural by construction.

---

## 2. The trust boundary

Lowering produces `Kernels { loops: Vec<ir::Loop>, skipped: Vec<Skipped> }`. Two
properties matter to a consumer:

* **`loops` are IR that `m0-decide` accepts unchanged** — every emitted `Loop` is
  re-checked with `Loop::validate` before it leaves (`close_nest`), so a loop that
  fails a decider's precondition is reported as a refusal instead of shipped.
* **`skipped` is not decoration.** A file with one analyzable nest and one construct
  outside the subset yields both facts (§9.14's rule is *per construct*), and a
  consumer that reads only `loops` is reading a partial analysis. `Kernels` says so
  in its own doc comment, and `phase9_fuzz` asserts the split for nine different bad
  constructs.

`LowerError` distinguishes the two failure kinds the caller must respond to
differently: `OutOfScope(name)` — the language, but §9.14's boundary; and
`NotAnalyzable(msg)` — in scope, but this shape is not expressible in the IR.

---

## 3. Soundness decisions worth naming

1. **A `for-of` is lowered to the walk it denotes, not to a guess.** `v#of` is a
   synthesized IV (`#` cannot appear in a JS identifier, so it cannot collide), its
   trip count is `Env`'s `A.length`, and a read of the binding is recorded as a read
   of `A` at that index. Two facts must be *available* or the construct is refused:
   the iterable is a plain name, and the length resolves. A **zero-length** array is
   deliberately **not** a refusal — nothing in the body executes, so there is no
   access to record.
2. **Sibling loops are refused, not merged.** One `Loop` states one iteration space.
   Charging `for (i) { for (j) A[…]; for (k) B[…]; }` to a single nest would emit
   depth 2 with `k` playing `j`'s role — a dependence between iterates that never
   coexist. The check compares a reopened level's IV name against the space already
   recorded, so the *same* IV reopening the same space is still lowerable.
3. **A guard the IR cannot name refuses the nest; one it can name but not constrain
   is dropped.** The dropped case only ever *adds* dependences (its reads are still
   recorded), which is the sound direction; the refused case is where a silently
   dropped predicate would become a wrong `NoDependence`.
4. **`Access::expr` is always `None`.** A surface expression would send the access to
   M2g, which re-derives coefficients from the string and would discard the ones
   computed here.
5. **Statements beside a nested loop are charged to the same nest**, padded with a
   zero for every deeper IV. A zero can only *add* address collisions, never remove
   one — so the dependence relation is preserved rather than invented.
6. **`unknown_kind()`, not an empty dependence slice.** `ir::loopkind` answers
   `Doall` for `&[]`, which means "computed, and empty". The frontend passes `&[]`
   only when it has really computed the set; otherwise it records `Sequential`.
   `loopkinds::an_uncomputed_dependence_set_must_not_read_as_doall` pins this.

---

## 4. Two things the tests found

Both are recorded because they are the kind of mistake this codebase keeps making:
a *plausible* claim that measurement contradicted.

1. **The first differential fuzz generator emitted siblings, not a nest.** It wrote
   each loop's closing brace as that loop's statements finished, so a "depth-3 nest"
   arrived at the lowering as three sibling loops — and the lowering was right to
   answer with three loops. The oracle was wrong, not the code. Fixed by opening the
   whole chain before writing any statement; the generator now says so in a comment.
2. **A certificate tamper that swaps the `pair` is *accepted*.** The first version of
   `phase9_e2e`'s negative test swapped the two elements of the certificate's `pair`
   and expected `REJECT`; the checker said `ACCEPT`. It is right: a pair is a
   *locator*, and swapping it names the same two accesses in the other order. The
   test now bends a **claim** instead (`TripCount`'s `count`, or an `Exact` distance
   vector), which the checker must rebuild from the CFG and the loop. The old form
   was a vacuous negative wearing the costume of a real one.

---

## 5. Verification

| Command | Result |
|---|---|
| `cargo test --release --lib frontend::` | **33 passed** (14 lexer, 12 `ctrl_edges`, 5 `loopkinds`, 1 doc-contract, 1 other) |
| `cargo test --release --test phase9_lower` | **20 passed** |
| `cargo test --release --test phase9_fuzz` | **4 passed** |
| `cargo test --release --test phase9_e2e` | **5 passed** |
| `cargo test --release --test phase9_kernels` | 13 passed (unchanged) |
| `cargo test --release --test phase9_parser` | 5 passed (unchanged) |
| `cargo test --release --test phase9a_evidence` | 20 passed (unchanged) |
| `bash tests/run_phase9_tests.sh` | the gate above, in one command: **9 PASS / 0 FAIL** |
| `cargo test --release` (whole suite) | **920 passed, 0 failed** — 863 before the Phase 9 pass, 905 after it, +15 from the follow-up (§5.9 runtime claim, `induct`/`scalardd` in-file tests, `ctrl_edges` `reorder_plan`) |
| `bash tests/run_phase7_tests.sh` | 5 PASS / 0 FAIL |
| `bash tests/run_phase8_tests.sh` | 6 PASS / 0 FAIL |
| `bash tests/run_phase5_6_tests.sh` | 12 PASS / 0 FAIL |
| `bash tests/run_all.sh` | 10 loops: 10 ACCEPT, 0 REJECT, 0 UNKNOWN |
| `bash tests/run_real.sh` | coverage 100.0% (EXACT+NONE / total), 0 rejects, 0 crashes |
| `bash tests/run_negative.sh` | 11 PASS / 0 FAIL — "checker is sound on all negative cases" |

The 42 new tests are exactly the additions of this pass: 20 `phase9_lower`,
4 `phase9_fuzz`, 5 `phase9_e2e`, 7 `ctrl_edges`, 5 `loopkinds`, and the 1
`DecisionTree` shape fuzz in `phase2_features`. A later follow-up added 5 more
`ctrl_edges` tests for the `reorder_plan` consumer (12 total) and left the rest
unchanged.

**What the end-to-end test actually does**, since it is the headline:

1. `lower_source(include_str!("js_kernels/syr2k.js"), Env::of(&[("n", 8), ("ld", 8)]))`
   — a real `.js` fixture through the real lowering.
2. Writes the resulting `Loop` as `loop.json` in a temp directory.
3. Runs `m0-decide loop.json` there (the real binary, via `CARGO_BIN_EXE_m0-decide`).
4. If it wrote `cert.json`, runs `m0-check loop.json cert.json` and requires `ACCEPT`.
5. Asserts that **at least one** fixture produced a certificate, so the test cannot
   pass vacuously by deciding nothing.

Then the negatives: a loop file whose first IV bound is shrunk (the certificate's
loop hash no longer matches) must `REJECT`; a certificate whose claim is bent by one
must `REJECT`.

---

## 6. Accepted scope (§9.14)

Refused **by name**, so nothing is silently guessed — each of these is a stated
boundary, not an unfinished branch:

| Refused | Refused at |
|---|---|
| `while`, `do-while` | `refusal for a trip count that is not a function of a counter` |
| `for-in` | keys, which no `IV` chain states |
| `for await` | `async` iteration |
| `break` / `continue` | a body that can leave early has no bound its IV could state |
| `try` / `catch` / `finally`, `throw` | control flow the nest cannot express |
| `switch`, labelled flow | same |
| `class`, `import`, `export`, module syntax | not a loop nest |
| destructuring (`let [a,b] = A`, `for (const [a,b] of A)`) | §9.6's module |
| a function expression, a call, `new`, `await`, `yield` in a body | §9.5's effects |
| a named property (`obj.field`) | §9.8's shape inference |
| three or more subscripts | §9.9's array-of-structs |
| a nonrectangular bound (`j <= i`), a nonlinear index, a foreign name in an index | `Access::index` is one coefficient per IV |
| an access outside any loop, depth > 4, `i64` overflow | IR capacity |

`closures.rs`, `objects.rs` and `aostruct.rs` are **not built** and §9.14 says they
need not be; `stubs.md` §7 Stub 6 records that.

---

## 7. Close-out

Phase 9 is complete. What was true before this pass — "a `.js` file cannot reach the
analyzers" — is no longer true, and the difference is measured by
`tests/phase9_e2e.rs` rather than described here.

Two honest qualifications:

* **`ctrl_edges` has a consumer.** Its SPP Layer 2 reordering plan —
  [`crate::frontend::ctrl_edges::ControlGraph::reorder_plan`] — returns the
  straight-line runs of control-equivalent statements a reordering pass may
  permute, split at every decision, join, arm boundary and terminator so a
  reorder can never change which decisions govern a statement. The module is a
  verified *and* load-bearing building block; what still does not exist is a pass
  that *acts* on the plan (a transformation that rewrites statement order), which
  is SPP Layer 2's own scope rather than this module's.
* **`jaccard.js` still does not decide.** Its `for-of` now lowers (the refusal moved),
  but its loop body calls `setB.includes(item)` inside a guard, and a call in a guard
  is refused because the IR cannot state the call's effects. That is §9.14's honest
  answer, and `phase9_lower::jaccard_lowers_the_for_of_and_then_refuses_the_call_in_its_guard`
  asserts precisely that the *first refusal moved* — which is the honest way to test
  a partial improvement.

Everything else in `stubs.md` is either closed or a conservative fallback that is
correct by construction; `stubs.md` §8 lists the nine remaining items, none of which
is a soundness hole.
