# UDDP — Universal Dependence Detection Pipeline

**A Rust dependence analyzer for JavaScript-style loops that emits machine-checkable certificates.**

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()
[![Tests](https://img.shields.io/badge/tests-582%20lib%20%2B%20integration-blue)]()
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)]()

---

## What UDDP Does

Given a loop nest with array accesses, UDDP decides whether two accesses can touch the same memory location and at what **dependence distance**.

```javascript
for (i = 0; i < N; i++)
    A[i] = A[i + 1];
```

**UDDP decides:**
- **Dependence?** Yes — the write to `A[i]` and the read from `A[i+1]` are the same location when the loop carries the dependence
- **Distance?** `-1` (the write at iteration `i` conflicts with the read at iteration `i+1`)
- **No dependence?** For `A[i]` vs `A[i + 100]` on a short loop, UDDP proves disjointness and returns `None`

**Every decision comes with a certificate** that an independent checker (`m0-check`) verifies. The checker does not re-run the analysis; it replays the witness against the loop file on disk.

---

## Quick Start

```bash
# Enter the tree
cd ~/kyjit/m-ky/m0

# Build everything (release with overflow checks)
cargo build --release

# Analyze a loop
./target/release/m0-decide tests/m2g_loops/g001.json

# Verify the emitted certificate
./target/release/m0-check tests/m2g_loops/g001.json cert.json

# Run one level's test suite
./tests/run_m2g_tests.sh

# Run the full regression (from repo root)
../uddp.sh all
```

---

## Architecture in One Picture

```
Loop JSON
    │
    ▼
┌──────────────────────────────────────────────┐
│           Dispatcher (decide::decide)         │
│  Routes to the right level based on which    │
│  IR features the loop carries                │
└──────────────────────────────────────────────┘
    │
    ▼  tries in order:
    │
    │  M13 (assertions)   ┐
    │  M10 (fixed point)  │  Pre-M0 routes:
    │  M8  (recurrence)   │  any level that rewrites
    │  M7  (hash)         │  the loop before M0 sees it
    │  M6  (products)     │
    │  M11 (symbolic FP)  │
    │  M9  (mod-p)        │
    │  M2g, M2a, M2a.5   │
    │  M4, M2b, M5, M3   ┘
    │
    ▼
┌──────────────────────────────────────────────┐
│        M0 — constant-bound affine            │
│  Every lower level delegates here after      │
│  rewriting the loop into affine form         │
└──────────────────────────────────────────────┘
```

---

## The Certificate Discipline

**Invariant A — Checker Independence:** Every semantic output carries a certificate that `m0-check` verifies without trusting the analyzer. `V(loop_ir, cert) ∈ {ACCEPT, REJECT}`, total, deterministic, resource-bounded.

**Invariant B — Sound-by-Conservatism:** Output lattice `Exact < None < Unknown` in informativeness. The analyzer moves only toward `Unknown` when uncertain. A wrong `Exact` and a wrong `None` are the two catastrophic outputs. `Unknown` is always safe.

**Corollary A1 — Certificate Completeness:** Nothing influences a decision that is not (a) replayed from the certificate, or (b) re-derivable by the checker from the IR snapshot. "Trust-me" fields forbidden.

---

## Levels Shipped (18 Subsystems)

| Level | Module | Algorithm | Certificates |
|-------|--------|-----------|--------------|
| **M0** | `m0.rs` | Trivial base/offset overlap | `Exact` / `None` |
| **M1** | `m1.rs` | GCD test on strides | `Exact` / `None` |
| **M2a** | `m2a.rs` | Banerjee inequality (real-valued) | `Exact` / `None` |
| **M2a.5** | `m2a5.rs` | Extended Banerjee with symbolic bounds | `Exact` / `None` |
| **M2b** | `m2b.rs` | Interval test with direction vectors | `Exact` / `None` |
| **M2g** | `m2g.rs` | **Normalization** — converts `expr` → affine index/offset | `Rewrite` |
| **M3** | `m3.rs` | Guard range narrowing | `Rewrite` / `TripCount` |
| **M4** | `m4.rs` | **Exact Presburger solver** (Omega-style) | `Rewrite` / `Exact` |
| **M5** | `m5.rs` | Scalar evolution / induction analysis | `Rewrite` / `TripCount` |
| **M6** | `m6.rs` | Multi-dimensional exact solving | `Exact` |
| **M7** | `m7.rs` | Symbolic pointer chase | `Rewrite` |
| **M8** | `m8.rs` | Non-linear recurrences | `Exact` |
| **M9** | Frontend | Rust-like loop language → IR | — |
| **M10** | `m10.rs` | Advanced alias analysis | `Exact` / `None` |
| **M11** | `m11.rs` | Interprocedural analysis | `Exact` / `None` |
| **M12** | `m12.rs` | Context-sensitive analysis | `Exact` / `None` |
| **M13** | `m13.rs` | Subdomain enumeration fallback | `AssertionChain` |

