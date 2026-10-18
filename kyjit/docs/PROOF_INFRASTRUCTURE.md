# docs/PROOF_INFRASTRUCTURE.md

**Project:** KYJIT
**Document type:** Specification
**Component:** Proof infrastructure (miscompile detector, differential harness, translation validator)
**Date:** 2026-09-13
**Author:** Director
**Status:** Authoritative. Spec for the first three tools.
**Depends on:** `docs/TRIAGE.md` (keep-list defines what is tested)
**Depended on by:** Every optimization pass, every codegen pass, every benchmark

---

## 0. Why this document exists

KYJIT is Class B. The fast path must be provably equivalent to the slow path for every input.

The failure mode is **silent wrong answers**. Not crashes. Not exceptions. Wrong results, on rare inputs, weeks later in production.

This is the hardest failure mode in software. Code review does not catch it. Unit tests that only test the happy path do not catch it. Benchmarks do not catch it, because benchmarks measure speed, not correctness.

**The only defense is to run two paths against each other, on real inputs, before production.**

Three components:

1. **Miscompile detector** — runs both paths on the same input, compares outputs bit-for-bit
2. **Differential harness** — runs the detector across a corpus, in CI, on every commit
3. **Translation validator** — proves the IR-to-machine translation is correct

The first two are built now. The third is not started.

---

## 1. The Class B problem

### 1.1 Shape

```
                  ┌────────────────────┐
Input ──►         │ Fast path (AOT)    │──► Output A
                  └────────────────────┘
                  ┌────────────────────┐
Input ──►         │ Slow path (interp) │──► Output B
                  └────────────────────┘

Requirement: Output A == Output B for every Input
```

The fast path is allowed to be wrong, as long as it is **provably never wrong**. The slow path is the reference. If they diverge, the fast path is the bug.

### 1.2 Failure mode

| Failure | Detected | Cost |
|---|---|---|
| Crash | Immediately | Low |
| Exception | Immediately | Low |
| **Wrong answer** | **Weeks later** | **Catastrophic** |

A wrong answer in a compiler means wrong output in production, wrong financial calculations, wrong data in databases, lost trust, potential lawsuits.

**Zero is the only correct rate of silent wrong answers.**

### 1.3 Why this is hard

| Reason | Why |
|---|---|
| Input space is infinite | Every JS program ever written |
| Optimizations are lies | Each is a claim that must be provably true |
| Interactions are dense | N features = O(N²) pairs, each a potential bug |
| Failures are rare | A miscompile might fire once per billion executions |
| Reproduction is hard | Rare bugs often cannot be reproduced without the original input |

The defense runs before production, on every commit, across a corpus, catching divergence bit-exactly.

---

## 2. Component 1 — Miscompile detector

### 2.1 Purpose

Run the same JS input through two execution paths and compare outputs. If they differ, the fast path is wrong.

### 2.2 Interface

**CLI:**
```bash
miscompile_detector <input.js> --oracle=interp --target=aot
```

**Library:**
```c
typedef enum { ORACLE_INTERP, ORACLE_V8, ORACLE_CUSTOM } OracleKind;
typedef enum { TARGET_AOT, TARGET_UOP, TARGET_LLVM } TargetKind;

typedef struct {
    const char *input_js;
    size_t      input_len;
    OracleKind  oracle;
    TargetKind  target;
} DetectorRequest;

typedef struct {
    bool        diverged;
    char       *oracle_output_json;
    char       *target_output_json;
    char       *minimal_repro_js;
    const char *suspected_pass;
    uint64_t    runtime_ns_oracle;
    uint64_t    runtime_ns_target;
} DetectorResult;

int miscompile_detector_run(const DetectorRequest *req, DetectorResult *out);
```

### 2.3 What "equal output" means

This is the most important decision in the document. "Compare outputs" hides a dozen sub-decisions.

| Output kind | Comparison rule |
|---|---|
| Return value (number) | Bit-exact IEEE 754 |
| Return value (string) | Byte-exact, including UTF-8 encoding |
| Return value (boolean) | Exact |
| Return value (null / undefined) | Exact |
| Return value (object) | Deep structural equality, including property order |
| Return value (array) | Element-wise, length-equal |
| Thrown exception | Same class name, same message, same stack-trace shape |
| Side effects (console.log) | Logged; compared only in strict mode |
| Side effects (file I/O) | Mocked; compared only in strict mode |
| Side effects (network) | Mocked; not compared |
| Timing | Not compared |
| Memory addresses | Not compared |

