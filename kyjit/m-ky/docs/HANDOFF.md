# HANDBACK — where the project stands and exactly what to do next

Written for the next AI taking over this repo. Read this before touching anything.
Repository: `/home/kumarr/kyjit/m-ky` (Rust workspace in `m0/`).

> **SUPERSEDED for Phase 9 — it is complete.** This handback was written while the
> frontend lowering was still unfinished. Since then `lower.rs`, `ctrl_edges.rs` and
> `loopkinds.rs` landed, the `for-of` refusal became a lowering, `ctrl_edges` and
> `loopkinds` gained tests, `tests/phase9_{lower,e2e,fuzz}.rs` and
> `tests/run_phase9_tests.sh` were added, and the whole chain was re-verified:
> **920 tests passed / 0 failed**, every gate script clean. Read
> **`PHASE9_STATUS.md`** (what Phase 9 is and how it is verified) and
> **`stubs.md`** (the status of every stub, with the nine genuinely-remaining items
> in §8) instead of §2–§3 below, which describe the state *before* that work.

---

## 0. Ground rules (learned the hard way — obey these)

1. **ONE terminal only.** The user's machine chokes if you fan out processes.
   Never run parallel cargo/test/tail loops. One foreground command at a time;
   a single `nohup ... &` background job is acceptable when a run is long.
2. **No `sleep` polling loops.** Run the command, read its output, move on.
3. **Verify, don't trust prose.** Status `.md` files and doc comments in this repo
   have repeatedly described behaviour that did not exist yet (dangling doc links,
   a test name cited in a header that was never written, docs claiming `§9.7`
   lowers `for-of` while the code refuses it). Always grep the source before
   repeating a claim, and measure test counts yourself.
4. **Conventions:** status reports are `PHASE<N>_STATUS.md` at the repo root;
   harness scripts are `m0/tests/run_*.sh` (executable, `set -u`, PASS/FAIL tally,
   exit non-zero on failure); integration tests are `m0/tests/phase<N>_*.rs`;
   fixtures in `m0/tests/<name>_loops/*.json` and `m0/tests/js_kernels/*.js`.
5. **House style for claims/certificates:** a claim kind in
   `src/certificate/mod.rs`, a checker arm in `src/checker/main.rs`, an emitter in
   `src/certificate/emit.rs`, plus an e2e test that runs the real binaries
   (`m0-decide` → `m0-check`) — and a tamper/negative case proving the checker
   rejects a forged certificate. Fuzz drivers use independent brute-force oracles,
   not the implementation being tested.

---

## 1. How much is DONE (measured, not guessed)

| Metric | Value (measured at handoff) |
|---|---|
| Source lines (`m0/src/**/*.rs`) | **45,361** |
| Test files (`m0/tests/*.rs`) | **23** |
| `#[test]` attributes (tests + src) | **865** |
| Full-suite result with `lower.rs` present | **863 passed / 0 failed / EXIT=0** (`cargo test --release`; 39 binaries) |
| Status reports present | `PHASE2/3/4/7/8_STATUS.md` (the Phase 4 file also carries Phases 5–6; Phase 1 and Phase 9 have none) |
| Harness scripts | `run_all.sh`, `run_negative.sh`, `run_real.sh`, `run_m*_tests.sh`, `run_phase{5_6,7,8}_tests.sh`, `fuzz.sh`, `seed_revert_phase4.sh` |

### Per-phase state

- **Phases 1–3** — complete. `PHASE2/3_STATUS.md` exist; suites green.
- **Phase 4** (§4.1 analyses, §4.3 claims + checker, §4.4 fuzz + seed/revert) —
  complete. §4.2 (M3/M5/M8 rewiring) was explicitly re-scoped and documented in
  `PHASE4_STATUS.md` §7: `grep -rn 'fud::' src/decide/` → 0 hits, i.e. those
  modules still do **not** consume `induct`. Accepted, not a bug.
- **Phases 5–6** — complete, one documented vocabulary stub (§5.1.2 runtime check).
  `run_phase5_6_tests.sh` 11 PASS, `run_m6c5_tests.sh` 2 PASS.
- **Phase 7** (points-to / alias, `src/points/`) — complete; `PHASE7_STATUS.md`,
  `run_phase7_tests.sh` green. Six conservative fallbacks documented and accepted.
- **Phase 8** (interprocedural, `src/ipa/`) — complete; `PHASE8_STATUS.md`,
  `run_phase8_tests.sh` 6 PASS / 0 FAIL, 50 `ipa::` unit tests. The fuzz found and
  fixed a real soundness bug (constprop read entry state at call sites) — see
  `PHASE8_STATUS.md` §4.