**Key Entry Point:**
---

## Directory Layout

```
m-ky/
├── README.md              # This file
├── uddp.sh                # Control panel script (run all tests)
├── Makefile               # Convenience targets
├── stubs.md               # Hardening ledger (all DONE)
├── HANDOFF.md             # Project handoff notes
├── PHASE*_STATUS.md       # Phase status documents
└── m0/                    # The UDDP crate
    ├── Cargo.toml
    ├── README.md          # Detailed technical docs
    ├── ENGINE_SUMMARY.md  # Architecture summary
    ├── src/
    │   ├── main.rs        # m0-decide binary
    │   ├── lib.rs         # Library root
    │   ├── checker/
    │   │   └── main.rs    # m0-check binary (independent)
    │   ├── decide/        # Decision pipeline (M0–M13)
    │   ├── ir/            # IR: Loop, Access, Guard, IV, Scalar
    │   ├── solver/        # Integer/Presburger solvers
    │   ├── frontend/      # Parser + lowering (Phase 9)
    │   └── certificate/   # Certificate types + emission
    ├── tests/
    │   ├── *.sh           # Level test runners
    │   ├── loops/         # Baseline corpus
    │   ├── m*_loops/      # Per-level test corpora
    │   ├── negative/      # Soundness corpus (checker REJECT)
    │   └── *.rs           # Integration + fuzz tests
    └── target/release/    # Built binaries
```

---

## Building

```bash
cd m0

# Standard release build (overflow-checks enabled in Cargo.toml)
cargo build --release

# Debug build
cargo build

# Build specific binary
cargo build --release --bin m0-decide
cargo build --release --bin m0-check
```

**Binaries produced:**
| Binary | Purpose |
|--------|---------|
| `m0-decide` | Analyze a loop JSON, emit `cert.json` |
| `m0-check` | Verify `cert.json` against loop JSON |
| `m0-batch` | Batch process a directory of loops |
| `m0-compile` | Compile `.udd` source to loop JSON |
| `m0-parallel` | Parallel batch with rayon |
| `m0-stream` | Streaming JSONL processor |
| `m1-decide` | M1-only decision (GCD test) |
| `m1-distance` | Distance enumeration for M1 |

---

## Running the Analyzer

### Input Format (Loop JSON)

```json
{
  "depth": 1,
  "ivs": [{ "name": "i", "lower": 0, "upper": 100, "step": 1 }],
  "body": [
    {
      "kind": "Write",
      "base": "A",
      "index": [1],
      "offset": { "Const": 0 },
      "elem_size": 8,
      "index_kind": "Affine",
      "indirect": false
    },
    {
      "kind": "Read",
      "base": "A",
      "index": [1],
      "offset": { "Const": 1 },
      "elem_size": 8,
      "index_kind": "Affine",
      "indirect": false
    }
  ],
  "scalars": [],
  "guards": []
}
```

### Basic Usage

```bash
# Decide all pairs in a loop
m0-decide loop.json

# Output:
# pair (0, 1): EXACT distance [-1]
# pair (1, 0): EXACT distance [1]
# overall: EXACT
# wrote cert.json
```

### With Environment Variables (M9 parametric)

```bash
M0_ENV="p=8,modulus=4" m0-decide loop.json
```

---

## Running the Checker

```bash
# Verify certificate
m0-check loop.json cert.json

# Output: ACCEPT  (or REJECT with reason)
```