Rule: any observable output that a program can read must match exactly.

### 2.4 Float comparison

Floats are the hardest case.

Rule: **bit-exact IEEE 754 comparison.**

```c
static bool float_equal(double a, double b) {
    uint64_t ua, ub;
    memcpy(&ua, &a, 8);
    memcpy(&ub, &b, 8);
    return ua == ub;
}
```

Consequences:
- `0.1 + 0.2` must produce the exact same 64-bit pattern in both paths.
- NaN must have the same bit pattern (not just `isnan()`).
- `-0.0` and `0.0` are different.
- Denormals must match exactly.

If the JS spec allows implementation-defined behavior at a point, both paths must agree on the same implementation choice.

### 2.5 Object comparison

Rule: deep structural equality.

```c
static bool object_equal(JSValue a, JSValue b) {
    if (type(a) != type(b)) return false;

    /* For ordinary objects:
     *   1. Same prototype (by identity)
     *   2. Same own property names (same order, same keys)
     *   3. Same property values (recursive)
     *   4. Same extensibility flag
     *   5. Same internal slots (for exotic objects)
     *
     * For arrays:
     *   1. Same length
     *   2. Element-wise equal
     *   3. Same holes pattern
     *
     * For functions:
     *   1. Same name
     *   2. Same arity
     *   3. Same source (if available)
     */
}
```

Cycle detection: objects can be circular. Comparison handles cycles with a visited set.

### 2.6 Exception comparison

Rule: same class, same message, same shape.

```c
static bool exception_equal(JSException a, JSException b) {
    return strcmp(a.constructor, b.constructor) == 0
        && strcmp(a.message,     b.message)     == 0
        && a.stack_depth == b.stack_depth;
}
```

Do not compare: internal stack addresses, timestamps.
Do compare: constructor name, message string, number of frames.

### 2.7 Side effects

Two modes.

- **Default:** side effects (`console.log`, file writes) are logged but not compared. Side effects are often nondeterministic (timestamps, addresses); comparing them produces false positives.
- **Strict:** side effects are intercepted, queued, and compared in order. Used when the program's side effects are known to be deterministic.

### 2.8 Reduction

When a divergence is found, produce a minimal repro.

Algorithm: delta debugging on the AST.

```
1. Parse input to AST.
2. While AST can be reduced:
   a. Try removing a node.
   b. Re-run detector.
   c. If still diverges, keep the smaller AST.
   d. Else restore the node.
3. Output the smallest AST that still diverges.
```

Time-box: 60 seconds. If exceeded, fall back to the unreduced input.

Output format:

```js
// Minimal repro
// Oracle: 42
// Target: 43
// Suspected pass: sroa.c:eliminate_alloc
```

### 2.9 Reporting

Every divergence produces a report with:

| Field | Description |
|---|---|
| Input (original) | Full JS source |
| Input (minimal) | Reduced AST as JS source |
| Oracle output | Full output from slow path |
| Target output | Full output from fast path |
| Diff | The exact difference |
| Suspected pass | IR pass whose output changed between working and broken |
| Compiler version | Git hash |
| Platform | CPU, OS, build flags |
| Timestamp | When detected |
| Severity | Compile-error, wrong-result, undefined-behavior |

Reports are stored as JSON + human-readable in `tools/miscompile_detector/reports/`.

### 2.10 Non-goals

The detector is not:
- A verifier (it detects divergence, it does not prove correctness)
- A fuzzer (it takes inputs, does not generate them)
- A benchmark (it does not measure speed)
- A debugger (it does not step through execution)
- A test suite (it is infrastructure for tests, not the tests themselves)

### 2.11 Test plan for the detector itself

The detector must be tested, or it might be silently broken.

Injection tests:

```bash
tools/miscompile_detector/inject_divergence --pass=sroa --delta=1
tools/miscompile_detector/run test_cases/*.js
# Assert: divergence reported on injected case
```

Coverage:

| Case | Expected |
|---|---|
| Identical outputs | No divergence reported |
| Different float bits | Divergence reported |
| Different exception message | Divergence reported |
| Different object key order | Divergence reported |
| Different side-effect order (strict mode) | Divergence reported |
| Same output, different timing | No divergence reported |

