# CLAUDE.md — UDDP + SPP Project Guide

This file provides context for working on the **UDDP** (Universal Dependence Detection Pipeline) and **SPP** (Speculative Polyhedral Parallelization) projects in this workspace.

---

## Repository Structure

```
/home/kumarr/kyjit/m-ky/
├── README.md                    # UDDP overview
├── uddp.sh                      # UDDP control panel script
├── Makefile                     # Convenience targets
├── stubs.md                     # Hardening ledger (H-1..H-5 DONE)
├── HANDOFF.md                   # Project handoff notes
├── CONTINUATION.md              # Migration/handoff stubs
├── docs/                        # Phase specs & status reports
│   ├── PHASE*_STATUS.md         # Per-phase implementation status
│   ├── phase*.txt, phase*.md    # Technical specifications
│   ├── stubs.md                 # Hardening ledger (copy)
│   ├── HANDOFF.md               # Handoff notes (copy)
│   └── litellm_config.yaml      # LLM config
├── m0/                          # UDDP crate (Rust)
│   ├── Cargo.toml
│   ├── README.md                # Detailed technical docs
│   ├── ENGINE_SUMMARY.md        # Architecture summary
│   ├── src/
│   │   ├── main.rs              # m0-decide binary
│   │   ├── lib.rs               # Library root
│   │   ├── checker/main.rs      # m0-check binary (independent)
│   │   ├── decide/              # Decision pipeline (M0–M13)
│   │   ├── ir/                  # IR: Loop, Access, Guard, IV, Scalar
│   │   ├── solver/              # Integer/Presburger solvers
│   │   ├── frontend/            # Parser + lowering (Phase 9)
│   │   └── certificate/         # Certificate types + emission
│   ├── tests/                   # Test suites & corpora
│   │   ├── *.sh                 # Level test runners
│   │   ├── loops/               # Baseline corpus
│   │   ├── m*_loops/            # Per-level test corpora
│   │   ├── negative/            # Soundness corpus
│   │   └── *.rs                 # Integration + fuzz tests
│   └── target/release/          # Built binaries
└── spp/                         # SPP specification
    └── work.md                  # Complete SPP spec (all 5 layers)
```

---

## UDDP — Universal Dependence Detection Pipeline

**What it does:** Given a loop nest with array accesses, decides whether two accesses can touch the same memory location and at what **dependence distance**. Every decision comes with a **machine-checkable certificate** verified by an independent checker (`m0-check`).

### Architecture: 18 Subsystems (M0–M13)

| Level | Module | Algorithm | Certificates |
|-------|--------|-----------|--------------|
| **M0** | `m0.rs` | Trivial base/offset overlap | `Exact` / `None` |
| **M1** | `m1.rs` | GCD test on strides | `Exact` / `None` |
| **M2a** | `m2a.rs` | Banerjee inequality (real-valued) | `Exact` / `None` |
| **M2a.5** | `m2a5.rs` | Extended Banerjee with symbolic bounds | `Exact` / `None` |
| **M2b** | `m2b.rs` | Interval test with direction vectors | `Exact` / `None` |
| **M2g** | `m2g.rs` | **Normalization** — expr → affine index/offset | `Rewrite` |
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

**Pipeline:** Dispatcher (`decide::decide`) routes by highest `IndexKind` in access pair → levels rewrite/delegate → **M0** is terminal affine solver.

### Certificate Discipline (Two Invariants)

1. **Invariant A — Checker Independence:** `V(loop_ir, cert) ∈ {ACCEPT, REJECT}` — checker verifies without re-running analysis
2. **Invariant B — Sound-by-Conservatism:** Output lattice `Exact < None < Unknown` — analyzer only moves toward `Unknown` when uncertain

### Current Status (Measured)

- **920 tests pass / 0 fail** (`cargo test --release`)
- **All 5 hardening items (H-1..H-5) complete and fuzz-guarded**
- **Phase 9 (frontend) complete** — `.js` files reach analyzers, certificates replay in real checker
- **Phase 8 (interprocedural) complete** — call graph, summaries, constprop, context refinement
- **7 soundness bugs found by differential fuzzing** (all fixed)
- **1,001 real loops in M0 baseline** — 601 EXACT, 400 NONE, 0 UNKNOWN, 0 checker rejections