The checker:
- Accepts both single-object and array certificate formats
- Verifies **every** certificate in the file (conjunction)
- Re-computes the witness from the loop on disk
- Exits 0 on ACCEPT, 1 on REJECT, 2 on usage error

---

## Running the Test Suite

---

## Control Panel Script (uddp.sh)

From the repo root (`m-ky/`):

```bash
./uddp.sh all          # Full regression
./uddp.sh lib          # cargo test --lib
./uddp.sh integration  # All E2E test suites
./uddp.sh negative     # Soundness corpus
./uddp.sh level M2g    # Single level suite
./uddp.sh build        # cargo build --release
./uddp.sh clean        # cargo clean
./uddp.sh bench        # Run benchmarks
./uddp.sh help         # Show help
```

---

## The IR (Intermediate Representation)

### Core Types

```rust
// Loop nest
pub struct Loop {
    pub depth: usize,
    pub ivs: Vec<IV>,                    // Induction variables
    pub body: Vec<Access>,               // Memory accesses
    pub scalars: Vec<ScalarVar>,         // Scalar variables
    pub stmts: Option<Vec<LoopStmt>>,    // Scalar statements
    pub guards: Vec<Guard>,              // Loop guards
    pub reductions: Vec<Reduction>,      // Reduction ops
    pub func_summaries: Vec<FunctionSummary>,
}

// Induction variable
pub struct IV { pub name: String, pub lower: i64, pub upper: i64, pub step: i64 }

// Array access
pub struct Access {
    pub kind: AccessKind,                // Read / Write / ReadWrite
    pub base: String,                    // Array name
    pub index: Vec<i64>,                 // Affine coefficients
    pub elem_size: i64,                  // Element size in bytes
    pub offset: OffsetValue,             // Const(i64) or Param(name)
    pub expr: Option<String>,            // Original expression (for M2g)
    pub index_kind: IndexKind,           // Affine/Product/Hash/Recurrence/FixedPoint
    pub indirect: bool,                  // Pointer chase
    pub guard: Option<Guard>,            // Per-access guard
    pub range: Option<RangeExpr>,        // Value range
}

// Scalar expression (for lowering)
pub enum ScalarExpr {
    Iv(usize), Scalar(usize), Const(i64),
    Add(Box<ScalarExpr>, Box<ScalarExpr>),
    Mul(Box<ScalarExpr>, Box<ScalarExpr>),
    Load { base: String, index: Box<ScalarExpr> },
}
```

### Index Kinds (the dispatch key)

```rust
pub enum IndexKind {
    Affine,                              // M0–M5
    Product(Vec<usize>),                 // M6
    Hash(usize),                         // M7
    Recurrence(RecurrenceKind),          // M8
    FixedPoint,                          // M10
    SymbolicFixedPoint { depth: usize }, // M11
}
```

The dispatcher (`decide::decide`) routes based on the highest `IndexKind` present in the access pair.

---

## The Certificate Format

### Claim Types

```rust
pub enum Claim {
    Exact(Distance),        // Proven dependence with distance vector
    None,                   // Proven independent
    Unknown(Reason),        // Could not decide
    Direction(Direction),   // Direction vector (<, =, >, *)
    Level(usize),           // Dependence level (M0–M13)
    CrossLoop(Distance),    // Cross-loop dependence
    Parametric(ParametricClaim),  // Parametric (M9)
    TripCount,              // H-4: Loop trip count
    ScalarDependence,       // Scalar dependence
    ArrayRegion,            // Phase 6
    FunctionSummary,        // Phase 7
    RuntimeCheck,           // Phase 8
}
```

### Witness Types

```rust
pub enum Witness {
    IntSolution { i: Vec<i64>, j: Vec<i64> },           // Exact/None
    UnsatProof { constraints: Vec<Constraint> },        // None
    DirectionSolution { i: Vec<i64>, j: Vec<i64> },     // Direction
    CrossLoopSolution { ... },                          // CrossLoop
    AssertionChain { assertions: Vec<Assertion>, inner: Box<Witness> }, // M13
    Rewrite { rule: RewriteRule, original: Loop, rewritten: Loop, inner: Box<Witness> }, // H-3
    TripCountEvidence { cfg: CFG, iv_var: usize, loop_: usize, bound: i64, bound_step: i64, companion_step: Option<i64> }, // H-4
    ScalarDependenceEvidence { ... },
    ArrayRegionEvidence { ... },
    FunctionSummaryEvidence { ... },
    RuntimeCheckEvidence { ... },
}
```