The detector must pass every coverage case before any optimization ships.

---

## 3. Component 2 — Differential harness

### 3.1 Purpose

Run the detector across a corpus, on every commit, in CI.

### 3.2 Corpus — three tiers

| Tier | Source | Purpose | Count |
|---|---|---|---|
| Tier 1 | Test262 | Spec compliance | [TO BE MEASURED — run `find . -name '*.js' \| wc -l` in the Test262 checkout] |
| Tier 2 | Real npm packages | Real workloads | [TO BE ASSEMBLED — list in `tools/differential/npm_corpus.txt`] |
| Tier 3 | Generated programs | Edge cases and combinations | [TO BE DEFINED — count chosen when generator is specced] |

**Corpus counts are inputs to the harness, not claims in this document.** They are measured when the corpus is assembled.

### 3.3 Tier 1 — Test262

Test262 is the official ECMAScript test suite. It lives at https://github.com/tc39/test262.

How to run:

```bash
tools/differential/run_test262 --oracle=interp --target=aot
```

Output:

```
Test262 results:
  Passed:   <n>
  Failed:   <n>
  Skipped:  <n>
  Duration: <t>
```

Gate: 100% pass on the supported surface before any release.

### 3.4 Tier 2 — Real npm packages

List of npm packages lives in `tools/differential/npm_corpus.txt`. Chosen to cover the JS feature space:

- Utility: lodash, ramda, date-fns
- Numeric: mathjs, numeric, simple-statistics
- Data: csv-parse, fast-xml-parser, json5
- Crypto: crypto-js, tweetnacl, bcrypt-js
- Parsing: acorn, espree, cheerio
- Networking: axios, node-fetch, ws
- Framework: express, koa, fastify
- Testing: jest, mocha, ava
- Other: chalk, commander, rimraf

The exact list is decided when the corpus is assembled, not asserted here.

How to run:

```bash
tools/differential/run_npm --list=tools/differential/npm_corpus.txt \
                           --oracle=interp --target=aot
```

Gate: 100% pass on the assembled corpus before any release.

### 3.5 Tier 3 — Generated programs

Generator combines JS features systematically:

```
For each combination of:
  - primitive types (int, float, string, bool, null, undefined, bigint)
  - operators (+ - * / % ** << >> >>> & | ^ && || ??)
  - control flow (if, for, while, switch, try/catch)
  - functions (decl, expr, arrow, async, generator)
  - objects (literal, computed, spread, destructure)
  - arrays (literal, spread, destructure, holes)

Generate a program that exercises the combination.
```

Count and seed chosen when the generator is specced.

How to run:

```bash
tools/differential/run_generated --count=<N> --seed=42 \
                                  --oracle=interp --target=aot
```

Gate: 100% pass before any release.

### 3.6 CI integration

Trigger: every push to any branch.

Pipeline:

```yaml
# .github/workflows/differential.yml
name: Differential Testing
on: [push, pull_request]
jobs:
  build-and-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - run: make build
      - run: tools/differential/run_test262 --oracle=interp --target=aot
      - run: tools/differential/run_npm --list=tools/differential/npm_corpus.txt
      - run: tools/differential/run_generated --count=10000 --seed=42
      - uses: actions/upload-artifact@v3
        if: failure()
        with:
          name: differential-failures
          path: tools/differential/results/
```

Gate: no merge to `main` if any tier fails.

### 3.7 Regression database

Every divergence ever found is stored in a regression database.

```sql
CREATE TABLE divergences (
    id              INTEGER PRIMARY KEY,
    date_found      TEXT NOT NULL,
    input_hash      TEXT NOT NULL UNIQUE,
    input_js        TEXT NOT NULL,
    minimal_repro   TEXT,
    oracle_output   TEXT NOT NULL,
    target_output   TEXT NOT NULL,
    suspected_pass  TEXT,
    git_hash        TEXT NOT NULL,
    fixed_in        TEXT,
    severity        TEXT NOT NULL
);
```

Every commit runs the regression database. No fixed divergence may reappear.

### 3.8 Metrics

| Metric | Target |
|---|---|
| Pass rate across all tiers | 100% |
| Regression reappearance | 0 |
| Divergences per run | 0 |
| Detector false positive rate | < 1% |
| Time to run Tier 1 | [TO BE MEASURED] |
| Time to run Tier 2 | [TO BE MEASURED] |
| Time to run Tier 3 | [TO BE MEASURED] |