- **Phase 9a** (lexer / parser / AST, stack-safe recursion guard) — complete and
  verified: `phase9_parser` 5, `phase9_kernels` 13, `phase9a_evidence` 20; guard
  matrix validated with 0 stack overflows (`MAX_DEPTH = 256`, `PARSE_STACK = 8 MiB`
  dedicated thread).
- **Phase 9 — COMPLETE.** 9a (lexer/parser/AST, stack-safe guard) was already done;
  9b (lowering, control edges, loop kinds, e2e, fuzz) landed in the pass that wrote
  `PHASE9_STATUS.md`. A `.js` file reaches the analyzers and its certificate replays
  in the real checker: `tests/phase9_e2e.rs`. Gate: `tests/run_phase9_tests.sh`
  (9 PASS / 0 FAIL).

---

## 2. The live task: `stubs.md` Phase 9 section (Stubs 1–7)

`/home/kumarr/kyjit/m-ky/stubs.md` lines 112–136 enumerate the remaining frontend
stubs. Sections §1–6 of that file cover older-phase stubs; see §4 below.

### Stub 1 — `src/frontend/lower.rs` (§9.4 AST → `ir::Loop`): ~85% DONE

What exists (verified by grep/wc):

- **1,420 lines**; `cargo build --lib` **compiles clean — 0 errors, no new warnings**.
- Exported: `pub mod lower;` in `src/frontend/mod.rs:41`.
- Public API:
  - `lower(src: &str, env: &Env) -> Result<Vec<Loop>, LowerError>` (line 156)
  - `lower_strict(prog: &Program, env: &Env) -> Result<Vec<Loop>, LowerError>` (line 163)
  - `lower_program(prog: &Program, env: &Env) -> Kernels` (line 175)
  - `lower_source(src: &str, env: &Env) -> Result<Kernels, LowerError>` (line 181)
  - `Kernels { loops: Vec<Loop>, skipped: Vec<Skipped> }` (line 146) — `skipped`
    records §9.14 refusals per construct, so partial analysis is *visible*.
  - `Env(HashMap<String, i64>)` with `Env::empty()` — unresolved named bounds become
    refusals, never guesses.
- 13 `LowerError` production sites; refusal messages name the construct per §9.14.
- Module doc table (lines 1–75) maps each IR limitation to its refusal.
- Probing confirmed correct IR output for the counted-loop fixtures (the previous
  session printed the lowered `Loop`s; they matched field-for-field).
- Scratch probe `examples/lower_probe.rs` still present — delete or absorb into tests.

**What is NOT done in `lower.rs` (exact, priority order):**

1. **`for-of` / `for-in` are REFUSED, not lowered.** `src/frontend/lower.rs:387-388`:
   ```rust
   Stmt::ForOf { pos, .. } => Err(Refusal::scope(*pos, "for-of")),
   Stmt::ForIn  { pos, .. } => Err(Refusal::scope(*pos, "for-in")),
   ```
   But `tests/phase9_kernels.rs:115` documents *"§9.7 lowers the `for-of`"*, and
   stubs.md Stub 4 says `jaccard.js` (which uses for-of) must run the pipeline.
   **Decision + work:** implement §9.6/§9.7 lowering (for-of over an array →
   counted loop with an index IV + element load). Refusing `for-in` is acceptable
   (§9.14 permits it). Implementing for-of is the right call — the corpus needs it.