---

## SPP — Speculative Polyhedral Parallelization

**What it does:** Parallelizes loops that cannot be parallelized statically. A stack of **five layers** that select the cheapest mechanism capable of validating each assumption.

### The Five Layers

```
SPP
│
├── 1. Dependence / legality
│   ├── UDDP M₀–M₁₀ (consumed as artifacts)
│   ├── Interprocedural alias analysis (3,500 lines)
│   ├── Dependence polyhedra (700)
│   ├── Reduction generalization (1,000)
│   ├── Recurrence decomposition (900)
│   ├── Effect / library summaries (500 + 400)
│   ├── Non-affine dependence gray zone (1,200)
│   ├── Runtime-checkable dependence (800)
│   ├── Cross-iteration alias speculation (800)
│   └── Memory-model awareness (600)
│
├── 2. Parallel Version Generation (2,100 lines)
│   ├── Version lattice generator (Pareto-optimal, not fixed A/B/C/D)
│   ├── Cross-version code dedup (instruction cache)
│   ├── Full transformation catalog:
│   │   Statement reordering, unswitching, peeling, index-set splitting
│   │   Scalar expansion, loop fusion/fission/alignment
│   │   Loop reversal, interchange, skewing, linear transformations
│   │   Strip-mining, tiling, circular skewing, striping
│   │   Loop collapsing, coalescing, interprocedural transformations
│   └── Runtime dispatcher
│
├── 3. SPP-HX — Hierarchical Execution Optimizer (11,900 lines)
│   ├── A. Hardware model (CPU topology, cache, SIMD, memory, NUMA, sync)
│   ├── B. Learned cost model (ML: gradient-boosted trees / small MLP)
│   ├── C. Bayesian search space (thread count, SIMD width, tile size, chunk, fusion, schedule, placement)
│   ├── D. Execution optimizer (hierarchical scheduling, barrier minimization, NUMA placement, cache locality, adaptive load balancing)
│   ├── E. Online re-optimization (re-JIT mid-execution)
│   ├── F. Energy/power as first-class cost
│   └── G. Profitability gate (10x speedup admission criterion)
│
├── 4. Speculation Tiers (11,500 lines)
│   ├── Tier 0: Static proof (0 cycles)
│   ├── Tier 1: Runtime guards (~10 cycles)
│   ├── Tier 2: Version/range checks (~100 cycles)
│   ├── Tier 3: Chunk speculation (~1,000 cycles)
│   ├── Tier 4: Page versioning (~10,000 cycles)
│   ├── Tier 5: Software TM (~100,000 cycles)
│   ├── Tier 6: **Optimized speculation** (~150–550 cycles) — universal fallback
│   └── Hardware fast path: TM (TSX/TME) if available (~1,000 cycles)
│   └── Three-layer rollback: registers → shadow pages → version counters
│   └── Allocator-aware rollback, nested checkpoints, I/O buffering
│
└── 5. Acceleration (3,800 lines)
    ├── Portable vector IR (LLVM VPlan / MLIR vector dialect)
    ├── Software prefetch
    ├── Cache-blocked tiling
    ├── GPU transfer cost model (PCIe vs compute tradeoff)
    └── CPU/GPU partitioning
```

### Total: ~59,500 lines (~70k with headers/tests), ~35 weeks

---

## UDDP → SPP Relationship

**SPP Layer 1 consumes UDDP as artifacts.** UDDP emits certificates; SPP reads them. The trust boundary:

| UDDP Output | SPP Consumption |
|-------------|-----------------|
| `Claim::Exact(d)` | Wavefront with step `d` |
| `Claim::None` | Doall parallelization |
| `Claim::Direction(D)` | Direction vector tests for interchange |
| `Claim::Level(k)` | Determines which loop level carries dependence |
| `Claim::Parametric` | Runtime specialization |
| `Claim::RuntimeCheck` | Tier 1/2 speculation guards |
| `Claim::CrossLoop` | Fusion legality |
| `Claim::Reduction` | Parallel reduction trees |
| `Claim::TripCount` | Peeling, fusion, scheduling |
| `Claim::ScalarDependence` | Scalar expansion/privatization |