Any deviation is a blocker.

---

## 4. Component 3 — Translation validator (not started)

### 4.1 Purpose

Prove the translation from IR to machine code is correct, not just detect divergences.

### 4.2 Why not started

Translation validation is a research-grade problem:

- Requires formal semantics for IR
- Requires formal semantics for machine code
- Requires proof that the translation preserves semantics

No production compiler ships a full translation validator. CompCert proves the entire compiler correct by construction. No JIT or AOT with a feature set like KYJIT's has ever done it.

**Not started.** No phase. No timeline. It starts when it can be finished, and not before.

### 4.3 Trigger condition

Translation validation becomes worth starting when:

- Components 1 and 2 are stable (no false positives, fast enough)
- The compiler has reached a point where optimizations are subtle enough that testing alone is insufficient
- The proof infrastructure has produced enough real divergences to identify which transformations require formal proof

Until all three hold, it is not started.

### 4.4 Shape if built

```
Translation:
  IR_program ──► Machine_program

Validation:
  ∀ input:  exec_IR(IR_program, input) == exec_machine(Machine_program, input)

Proof methods:
  - Symbolic execution of both
  - Equivalence via SMT
  - Refinement proof via translation invariants
```

Tools: Coq, Isabelle, or a custom SMT-based checker.

**Not started.**

---

## 5. Oracle decision

### 5.1 The question

Which path is the reference?

### 5.2 Options

| Option | Pros | Cons |
|---|---|---|
| KYJIT interpreter | Same parser, same semantics, fast to integrate | May share bugs with the fast path |
| V8 | Independent implementation, battle-tested | Different semantics on edge cases |
| Both | Catches more | 2× test time, 2× integration work |

### 5.3 Decision

**Oracle 1:** KYJIT interpreter.

The interpreter and the fast path share the parser but not the optimizer. If they diverge, the optimizer is wrong. If they agree, both are consistent with the parser.

**Oracle 2:** V8.

Added once the interpreter is stable. V8 catches bugs the interpreter shares — parser bugs, runtime bugs. It is the independent check.

**Both oracles run together once Oracle 2 exists.** Divergence between the two oracles is itself a bug to investigate.

---

## 6. First finished feature

The detector runs on the existing pipeline (`parser → interpreter → μOp cache`) and produces its first divergence report.

### 6.1 What that means

Before any new feature is added, the detector must:

1. Take a JS input
2. Run it through the parser
3. Run it through the interpreter → output A
4. Run it through the μOp cache → output B
5. Compare outputs bit-exactly
6. Report divergence (or agreement)
7. Reduce divergence to a minimal repro
8. Write a report to disk

It runs on a set of Test262 files without crashing.

### 6.2 What "first divergence report" means

The detector will find a divergence. This is expected. The μOp cache is young; the interpreter is more mature. The divergence is not a bug in the μOp cache — it is a bug in the proof that the μOp cache is equivalent to the interpreter.

The first divergence report is a milestone because it proves the detector works.

### 6.3 What happens after

1. Fix the μOp cache.
2. Re-run the detector.
3. Confirm no divergence.
4. Add the divergence to the regression database.
5. Move to the next feature.

From this point forward, every feature has the detector as its gate.

---

## 7. Integration with the build pipeline

### 7.1 Where the detector runs

```
Developer writes code
       │
       ▼
Commit pushed
       │
       ▼
CI runs:
  1. Build
  2. Unit tests
  3. Differential harness (Test262 + npm + generated)
  4. Regression database
  5. Benchmark
       │
       ▼
If all pass:  merge to main
Else:         fail build, upload reports
```

### 7.2 Where the differential harness runs

- **Locally:** every developer runs `tools/differential/run_test262` before committing.
- **CI:** every push runs all three tiers.
- **Nightly:** extended corpus, once the corpus exists.
- **Weekly:** V8 as second oracle, once V8 is integrated.

### 7.3 Where the translation validator runs

Not started. If built, it runs on every IR-to-machine translation. Slower than the differential harness but more rigorous.

---

## 8. Dependencies