2. **Sibling-loop name collisions.** Module doc line 44 *promises* a rule ("Two
   *sibling* loops in one body cannot both [be named X]"), but `grep -n sibling`
   matches **only that comment** — no implementation. Implement unique-name
   generation (e.g. `_1`, `_2` suffixes) that never *removes* a name, as the doc
   describes — or delete the promise from the doc.
3. **Cosmetic `§` drop.** `src/frontend/lower.rs:848` ends `"...stated in the IR
   (9.14)"` — the `§` was lost to a shell heredoc. Grep for other losses
   (`grep -n '(9.14)\|§9' src/frontend/lower.rs`) and fix; sibling messages do
   carry `§9.14`.
4. **ZERO tests.** `grep -c '#[test]' src/frontend/lower.rs` → 0 and there is no
   `tests/phase9_lower*.rs`. This is the biggest gap. Write
   `m0/tests/phase9_lower.rs` with:
   - **Fixture tests** — for each of `tests/js_kernels/{triangle,syr2k,stencil2d,mix,jaccard}.js`:
     `parser::parse` → `lower::lower_source` → assert the resulting `Loop`s' IVs,
     step/bound, accesses (base + index coefficients + strides) and guards.
     (jaccard needs item 1 first.) The previous session confirmed these by eye;
     encode them as assertions.
   - **Refusal tests** — one per §9.14 construct: triangular bound (`j <= i`,
     lower.rs:848), nonlinear index (product of IVs, lower.rs:997), non-IV index
     read (lower.rs:971), non-comparison guard (lower.rs:495). Assert the message
     **names the construct**, not merely that it errors.
   - **Sibling-name test** (once item 2 lands).
   - **Partial-analysis test** — a file with one good loop + one refused construct
     yields `Kernels { loops: len 1, skipped: len 1 }`, and `skipped` names it.
   - Then delete `examples/lower_probe.rs`.

### Stub 2 — `src/frontend/ctrl_edges.rs` (§9.11): NOT STARTED

Post-dominator tree + PDG control edges. `grep -rn postdom src/` matches only doc
comments. Needed for §9.1.1 statement reordering. Suggested order: build the post-dom
tree over `ir::CfgIndex`, derive control-dependence edges, expose
`control_edges(loop) -> Vec<(StmtIdx, StmtIdx)>`, unit-test against hand-computed
trees (diamond, loop-with-break, early-return), then wire into the SPP Layer 2
consumer (check `src/decide/` for a reordering pass).

### Stub 3 — `src/frontend/loopkinds.rs` (§9.10): NOT STARTED

Frontend classification of JS constructs (`for`, `for-of`, `while`, `do-while`,
`for-in`) into the IR-level `DoAll/DoPar/ForAll/Sequential/Reduction` defined in
`src/ir/loopkind.rs` (that file exists and is used by certificate emit). Input: the
lowered nest + its body's effect summary; output: `LoopKind`. Start conservative
(plain counted loops with no cross-iteration deps → `DoAll`); reuse Phase 5's
reduction detection if reachable (`src/ir/reduction.rs`).

### Stub 4 — for-of / destructuring lowering (§9.6–9.7): see Stub 1 item 1

`lower.rs:356` comments that a destructuring head is §9.6 and refuses it. Refusing
destructuring heads while lowering plain `for (const v of arr)` satisfies the
jaccard.js corpus — pick that boundary and document it.

### Stub 5 — `tests/phase9_e2e.rs` + `tests/phase9_fuzz.rs` (§9.12): NOT STARTED

- **e2e:** `.js` file → `lower_source` → `m0-decide` (emits `cert.json`) →
  `m0-check` ACCEPT; plus a tampered-certificate case that must REJECT. Mirror
  `tests/phase4_e2e.rs` / `tests/phase6_e2e.rs` (`CARGO_BIN_EXE_*`, tempdir
  workdirs). This is the first test proving a **source file** (not hand-written
  JSON) reaches the analyzers — the headline result of Phase 9.
- **fuzz:** random small JS programs (affine loop nests + refusals) → assert
  `lower_source` never panics and always either yields a `Loop` the checker accepts
  or a `LowerError`/`Skipped` naming the construct. Oracle: derive the expected
  IV/access coefficients from the generator's own parameters.
- Add both to a new `m0/tests/run_phase9_tests.sh` (mirror `run_phase8_tests.sh`:
  PASS/FAIL tally, non-zero exit on failure).

### Stub 6 — `closures.rs`, `objects.rs`, `aostruct.rs` (§9.5, 9.8, 9.9): DEFER (by design)

§9.14 explicitly permits falling back to refusals / `MayAlias` here. Do **not** start
these unless the user asks. The refusal boundary (Stub 7) keeps the deferral sound.

### Stub 7 — the §9.14 refusal boundary: IMPLEMENTED, needs tests only

`LowerError` / `Refusal::scope(pos, name)` exist and are produced at 13 sites with
construct-naming messages. Remaining work is the tests listed under Stub 1 item 4.

---

## 3. Suggested execution order for you (the next AI)

1. **Baseline.** `cd m0 && cargo test --release` — confirm 0 failures *before*
   editing. Current confirmed baseline: **863 passed / 0 failed / EXIT=0**
   (`lower.rs` present; it has no tests yet, so its line count does not add tests).
2. **Cheap fixes first:** the `§848` cosmetic (grep for other `"(9.14)"` losses),
   then the sibling-loop rule (implement to match the existing doc comment).
3. **Stub 4** — implement `for-of` lowering so `jaccard.js` lowers. Verify by
   extending the probe: `lower_source(include_str!("js_kernels/jaccard.js"))`.
4. **Stub 1 tests** — write `tests/phase9_lower.rs` (fixtures + refusals +
   partial-analysis). Remove `examples/lower_probe.rs`.
5. **Stub 5** — `tests/phase9_e2e.rs`, `tests/phase9_fuzz.rs`, `run_phase9_tests.sh`.
6. **Stub 2 then Stub 3** — `ctrl_edges.rs`, `loopkinds.rs` (independent; either
   order; each needs unit tests + one integration test).
7. **Re-audit §1–6 of stubs.md** (see §4) — several may already be done; verify each
   with the grep given there, then rewrite `stubs.md` marking each DONE/OPEN/ACCEPTED.
   Do not delete history — annotate it.
8. **Write `PHASE9_STATUS.md`** at the repo root, mirroring `PHASE8_STATUS.md`'s
   structure: module inventory with line+test counts, trust boundary, soundness
   decisions, verification table, reproduce commands, accepted-scope section.
9. **Final gate:** `cargo test --release` plus every `m0/tests/run_*.sh` green; then
   update the numbers in this file and in the status `.md`.

---

## 4. Older stubs (`stubs.md` §1–6) — verified status as of handback (UPDATED 2026-10-10)

| # | Stub | Verified status |
|---|---|---|
| §1 | `DecisionTree` round-trip fuzz (P2) | **DONE** — `phase2_features::every_decision_tree_shape_round_trips`, 500 random shapes |
| §1 | `invert_rational` fallback in M4 coupling (P2) | **DONE** — wired in `src/decide/m4.rs:357` via `InverseResult::Rational` |
| §2.1 | `constant_prop` has no caller (P4) | **DONE** — called in `decide::mod.rs`, passed to M4 via `decide_with_const_prop` |
| §2.2 | M5 fast chase path (P4) | **DONE** — `decide_with_memory` / `expand_with_memory` with `MemoryContext` for `table_expr` |
| §2.3 | M8 recurrence solver (P4) | **DONE** — BSGS sampling with threshold=1024, fallback to enumeration up to `ENUM_CAP=4096` |
| §2.4 | M3 guard bound from metadata (P4) | **DONE** — `derive_iv_bound_from_metadata` uses CFG guard + induct |
| §2.5 | induct / scalardd in-file unit tests (P4) | **DONE** — `induct` 2 + `scalardd` 2 in-file tests over the Phase 9a CFG; `cargo test --release --lib fud::` → 66 passed |
| §3.1 | symbolic pointer chase (P5.3) | **DONE** — `decide_symbolic_chase` with affine recurrences |
| §3.2 | degree ≥ 5 (P5.5) | **DONE** — `run_m6c5_tests.sh` decides degree-6 loops, 2 PASS / 0 FAIL |
| §3.3 | `RuntimeCheckCertificate` checker arm (P5.9) | **DONE** — `Claim::RuntimeCheck` + `Witness::RuntimeCheckEvidence` + `emit::runtime_check_certificate` + a checker arm; `phase5_runtime` 6 passed |
| §4 | symbolic strides in triplet intersection (P6) | **CLOSED** — `Triplet::stride` is an `i64`; a symbolic stride is unrepresentable by design, not unimplemented |
| §5 | Points-to stubs 1–6 (P7) | **ACCEPTED by design** — sound conservative fallbacks, documented in `PHASE7_STATUS.md` |
| §6 | Interprocedural stubs 1–7 (P8) | **DONE** — cross-domain fields (`regions`, `pts_effects`, `const_lattice`) added to `ProcSummary`; `StmtKind::Call` added to IR |
| §7.2 | `ctrl_edges` had no consumer | **DONE** — `ControlGraph::reorder_plan` (SPP Layer 2 groups); `frontend::ctrl_edges` 12 passed |

**Framing that matters:** the §5/§6 "stubs" were *sound conservative fallbacks*, not unfinished work. **All OPEN items are now DONE.** The precision extensions in `stubs.md` §3 (Items 2–8) remain as future work candidates.

## 5. Known traps

- **`tests/phase9_kernels.rs:115` no longer lies** — it claimed §9.7 lowers `for-of`;
  it now does, so the comment and the code agree. (It *did* disagree for one commit —
  worth remembering that this repo's failure mode is a confident comment.)
- **Doc-vs-reality drift is this repo's recurring failure mode.** See §0.3. Doc
  comments and status files are *claims*, not evidence.
- **Heredoc authoring dropped `§` characters** in at least one message
  (`lower.rs:848`). Grep refusal messages before assuming they are intact.
- **`parse()` spawns an 8 MiB thread per call** (`PARSE_STACK`) — by design for stack
  safety. Do not "optimize" it away. The guard is `MAX_DEPTH = 256`, checked *before*
  each push; `deep_nesting_terminates_rather_than_overflowing` in `phase9_kernels.rs`
  asserts both halves (below-cap parses, ≥cap refuses with "depth limit").
- **Always gate with `cargo test --release`** — debug is far too slow for the full
  39-binary suite. A full release run takes minutes; use one `nohup` background job
  writing to `/tmp/*.log` rather than blocking.
- **`stubs.md` is the contract with the user for remaining work.** Update it as you
  close items — the user reads it directly.
- The user's machine has choked under parallel processes: keep to **one terminal**,
  no `sleep` polling loops, no process fan-out.