**UDDP is complete through M13** (18 subsystems). SPP Layer 1 needs:
- Interprocedural alias analysis (Phase 8 done, but needs field-sensitivity)
- Dependence polyhedra (M6 extension)
- Non-affine gray zone (new `IndexKind::Range`)
- Reduction generalization (new `Claim::Reduction`)
- Cross-iteration alias speculation (SPP-only, not certifiable)
- Effect summaries (IPA complete)

---

## Key Commands

### UDDP (from `/home/kumarr/kyjit/m-ky/m0/`)

```bash
# Build
cargo build --release

# Run analyzer on a loop
./target/release/m0-decide tests/m2g_loops/g001.json

# Verify certificate
./target/release/m0-check tests/m2g_loops/g001.json cert.json

# Full regression (from repo root)
../uddp.sh all

# Per-level test suite
./tests/run_m4_tests.sh

# Library tests
cargo test --release --lib

# Fuzzing
cargo test --release --lib fuzz
```

### UDDP Control Panel (from repo root `/home/kumarr/kyjit/m-ky/`)

```bash
./uddp.sh all              # Full regression
./uddp.sh lib              # cargo test --lib
./uddp.sh integration      # All E2E test suites
./uddp.sh negative         # Soundness corpus
./uddp.sh level m4         # Single level suite
./uddp.sh build            # cargo build --release
./uddp.sh clean            # cargo clean
./uddp.sh bench            # Run benchmarks
./uddp.sh check            # Verify certificates
```

---

## Adding a New UDDP Level

1. **Extend IR** (if needed): Add variant to `IndexKind` in `src/ir/mod.rs`
2. **Write decision procedure**: Create `src/decide/mXX.rs` with `pub fn decide(loop_, p, q) -> (Decision, Option<Certificate>)`
3. **Define witness type** in `src/certificate/mod.rs` if needed
4. **Implement checker support** in `src/checker/main.rs` (`check_one`)
5. **Wire into dispatcher** in `src/decide/mod.rs` (correct precedence order)
6. **Write differential fuzzer** in `mXX.rs` — compare `decide()` against brute-force simulation
7. **Add test corpus** in `tests/mXX_loops/`
8. **Add test runner** `tests/run_mXX_tests.sh`
9. **Run regression**: `cargo test --release --lib && ./tests/run_all.sh`

**Critical:** Every level MUST have a differential fuzzer. The fuzzer generates random inputs, computes ground truth by enumerating IVs, runs `decide()`, and asserts zero unsound results.

---

## Important Files for Reference

| File | Purpose |
|------|---------|
| `m0/README.md` | Detailed technical docs, IR, certificates, adding levels |
| `m0/ENGINE_SUMMARY.md` | Module breakdown, line counts, test coverage, API |
| `docs/stubs.md` | Hardening ledger, precision extensions, process gates |
| `docs/HANDOFF.md` | Measured status, live tasks, execution order |
| `docs/PHASE9_STATUS.md` | Phase 9 (frontend) complete status |
| `docs/PHASE8_STATUS.md` | Phase 8 (interprocedural) complete status |
| `docs/PHASE4_STATUS.md` | Phase 4 (FUD consumers) status |
| `spp/work.md` | Complete SPP specification (all 5 layers) |

---

## Development Rules (from HANDOFF.md)

1. **ONE terminal only** — machine chokes with parallel processes
2. **No `sleep` polling loops** — run command, read output, move on
3. **Verify, don't trust prose** — grep source before repeating claims
4. **Always gate with `cargo test --release`** — debug too slow for full suite
5. **Run harness scripts serially** — `run_real.sh`, `run_all.sh`, `run_negative.sh` use `cert.json` in CWD
6. **`stubs.md` is the contract** — update it as you close items

---

## UDDP Binaries (built with `cargo build --release`)

| Binary | Purpose |
|--------|---------|
| `m0-decide` | Analyze a single loop JSON, emit `cert.json` |
| `m0-check` | Verify `cert.json` against loop JSON |
| `m0-batch` | Batch process 1,001-loop corpus |
| `m0-compile` | Compile `.udd` source to loop JSON |
| `m0-parallel` | Parallel batch with rayon |
| `m0-stream` | Streaming JSONL processor |
| `m0-pack` / `m0-packed` | Packed binary format |
| `m0-arena` | Arena-based analyzer path (experimental) |
| `m1-decide` / `m1-distance` | M1-specific entry points |
| `debug_p004` | Debug helper |