### Rewrite Rules (H-3)

```rust
pub enum RewriteRule {
    M2gNormalize,    // expr → affine index/offset via constprop
    M2aFlatten,      // flatten multi-dim index
    M3Recursion,     // mutual recursion unfolding
    M4GuardNarrow,   // guard intersection
    M5ChaseExpand,   // pointer chase expansion
}
```

The checker **re-applies the named rule** to `original` and requires byte-for-bit equality with `rewritten` before verifying `inner`.

---

## Adding a New Level

1. **Create `src/decide/mX.rs`** with `pub fn decide(...) -> (Decision, Option<Certificate>)`
2. **Define witness type** in `certificate/mod.rs` if needed
3. **Implement checker support** in `checker/main.rs` (`check_one`)
---

## What Fits UDDP vs What Doesn't

| ✅ Fits UDDP | ❌ Doesn't Fit |
|--------------|----------------|
| Affine array accesses in loops | General pointer arithmetic |
| Loop nests with constant bounds | Unbounded `while` loops |
| Guard/conditional narrowing | Dynamic memory allocation |
| Scalar evolution / induction vars | Function calls with side effects |
| Parametric bounds (M9) | Recursive data structures |
| Interprocedural summaries (M11) | Concurrent/parallel semantics |

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `REJECT: loop hash mismatch` | Regenerate `cert.json` with current `m0-decide`; the loop file changed |
| `REJECT: malformed certificate` | JSON parse error — check `cert.json` syntax |
| `REJECT: witness arity` | IV count mismatch — loop depth changed |
| `cargo test` fails on overflow | Use `checked_add`/`checked_mul` everywhere in checker; overflow ⇒ REJECT |
| `decide` returns `Unknown` | Check if loop uses unsupported `IndexKind`; add normalization (M2g) |

### Common Commands

```bash
# Regenerate all certificates
for f in tests/loops/*.json; do
  (cd /tmp && m0-decide "$f" 2>/dev/null)
done

# Find which level decided a loop
M0_TIMING=1 m0-decide loop.json

# Debug a specific pair
m1-decide loop.json 0 1

# Run fuzz for one level
cargo test --lib --release decide::m4::tests
```

---

## Fuzzing

Every `decide` function has a brute-force differential fuzzer as a `#[test]` property test. These compare `decide()` against an independent simulation of the loop's semantics — they do not trust the analyzer's reasoning, only its output.

```bash
# Run all fuzz tests
cargo test --lib --release fuzz

# Run specific level fuzz
cargo test --lib --release decide::m4::tests::fuzz
```

**7 soundness bugs found by fuzzing** (all invisible to hand-written tests):
1. `OffsetValue::as_const` silent collapse (934/2352 mismatches)
2. M7 hash comparison ignored offsets (189/500 mismatches)
3. M5 chase value model wrong (185/2000 mismatches)
4. M13 `Ne` assertions as no-ops (15/3000 violations)
5. M3/M3.5 step-s ranges treated as step-1 (274/1217 mismatches)
6. M4 guard intersection missed one-sided guards (146/1938)
7. M1 unrealizable distances (54/5578)

**Certificate pipeline cannot detect bugs 3, 5, 6, or 7.** Only differential fuzzing catches analyzer *model* bugs.

---

## Hardening Status

| Item | Description | Status |
|------|-------------|--------|
| **H-1** | Symbolic→constant degradation in emission | ✅ DONE |
| **H-2** | Checker arithmetic overflow | ✅ DONE |
| **H-3** | Rewrite witness (M2g→M4 provenance) | ✅ DONE |
| **H-4** | M3.5 bounded-unfold accounting | ✅ DONE |
| **H-5** | Multi-level firing precedence tests | ✅ DONE |

**All 5 hardening items complete.** See `stubs.md` for details.

---

## Using as a Library

```toml
# Cargo.toml
[dependencies]
m0 = { path = "../m-ky/m0" }
```