```
tools/miscompile_detector/
├── README.md              ← spec
├── detector.c             ← main logic
├── compare.c              ← bit-exact comparison
├── reduce.c               ← delta debugging
├── report.c               ← reporting
├── test_injection.c       ← detector self-test
└── reports/               ← output directory

tools/differential/
├── run_test262.c          ← Test262 harness
├── run_npm.c              ← npm harness
├── run_generated.c        ← generator + harness
├── regression_db.c        ← SQLite wrapper
├── npm_corpus.txt         ← npm package list (assembled, not asserted)
└── results/               ← output directory

.github/workflows/
└── differential.yml       ← CI integration
```

Shared infrastructure:

- Input loader — read JS source, parse to AST
- Execution wrapper — run AST through a path, capture output
- Comparison library — bit-exact comparison
- Reporting library — JSON + human-readable
- Corpus manager — enumerate test cases

Build once, use everywhere.

---

## 9. Success criteria

The proof infrastructure is finished when:

| Criterion | Measurement |
|---|---|
| Detector catches injected divergences | 100% of injection tests pass |
| Detector catches float divergences | Bit-exact comparison verified |
| Detector catches object divergences | Deep equality verified |
| Detector catches exception divergences | Class + message verified |
| Reduction produces minimal repros | Verified on known divergences |
| Reporting is complete | Every report has all 10 fields |
| Test262 harness runs all supported tests | [TO BE MEASURED] |
| npm harness runs all packages | [TO BE MEASURED] |
| Generated harness runs all programs | [TO BE MEASURED] |
| Regression DB prevents reappearance | 0 fixed divergences reappear |
| CI blocks merges on failure | Verified on a test branch |
| First divergence report written | Achieved |

All criteria must be met before the detector is considered finished.

---

## 10. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Detector has false positives | Wastes developer time | Bit-exact rules are precise; false positives reviewed weekly |
| Detector has false negatives | Silent wrong answers ship | Injection tests; second oracle (V8) added once available |
| Reduction is too slow | Blocks CI | Time-box reduction to 60 s; fall back to unreduced input |
| Corpus is not representative | Misses real bugs | Corpus chosen to cover feature space; add packages over time |
| Interpreter shares bugs with fast path | Divergence not detected | Add V8 as second oracle |
| CI runs too slowly | Developers bypass it | Optimize; run fast tier on every commit, full tier nightly |

---

## 11. Open questions

1. **Reduction time limit.** Proposed: 60 seconds max. Fall back to unreduced input if exceeded.
2. **False positive rate.** Unknown until the detector runs. Target: <1%.
3. **Detector in release builds?** No. CI-only. Release builds ship without it.
4. **Nondeterministic programs.** Run twice on the same path. If the same path produces different outputs, the program is nondeterministic; skip it.
5. **V8 integration cost.** Unknown. Measured when the integration starts.
6. **Detector on the GPU path.** Not initially. GPU output is not directly comparable to CPU output. Not started.

---

## 12. Sign-off

| Field | Value |
|---|---|
| Spec written by | Director |
| Date | 2026-09-13 |
| Components specified | 3 (detector, differential harness, translation validator) |
| Components built | 0 (spec only) |
| Components started | 0 |
| Oracle decision | KYJIT interpreter first; V8 added once interpreter is stable |
| First finished feature | Detector runs on `parser → interpreter → μOp cache`, produces first divergence report |
| Next document | `tools/miscompile_detector/README.md` |

---

## 13. What comes next

1. `tools/miscompile_detector/README.md` — the design spec for the detector itself.
2. `tools/fallback_profiler/README.md` — the instrument that measures the AOT contract's viability.
3. `tools/triage/README.md` — the one-time tool that produced `docs/TRIAGE.md`.
4. `src/foundation/arena.c` — the first file of the new tree.

---

## 14. The one-sentence summary

> **`docs/PROOF_INFRASTRUCTURE.md` is the Class B defense spec. Three components: a miscompile detector (bit-exact comparison, delta-debug reduction, structured reporting), a differential harness (Test262 + assembled npm corpus + generated programs, in CI, on every commit, with a regression database), and a translation validator (not started). The oracle is the KYJIT interpreter first; V8 is added once the interpreter is stable. The first finished feature is a detector that produces its first divergence report on the `parser → interpreter → μOp cache` pipeline. Every feature from that point forward has the detector as its gate. This is how silent wrong answers are prevented.**

---

**End of docs/PROOF_INFRASTRUCTURE.md**