---

## Test Corpora

| Directory | Purpose |
|-----------|---------|
| `tests/loops/` | 10 baseline loops |
| `tests/real_loops/` | 1,001 real loops (M0 baseline) |
| `tests/m2g_loops/` ... `tests/m13_loops/` | Per-level test loops |
| `tests/negative/` | Soundness corpus (checker MUST reject) |
| `tests/js_kernels/` | JavaScript kernels for Phase 9 frontend |
| `tests/phase*_loops/` | Integration test fixtures |

---

## Phase Completion Status

| Phase | Focus | Status |
|-------|-------|--------|
| 1 | Certificate foundations | ✅ DONE |
| 2 | Math engine (Ch. 4 + 7.1–7.3) | ✅ DONE |
| 3 | FUD chains (Ch. 6.1–6.3) | ✅ DONE |
| 4 | Scalar analysis (Ch. 6.4–6.7) | ✅ DONE (M3 rewired; M5/M8 blocked) |
| 5 | Subsystems (Ch. 7.5, 7.6, 8.3, 8.4) | ✅ DONE |
| 6 | Array regions (Ch. 8.1) | ✅ DONE |
| 7 | Points-to / aliasing (Ch. 8.2) | ✅ DONE |
| 8 | Interprocedural (Ch. 8.5) | ✅ DONE |
| 9 | Frontend extensions | ✅ DONE |

---

## What Remains for SPP Layer 1 (from stubs.md §3)

| Item | Bucket | Description |
|------|--------|-------------|
| 2. `ScalarExpr` beyond affine (Div, Mod, Min, Max, Cmp) | (I) | Extends `IndexExpr` inside `IndexKind::Affine` |
| 3. Non-affine gray zone (range-based runtime disambiguation) | (I) | New `IndexKind::Range` arm |
| 4. Reduction generalization (user-defined AC ops) | (A) | New `Claim::Reduction` + checker re-proves AC/identity |
| 5. Cross-iteration alias speculation | (A) or (I) | **Reclassify as SPP-only** — empirical, not certifiable |
| 6. Memory-model awareness (atomics, fences) | (C) | New `Claim::Ordering` + IR extension |
| 7. Library-effect summaries | (A) | New artifact family for external libraries |
| 8. Dependence polyhedra (full multi-dim) | (I) | M6 extension with polyhedral form |

**MUST NOT be hardened away** (stubs.md §4):
- Enumeration caps (M6: 4k products, M7: 64k hash, M8: 4k recurrence, M10: 64k table)
- A1/A5 type quarantine (RuntimeCheckCertificate consumed only by SPP Layer 4)
- Artifact-only consumption by SPP (no analyzer callbacks)

---

## Quick Start for New Work

```bash
# 1. Verify baseline
cd /home/kumarr/kyjit/m-ky/m0
cargo test --release

# 2. Check current stubs
cat ../docs/stubs.md | head -150

# 3. Run a specific level test
./tests/run_m4_tests.sh

# 4. Full regression from root
cd ..
./uddp.sh all
```

---

## Notes for Implementation

- **Release builds use `overflow-checks = true`** in `m0/Cargo.toml` — all arithmetic in checker and certificate-feeding paths must use `checked_*` operations
- **Certificates carry `loop_hash`** (SHA256 of canonical loop JSON) — if a level rewrites the loop, it MUST overwrite `cert.loop_hash = loop_.hash()` on the ORIGINAL loop before returning
- **Fuzzing methodology** (stubs.md §18): seed-and-revert validation mandatory; brute-force oracles; SerDe round-trips; corpus regression on every phase boundary
- **Crate topology** (stubs.md §19): edges point left; `uddp-arith` is the ONLY crate shared by analyzer and checker
- **Definition of done** (stubs.md §20): corpus ACCEPTs, seed-and-revert clean, A/B batch behavior explained, checker contains no analyzer code, SPP touches UDDP only through artifacts A1–A5