```rust
use m0::{ir::*, decide::{decide, Decision}};

let loop_ = Loop { /* ... */ };
let (decision, cert) = decide(&loop_, 0, 1);

match decision {
    Decision::Exact(d) => println!("Dependence at distance {:?}", d),
    Decision::None => println!("Proven independent"),
    Decision::Unknown(r) => println!("Unknown: {}", r),
}
```

### With Frontend (Phase 9)

```rust
use m0::frontend::{parse, lower};

let src = r#"
for i in 0..N {
    A[2*i + 1] = B[i];
    C[i] = A[2*i];
}
"#;

let ast = parse(src).unwrap();
let (loops, _) = lower(&ast).unwrap();
let (decision, cert) = m0::decide(&loops[0], 0, 1);
```

---

## Performance

| Phase | Complexity | Typical Latency |
|-------|------------|-----------------|
| M0–M1 | O(1) | < 0.1 ms |
| M2a–M2b | O(n) | < 0.5 ms |
| M2g | O(expr size) | < 1 ms |
| M3 | O(guards) | < 1 ms |
| M4 | NP-hard (fast for affine) | < 10 ms |
| M5–M6 | Polyhedral | < 50 ms |
| M7–M8 | Exponential worst-case | < 100 ms |

---

## Dependencies

- `serde` + `serde_json` — Serialization
- `num` — Big integers / rationals
- `itertools` — Iterator utilities
- `rand` — Fuzzing (dev only)
- `clap` — CLI (bins only)
- `rayon` — Parallel batch
- `bincode` — Binary serialization
- `sha2` — Loop hashing

---

## Documentation

Detailed specifications and phase status reports are in [`docs/`](docs/):

- **Phase Status Reports** — `docs/PHASE*_STATUS.md` — implementation status per phase
- **Phase Specifications** — `docs/phase*.txt`, `docs/phase*.md` — detailed technical specs
- **Engine Summary** — `m0/ENGINE_SUMMARY.md` — M0/M1/M4/M6/M9 engine architecture
- **Stubs Tracker** — `docs/stubs.md` — hardening items (H-1 through H-5) completion log
- **Handoff Notes** — `docs/HANDOFF.md` — project handoff documentation

---

## License

MIT OR Apache-2.0

---

## Credits

Built as the **UDDP** (Universal Dependence Detection Pipeline) — a certifying dependence analyzer for JavaScript-style loops with 18 shipped subsystems, ~9,500 lines of Rust, 582+ library tests, and zero regressions on the M0 baseline (1,001 real loops).
4. **Add to dispatcher** in `decide/mod.rs` (correct precedence order)
5. **Write fuzz tests** in `mX.rs` — compare `decide()` against brute-force simulation
6. **Add test corpus** in `tests/mX_loops/`
7. **Add test runner** `tests/run_mX_tests.sh`
8. **Update `ARCHITECTURE.md`** (if exists) before code

**Critical:** Every level must have a differential fuzzer. The fuzzer generates random inputs, computes ground truth by enumerating IVs, runs `decide()`, and asserts zero unsound results.
### From m0/ directory

```bash
# All library tests (582 tests)
cargo test --lib --release

# Integration tests
cargo test --test phase4_e2e --release
cargo test --test phase9_kernels --release
cargo test --test phase9a_evidence --release

# Per-level test suites
./tests/run_m2g_tests.sh
./tests/run_m2a_tests.sh
./tests/run_m3_tests.sh
./tests/run_m4_tests.sh
./tests/run_m5_tests.sh
./tests/run_m6_tests.sh
./tests/run_m7_tests.sh
./tests/run_m8_tests.sh
./tests/run_m9_tests.sh
./tests/run_m10_tests.sh
./tests/run_m11_tests.sh
./tests/run_m12_tests.sh
./tests/run_m13_tests.sh

# Negative (soundness) corpus — checker MUST reject
./tests/run_negative.sh

# Phase 9 frontend tests
./tests/run_phase9_tests.sh

# Full regression
./tests/run_all.sh
```

### From repo root (using uddp.sh)

```bash
# All tests
./uddp.sh all

# Specific suites
./uddp.sh lib
./uddp.sh integration
./uddp.sh negative
./uddp.sh level m2g
./uddp.sh level m4
```
```rust
use m0::decide::{decide, Decision};

let (decision, cert) = decide(&loop_, 0, 1);
```