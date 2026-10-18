# SPP — SPECULATIVE POLYHEDRAL PARALLELIZATION

**Document:** `docs/SPP.md`
**Status:** Final specification
**Depends on:** UDDP M₀–M₁₀, Sea of Nodes IR, Backend
**Depended on by:** DPS, GPU scheduling, the full engine
**Version:** Final. Frozen.

---

## 0. What SPP is

**SPP parallelizes loops that cannot be parallelized statically.**

It is not one technique. It is a stack of five layers that select the cheapest mechanism capable of validating each assumption.

The core question is not *"Can this loop be parallelized?"*

The core question is:

> **"What execution configuration maximizes useful work per cycle while satisfying the proof?"**

---

## 1. The five layers

```
SPP
│
├── 1. Dependence / legality
│   ├── UDDP M₀–M₁₀
│   ├── Interprocedural alias analysis
│   ├── Dependence polyhedra
│   ├── Reduction generalization
│   ├── Recurrence decomposition
│   └── Effect / library summaries
│
├── 2. Parallelization
│   ├── Wavefront
│   ├── Loop parallelism
│   ├── Task parallelism
│   └── Hierarchical decomposition
│
├── 3. SPP-HX — Hierarchical Execution Optimizer
│   ├── Hardware model
│   ├── Learned cost model
│   ├── Bayesian search space
│   ├── Execution optimizer
│   ├── Online re-optimization
│   └── Profitability gate
│
├── 4. Speculation
│   ├── Tier 0: Static proof
│   ├── Tier 1: Runtime guards
│   ├── Tier 2: Version / range checks
│   ├── Tier 3: Chunk speculation
│   ├── Tier 4: Page versioning
│   ├── Tier 5: Software TM
│   └── Tier 6: Optimized speculation
│
└── 5. Acceleration
    ├── Portable vector IR
    ├── Prefetch
    ├── CPU / GPU partitioning
    └── GPU kernels
```

---

## 2. Layer 1 — Dependence Engine

### 2.1 What it does

Given a loop, determine the exact dependence structure. Not just `EXACT` / `NONE` / `UNKNOWN`. Ten types:

| Type | Meaning | Action |
|---|---|---|
| `EXACT(d)` | Distance is exactly d | Wavefront with step d |
| `BOUNDED(d_max)` | Distance is at most d_max | Wavefront with d_max |
| `PARAMETRIC(d(n))` | Distance is a function of parameters | Specialize at runtime |
| `CONDITIONAL(guard)` | Dependence exists only if guard holds | Runtime check |
| `REDUCTION(op)` | Recognized reduction | Private partial states |
| `RECURRENCE(scan)` | Recognized recurrence | Prefix scan |
| `MAY_DEPEND` | May depend | Speculate |
| `MUST_DEPEND` | Must depend | Serial |
| `RUNTIME_CHECKABLE(test)` | Cheap runtime test decides | Version B |
| `UNKNOWN` | Genuinely undecidable | Speculate or serial |

### 2.2 Components

| # | Component | What it does | Lines |
|---|---|---|---|
| 1.1 | UDDP M₀–M₁₀ | Static dependence proof | 4,900 |
| 1.2 | **Interprocedural alias analysis** | Context-sensitive, field-sensitive, summary-cached | 3,500 |
| 1.3 | Dependence polyhedra | Multi-dimensional dependence | 700 |
| 1.4 | **Non-affine dependence** | Gray-zone tier: range-based runtime disambiguation | 1,200 |
| 1.5 | Runtime-checkable dependence | Generates cheap runtime tests | 800 |
| 1.6 | **Reduction generalization** | Arbitrary associative-commutative via idempotence proof | 1,000 |
| 1.7 | Recurrence decomposition | Prefix scans, parallel scans | 900 |
| 1.8 | **Cross-iteration alias speculation** | Profile-guided, cached dependence profile | 800 |
| 1.9 | Memory-model awareness | Atomics, fences, acquire/release | 600 |
| 1.10 | Effect summaries | READ / WRITE / ALLOCATE / THROW / IO / UNKNOWN | 500 |
| 1.11 | Library-effect summaries | memcpy, network_send, etc. | 400 |

**Total: ~15,300 lines**

### 2.3 Interprocedural alias analysis

**The single biggest determinant of real-world coverage.**

Without this, any loop calling a non-inlined function collapses to `MAY_DEPEND` and the whole pipeline is starved.

| Property | What it means |
|---|---|
| **Interprocedural** | Analyzes across function boundaries |
| **Context-sensitive** | Different call sites get different results |
| **Field-sensitive** | Distinguishes `obj.x` from `obj.y` |
| **Summary-cached** | Function summaries reused across calls |

**Lines: 3,500**

### 2.4 Non-affine dependence (gray zone)

Real code has:
- Array-of-structs
- Indirect indexing (`A[B[i]]`)
- Pointer chasing

**The gray zone tier:** range-based runtime disambiguation with symbolic bounds.

Not full affine. Not full speculation. The middle.

**Lines: 1,200**

### 2.5 Reduction generalization

Not just sum/max/histogram. **Arbitrary associative-commutative user-defined reductions** detected via idempotence/commutativity proofs on the operation.

So custom types (not just scalars) get parallel reduction trees.

**Lines: 1,000**

### 2.6 Cross-iteration alias speculation

Instrument once. Cache the dependence profile. Use PGO-style data to seed the classifier instead of guessing cold.

**Lines: 800**

### 2.7 Effect summaries

Every operation carries an effect summary:

```
READ
WRITE
ALLOCATE
THROW
BLOCK
ATOMIC
IO
UNKNOWN
```

Parallelization legality incorporates them.

### 2.8 Library-effect summaries

```
memcpy:       READ src, WRITE dst, NO_THROW, NO_GLOBAL_SIDE_EFFECT
network_send: IO, UNKNOWN_EXTERNAL_STATE, BLOCKING
```

---

## 3. Layer 2 — Parallel Version Generation

### 3.1 What it does

Every loop generates a **lattice of versions**, not a fixed A/B/C/D. Versions are parameterized by which assumptions are checked.

### 3.2 The version lattice

```
Version = (assumption_set, guard_set, dispatch_cost)
```

| Version | Assumptions checked | When used |
|---|---|---|
| A | All (static) | Always fast |
| B | Runtime checks | When static fails |
| C | Speculative | When checks fail |
| D | Serial | Fallback |

**The compiler picks the Pareto-optimal set at compile time based on expected input distributions.**

### 3.3 Cross-version code sharing

Dedupe the hot inner loop body across A–D. Only the guard/dispatch shell differs.

**Why:** Instruction cache pressure. Code size. Without dedup, 4 versions = 4x code.

### 3.4 Components

| # | Component | Lines |
|---|---|---|
| 2.1 | Version lattice generator | 600 |
| 2.2 | Pareto selector | 500 |
| 2.3 | Cross-version dedup | 700 |
| 2.4 | Runtime dispatcher | 300 |

**Total: ~2,100 lines**

---

## 4. Layer 3 — SPP-HX (Hierarchical Execution Optimizer)

### 4.1 A — Hardware model

| Component | What it reads | Lines |
|---|---|---|
| CPU topology | P-cores, E-cores, SMT | 400 |
| Cache hierarchy | L1/L2/L3 sizes, associativity | 300 |
| SIMD capabilities | AVX2, AVX-512, NEON | 200 |
| Memory bandwidth | Channels, speed | 300 |
| NUMA topology | Nodes, distances | 300 |
| Sync costs | Barrier, atomic, launch | 400 |

**Total: ~1,900 lines**

### 4.2 B — Analytic cost model

**A hand-written `T = T_compute + T_memory + ...` sum is the correct approach for production compilers.**
(LLVM, GCC, V8, HotSpot, Graal, Go, Rust, Swift, .NET all use analytic cost models.)

The analytic cost model computes log2(speedup) using a roofline model with cache hierarchy:

```
log2(speedup) = log2(baseline_time / transformed_time)

baseline_time = total_ops * baseline_cycles_per_op

transformed_time = max(compute_time, memory_time) + sync_time + tile_overhead

compute_time = total_ops / (flops_per_cycle_per_core * cores * simd_factor)

memory_time = total_bytes / (bytes_per_cycle_per_core * cores) + cache_miss_cycles

cache_miss_cycles = hierarchical cache model (L1/L2/L3/DRAM)

sync_time = (if parallel) thread_start_cycles + sync_barrier_cycles * log2(threads)

tile_overhead = (if tiled) tile_overhead_cycles * num_tiles
```

No ML model, no training, no retraining at install. Coefficients are calibrated by hand against benchmarks.

**Lines: 1,500**

### 4.3 C — Bayesian search space

**Brute-force argmin over (P, V, B, C) is combinatorially infeasible.**

Use **Bayesian optimization** or **learned cost-model-guided search** (like Ansor's evolutionary search + cost model).

| Dimension | Range |
|---|---|
| Thread count (P) | 1 to num_cores |
| SIMD width (V) | 128, 256, 512 bits |
| Tile size (B) | L1 / L2 / L3-resident |
| Chunk size (C) | Adaptive |
| Fusion | Which loops to fuse |
| Schedule | Static / dynamic / guided |
| Placement | NUMA node, core type |

**Lines: 2,000**

### 4.4 D — Execution optimizer

| Component | What it does | Lines |
|---|---|---|
| Static partitioning | Compile-time partition | 700 |
| Hierarchical scheduling | Program → Region → Loop → Tile → Worker → SIMD | 800 |
| Barrier minimization | Only sync where dependence requires | 600 |
| NUMA placement | First-touch, local scheduling | 500 |
| Cache locality | Tile placement | 600 |
| Adaptive load balancing | Work stealing, chunk adjustment | 700 |

**Total: ~3,900 lines**

### 4.5 E — Online re-optimization

**The runtime controller re-JITs a hot loop mid-execution if calibration reveals the initial static choice was wrong.**

This is what makes JIT-based systems (LLVM ORC + PGO, JVM tiered compilation) genuinely adaptive, not "adaptive" in name only.

**Lines: 1,200**

### 4.6 F — Energy/power as first-class cost

On heterogeneous hardware (P-core/E-core, mobile SoC), **pure latency optimization is the wrong objective half the time.**

Add energy as a cost term.

**Lines: 600**

### 4.7 G — Profitability gate

```
if predicted_speedup >= 10:
    emit SPP parallel version
else:
    choose SIMD / serial / other version
```

**Lines: 800**

### 4.8 Components

| # | Component | Lines |
|---|---|---|
| 3.1 | Hardware model | 1,900 |
| 3.2 | Learned cost model | 1,500 |
| 3.3 | Bayesian search space | 2,000 |
| 3.4 | Execution optimizer | 3,900 |
| 3.5 | Online re-optimization | 1,200 |
| 3.6 | Energy cost model | 600 |
| 3.7 | Profitability gate | 800 |

**Total: ~11,900 lines**

---

## 5. Layer 4 — Speculation Tiers

### 5.1 The corrected tiers

| Tier | Mechanism | Cost | When |
|---|---|---|---|
| 0 | Static proof | 0 | Always |
| 1 | Cheap runtime guards | ~10 cycles | Static fails |
| 2 | Version / range checks | ~100 cycles | Guards fail |
| 3 | Chunk speculation | ~1,000 cycles | Checks fail |
| 4 | Page / version tracking | ~10,000 cycles | Chunks fail |
| 5 | Software TM | ~100,000 cycles | Pages fail |
| 6 | **Optimized speculation** | **~150–550 cycles** | Universal |
| **Hardware fast path** | **TM (TSX/TME) if available** | **~1,000 cycles** | **Opportunistic** |

**The compiler always selects the cheapest tier capable of validating the assumption.**

### 5.2 Tier 6 — Optimized Software Speculation

**Hardware TM is disabled on most CPUs. It is not deployable in 2026.**

Tier 6 replaces it. It is the most aggressive software speculation tier, optimized to approach hardware TM performance.

| Mechanism | What it does |
|---|---|
| **Shadow pages** | Per-speculative-unit shadow page |
| **Page-table swapping** | Commit via page pointer swap |
| **Register save/restore** | Rollback via registers |
| **Version counters** | Conflict detection via counters |
| **MMU traps** | Invalid access detection |
| **Async-signal-safe handler** | Fast rollback |
| **Recovery trampoline** | Non-destructive unwind |

### 5.3 The three-layer rollback

**Layer 1 — Register save/restore**

```
At speculation start: save all registers to TLS
At rollback: restore all registers from TLS
Cost: ~20–50 cycles
```

**Layer 2 — Shadow pages**

```
At speculation start: allocate shadow page for writes
                     write to shadow, not real memory
At commit: swap shadow page → real memory
At rollback: discard shadow page
Cost: ~100–500 cycles
```

**Layer 3 — Version counters (fine-grained conflicts)**

```
At each write: increment version counter for the location
At commit: check read-set versions
At rollback: discard write-set
Cost: ~10–50 cycles per operation
```

**Composite rollback cost: 150–550 cycles. Near-zero in practice.**

### 5.4 Correctness gaps — real answers

**Allocator-aware rollback:** Speculative regions need a **thread-local bump allocator with a checkpoint/reset**. Heap state is not in registers.

**Nested speculation:** Explicit stack of checkpoints, not a single global one.

**I/O and non-idempotent side effects:** Buffering + deferred commit. Log the syscall, only flush on commit. This is a whole subsystem.

### 5.5 Hardware TM as fast path

Hardware TM is not dropped. It is a **fast path in front of Tier 6**, used opportunistically when available (Intel TSX successors, ARM TME) with Tier 6 as the universal fallback.

### 5.6 Components

| # | Component | Lines |
|---|---|---|
| 4.1 | Tier selector | 500 |
| 4.2 | Runtime guards | 600 |
| 4.3 | Version / range checks | 700 |
| 4.4 | Chunk speculation | 900 |
| 4.5 | Page versioning | 700 |
| 4.6 | Software TM | 1,000 |
| 4.7 | Tier 6 optimized speculation | 1,500 |
| 4.8 | Allocator-aware rollback | 800 |
| 4.9 | Nested checkpoint stack | 600 |
| 4.10 | I/O buffering + deferred commit | 900 |
| 4.11 | Hardware TM fast path | 700 |
| 4.12 | Commit / rollback engine | 1,000 |
| 4.13 | Conflict detection | 700 |
| 4.14 | Rollback trampoline | 400 |
| 4.15 | Speculation cost model | 500 |

**Total: ~11,500 lines**

---

## 6. Layer 5 — Acceleration

### 6.1 Portable vector IR

**Auto-vectorization should target a portable vector IR** (LLVM VPlan or MLIR vector dialect) rather than hand-targeting AVX2/AVX-512/NEON separately.

This lets one optimizer serve all ISAs instead of tripling maintenance cost.

### 6.2 GPU offload cost model

**PCIe/interconnect transfer cost vs. compute-on-device tradeoff is often the dominant term.**

This must be modeled, not a partition flag.

### 6.3 Components

| # | Component | Lines |
|---|---|---|
| 5.1 | Portable vector IR | 1,200 |
| 5.2 | Software prefetch | 500 |
| 5.3 | Cache-blocked tiling | 700 |
| 5.4 | GPU transfer cost model | 800 |
| 5.5 | CPU / GPU partitioning | 600 |

**Total: ~3,800 lines**

---

## 7. The full pipeline

```
Sea of Nodes IR
    │
    ▼
┌─────────────────────────────────────────┐
│ 1. Dependence Engine                    │
│    UDDP + alias + reductions + effects  │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 2. Parallel Version Generation          │
│    Version lattice + Pareto selector    │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 3. SPP-HX                               │
│    A. Hardware model                    │
│    B. Learned cost model                │
│    C. Bayesian search                   │
│    D. Execution optimizer               │
│    E. Online re-optimization            │
│    F. Energy cost                       │
│    G. Profitability gate (10x)          │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 4. Speculation Tiers                    │
│    Tier 0–6, hardware fast path         │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 5. Acceleration                         │
│    Portable vector IR + GPU             │
└────────────────┬────────────────────────┘
                 │
                 ▼
Machine code + certificates
```

---

## 8. The complete line count

| Layer | Lines |
|---|---|
| 1. Dependence Engine | 15,300 |
| 2. Parallel Version Generation | 2,100 |
| 3. SPP-HX | 11,900 |
| 4. Speculation Tiers | 11,500 |
| 5. Acceleration | 3,800 |
| Integration | 2,900 |
| Tests | 12,000 |
| **Total** | **~59,500** |

**With headers and generated code: ~70,000 lines.**

---

## 9. The build order

| # | What | Time |
|---|---|---|
| 1 | UDDP M₀ (done) | ✅ |
| 2 | UDDP M₁–M₁₀ | 3 weeks |
| 3 | Interprocedural alias analysis | 3 weeks |
| 4 | Non-affine dependence | 1 week |
| 5 | Reduction generalization | 1 week |
| 6 | Cross-iteration alias speculation | 1 week |
| 7 | Dependence Engine integration | 2 weeks |
| 8 | Parallel Version Generation | 1 week |
| 9 | SPP-HX: Hardware model | 1 week |
| 10 | SPP-HX: Learned cost model | 2 weeks |
| 11 | SPP-HX: Bayesian search | 2 weeks |
| 12 | SPP-HX: Execution optimizer | 3 weeks |
| 13 | SPP-HX: Online re-optimization | 2 weeks |
| 14 | SPP-HX: Profitability gate | 2 days |
| 15 | Speculation tiers | 7 weeks |
| 16 | Acceleration | 3 weeks |
| 17 | Integration | 1 week |
| 18 | Tests | 2 weeks |
| **Total** | — | **~35 weeks** |

**SPP: 35 weeks. ~60,000 lines.**

---


**10x is the admission criterion. Not a universal guarantee.**

---

## 11. The one honest sentence

> **"SPP is five layers: Dependence Engine (15,300 lines with interprocedural alias, non-affine gray zone, reduction generalization, cross-iteration speculation), Parallel Version Generation (2,100, lattice not fixed A/B/C/D, cross-version dedup), SPP-HX (11,900, analytic cost model (roofline + cache hierarchy), Bayesian search, online re-optimization, energy cost), Speculation Tiers (11,500, Tier 6 optimized speculation with three-layer rollback, allocator-aware rollback, nested checkpoints, I/O buffering, hardware TM fast path), Acceleration (3,800, portable vector IR, GPU transfer cost model). Total: ~59,500 lines. 35 weeks. Tier 6 is ~150–550 cycles, not hardware TM. Hardware TM is a fast path, not a requirement. 10x is the admission criterion, not a universal guarantee. SPP gives 10x–30x on affine loops. That is the final SPP."**

---

## What to do

**Build UDDP M₁–M₁₀ first. 3 weeks. Then interprocedural alias analysis. 3 weeks. Then the Dependence Engine. Then SPP-HX. Then speculation. Then acceleration. 35 weeks total.**

**Say the word. I write the M₁ spec.**


# Wolfe Chapter 9 — Loop Restructuring

## Extraction for UDDP + SPP

This is the transformation engine. Chapter 9 gives the exact algorithms and legality conditions for every loop restructuring transformation. Here is the extraction, mapped to SPP Layer 2.

---

## 9.1 Simple Transformations

### 9.1.1 Statement Reordering

**What it does:** Reorders statements (or loops) at any granularity.

**Legality condition:** Any topological sort of the dependence graph (including both data and control dependences) is a legal ordering.

**Dependence modification:** None — the dependence graph is preserved; only the linear order changes.

**SPP relevance:**
- Used for instruction scheduling (operation granularity)
- Used for locality (moving statements that use the same data closer)
- Used to prepare for loop fusion (moving loops adjacent)

**UDDP requirement:** A dependence graph with both data and control edges. The certificate is the topological sort order, verified against the dependence graph.

---

### 9.1.2 Unswitching

**What it does:** Removes a loop-invariant conditional from a loop. Replaces:

```
loop
  statements
  if test then then-part else else-part
  more statements
endloop
```

with:

```
if test then
  loop
    statements
    then-part
    more statements
  endloop
else
  loop
    statements
    else-part
    more statements
  endloop
endif
```

**Legality condition:** The conditional test must be **completely independent of the loop**.

**Dependence modification:** None — the dependence relations are preserved.

**SPP relevance:** Reduces branch frequency; simplifies loop body. But makes loop structure more complex, which can affect interchange.

**UDDP requirement:** Prove the conditional test is loop-invariant. Certificate: the test expression is a function of only loop-invariant variables.

---

### 9.1.3 Loop Peeling

**What it does:** Removes the first (or last) iteration(s) of the loop into separate code.

**Transformation:**
```
compute tc
for i = 0 to tc-1 do
  body
endfor
```
becomes:
```
compute tc
if tc > 0 then
  i = 0
  body
  for i = 1 to tc-1 do
    body
  endfor
endif
```

**Legality condition:** Always legal; may require a zero-trip test if the trip count may be zero.

**Dependence modification:** None — the iterations are the same, just separated.

**SPP relevance:**
- Adjusts trip count to enable loop fusion
- Removes conditionals that test the index variable
- Handles wrap-around variables
- Executes loop-invariant code only on the first iteration

**UDDP requirement:** Know the trip count. Certificate: the trip count expression, and the zero-trip guard.

---

### 9.1.4 Index Set Splitting

**What it does:** Divides the index set of a loop into two portions, replicating the body.

**Transformation:**
```
compute tc
for i = 0 to tc-1 do
  body
endfor
```
becomes:
```
compute tc
for i = 0 to s-1 do
  body
endfor
for i = s to tc-1 do
  body
endfor
```

**Legality condition:** Always legal; the union of the split index sets must equal the original.

**Dependence modification:** None — the iterations are the same, just partitioned.

**SPP relevance:**
- Removes conditionals that test the index variable (split at the condition boundary)
- Adjusts trip count for fusion

**UDDP requirement:** Know the split point s. Certificate: the two index sets are disjoint and their union is the original.

---

### 9.1.5 Scalar Expansion

**What it does:** Promotes a scalar to an array to break anti-dependences and output dependences.

**Transformation:**
```
for I = 1 to N do
  T = A(I) + B(I)
  C(I) = T + 1/T
endfor
```
becomes:
```
if N >= 1 then
  allocate Tx(1:N)
  for I = 1 to N do
    Tx(I) = A(I) + B(I)
    C(I) = Tx(I) + 1/Tx(I)
  endfor
  T = Tx(N)
endif
```

**Legality condition:**
- The scalar has **no upward-exposed uses** in the loop (all flow dependences are loop-independent).
- If the scalar is assigned conditionally, last-value assignment may not be possible — may not be feasible unless the scalar is dead after the loop.

**Dependence modification:** Breaks loop-carried anti-dependences and output dependences. Flow dependences become loop-independent.

**SPP relevance:**
- Enables loop fission and distribution
- Enables vectorization
- Removes false dependences that prevent parallelism

**UDDP requirement:** Prove no upward-exposed uses. Certificate: the scalar's def-use chain shows all uses are dominated by a definition in the same iteration.

---

## 9.2 Loop Fusion

**What it does:** Combines two adjacent countable loops with compatible limits into one.

**Transformation:**
```
for I = ... do body1 endfor
for I = ... do body2 endfor
```
becomes:
```
for I = ... do body1 body2 endfor
```

**Legality condition:** All dependence relations must be preserved. Before fusion, all dependences must flow from body1 to body2 (unless carried by an outer loop). After fusion, the loop can carry dependences from body2 to body1 — these would be spurious and illegal.

**Test:** A dependence from iteration i of body1 to any iteration j<i of body2 would be violated by fusion.

**Dependence modification:** Cross-loop dependences become loop-carried dependences. The direction vector for the fused loop includes the cross-loop distance.

**SPP relevance:**
- Reduces loop overhead
- Improves temporal locality (same data reused)
- Enables more scalar optimizations (CSE, instruction scheduling)

**UDDP requirement:** Cross-loop dependence distance. Certificate: the cross-loop distance is positive (or zero for loop-independent).

### 9.2.1 Array Assignments

Fortran 90 array assignments are defined as: compute RHS for all indices, then assign LHS. Simple-minded compilation generates computation loop + copy loop. Fusion eliminates the temporary and copy loop.

**Legality:** Fusion of computation and copy loop is always legal if the LHS array is not used on the RHS.

### 9.2.2 Complications

- **Different trip counts:** Peel or split the longer loop, or use conditionals.
- **Scalars assigned in both loops:** Must ensure correct last value.
- **Upward-exposed use of a scalar in one loop assigned in the other:** Fusion is illegal.

### 9.2.3 Fusing Parallel Loops

- Fusing two vector loops: always legal.
- Fusing two parallel loops: legal if all flow dependences are loop-independent.
- Fusing two doall loops: legal if all resulting dependences are loop-independent.
- Fusing two sequential loops: legal if no dependences are violated.

**Canonical dependence analysis does not compute cross-loop distance.** Wolfe shows how to compute it as the difference between iteration vectors of the two loops.

### 9.2.4 Loop Alignment

**What it does:** Chooses which iteration of the second loop fuses with which iteration of the first.

**Alignment parameter m:** Iteration i of the first loop fuses with iteration i+m of the second.

**Legality:** m is chosen to eliminate negative distance dependences.

**Implementation:** Peeling (peel m iterations) or conditionals (add m iterations, use conditionals to skip).

### 9.2.5 Fusion with Scalars

**Legality rules:**
- Scalar assigned in neither loop or assigned/used in only one loop: does not prevent fusion.
- Scalar with upward-exposed use in either loop that is assigned in the other: **fusion is illegal**.
- Scalar assigned in both loops with no upward-exposed use: fusion is legal if correct last value is assigned.

---

## 9.3 Loop Fission

**What it does:** Breaks a single loop into two or more smaller loops. Inverse of fusion.

**Transformation:**
```
for I = 1 to N do
  A(I) = A(I) + B(I-1)
  B(I) = C(I-1)*X + C
  C(I) = 1/B(I)
  D(I) = sqrt(C(I))
endfor
```
becomes:
```
for ib = 0 to N-1 do
  B(ib+1) = C(ib)*X + C
  C(ib+1) = 1/B(ib+1)
endfor
for ib = 0 to N-1 do
  A(ib+1) = A(ib+1) + B(ib)
endfor
for ib = 0 to N-1 do
  D(ib+1) = sqrt(C(ib+1))
endfor
```

**Legality condition:**
- Construct statement-level dependence graph.
- Find strongly connected components (SCCs).
- Nodes in the same SCC must stay in the same loop.
- If all nodes are in one SCC, fission cannot divide the loop.
- Ignore loop-carried anti-dependences and output dependences for scalars that will be expanded.
- Legal order: acyclic condensation of the dependence graph; any topological order.

**Dependence modification:** Dependences between loops become inter-loop dependences. The SCC structure is preserved.

**SPP relevance:**
- Breaks up large loops for instruction cache
- Improves memory locality (fewer arrays per loop)
- Enables other transformations (interchanging)
- Removes dependence cycles to enable parallelization

**UDDP requirement:** SCC computation. Certificate: the condensation graph and its topological order.

### 9.3.1 Fission with Conditionals

- Treat conditional block as a unit, or
- Use a temporary array to hold the conditional test result, guarding execution after fission.

### 9.3.2 Nested Loops

- Proceed from inner loop outward.
- After processing inner loop, treat each inner loop as an atomic unit for outer loop fission.
- Ignore dependences carried by outer loops when fissioning inner loop.

### 9.3.3 Fission Algorithm

1. Build dependence graph, treating inner loops as atomic units.
2. Ignore dependences carried by outer loops.
3. Ignore loop-carried output and anti-dependences for scalars that can be expanded.
4. Merge nodes for statements that must stay together.
5. Find SCCs.
6. Each SCC becomes a separate loop.
7. Generate code in topological order of the condensation graph.

### 9.3.4 Fission for Parallel Loops

- **Fission for forall loops: always legal** (forall preserves only lexically forward dependences).
- **Fission for doall loops: always legal** (doall carries no dependences).
- **Fission for dopar loops: legal if SCCs respect the parallelism** (dopar resolves all loop-carried conflicts as anti-dependences).

**Use for conversion to array assignments:**
1. If loop has no lexically backward dependences, convert to forall.
2. If there are lexically backward dependences, reorder statements to make them forward.
3. If there are dependence cycles, fission to remove the cycle into a separate loop.

---

## 9.4 Loop Reversal

**What it does:** Runs a loop backward.

**Legality condition:**
- Reversal of forall, dopar, or dosingle loops: always legal (execution order not defined).
- Reversal of sequential loop: legal only if the loop carries **no dependence relations**.

**Dependence modification:** Negates the dependence distance for that loop in any distance vector.

**SPP relevance:**
- Enables loop fusion where it would otherwise fail (negates cross-loop distance)
- Enables interchanging (negates (<,>) to (<,<))

**UDDP requirement:** Prove no dependence carried by the loop. Certificate: the loop has no loop-carried dependences (all dependences are loop-independent or carried by outer loops).

---

## 9.5 Loop Interchanging

**What it does:** Switches inner and outer loop of a tightly nested pair.

**Legality condition (tightly nested):**
- Loop-independent dependences: cannot prevent interchanging.
- Dependence carried by inner loop before interchange (zero distance in outer): still carried by same loop after interchange — legal.
- Dependence carried by outer loop with zero distance in inner: still carried by same loop — legal.
- **Interchange-sensitive dependence:** Nonzero distance in both loops. Must be carried by the new outer loop in the same way. Only direction vector (<,>) is violated by interchange.

**For two sequential loops:** Interchange is legal if no dependence has direction (<,>).

**Dependence modification:** The distance/direction vector entries for the interchanged loops are switched.

**SPP relevance:**
- **Single most important transformation.** Enables automatic discovery of parallelism.
- Moves dependence-carrying loop to outermost position for parallelization.
- Improves memory locality (changes stride of inner loop).
- Reduces loop startup overhead.

**UDDP requirement:** Dependence direction vectors. Certificate: the direction vector test for interchange legality.

### 9.5.2 Interchanging with Parallel Loops

Same basic rules. But parallel/sequential interchange is more complex:
- A dopar loop resolves all loop-carried conflicts as anti-dependences.
- Sequential loops resolve conflicts in execution order.
- An anti-dependence with positive distance is resolved the same way by both loop types.

### 9.5.3 Multiple Interchanges

- **Outermosting:** Moving a loop to outermost position.
- **Innermosting:** Moving a loop to innermost position.
- Direction vector tests for outermosting/innermosting.

### 9.5.4 Loop Limits

**When inner loop limits are invariant in outer loop:** Interchange without changing limits.

**When inner loop limits vary in outer loop (triangular/trapezoidal):**
- Interchange is equivalent to transposing the iteration space.
- Use Fourier-Motzkin projection to find new limits.
- Algorithm FMint: eliminate variables in order in,in-1,,i2.
- Construct new limits from inequalities involving i1,,ik but not ik+1,,in.

**This is SPP Layer 2's core algorithm for non-rectangular loops.**

### 9.5.5 Nontightly Nested Loops

- Sink nontightly nested code into inner loop by adding conditionals.
- Before-statement: merge with first iteration or prepend new iteration.
- After-statement: merge with last iteration or append new iteration.
- Dependence vectors must add an entry for the sunk code.

### 9.5.6 Interactions with Other Loop Transformations

- **Fission** to make nontightly nested loop tightly nested.
- **Reversal** to enable interchanging (negate (<,>) to (<,<)).
- **Interchanging** to enable fusion.
- **Unswitching** disables interchanging (and vice versa).

---

## 9.6 Loop Skewing

**What it does:** Changes iteration vectors by adding outer loop index to inner loop index.

**Transformation:** Iteration (i,j) becomes (i,j+fi) for skew factor f.

**Dependence modification:** Distance (d1,d2) becomes (d1,d2+fd1).

**Legality condition:** Always legal (unimodular transformation). But the resulting dependence distances must allow the desired subsequent transformation.

**SPP relevance:**
- Enables interchanging when normalization would prevent it
- Enables wavefront execution
- Enables tiling of triangular iteration spaces

**UDDP requirement:** Compute new dependence distances after skewing. Certificate: the transformed distance vectors.

---

## 9.7 Linear Loop Transformations

**What it does:** Unifies interchanging, reversal, and skewing as linear transformations of the iteration space.

**Framework:**
- Transformation matrix T.
- Interchanging: $\mathbf{T}_{\text{int}} = \begin{pmatrix} 0 & 1 \\ 1 & 0 \end{pmatrix}$.
- Reversal: Trev negates the index.
- Skewing: Tskew adds fi to j.
- Composition: matrix multiplication.

**Legality condition:** For sequential loops, 0Td for all dependence distance vectors d.

**Loop limits:** Aic becomes AT-1jc. Apply Fourier-Motzkin projection.

**Unimodular transformations:** (T)=1. Preserve integer points. Volume preserved.

**Nonunimodular transformations:** Allow scaling. Use Hermite Normal Form decomposition T=HU.

**This is the general framework for SPP Layer 2.**

### 9.7.1 Nonunimodular Transformations

- T=HU with U unimodular and H in HNF.
- Transform limits by U first, then H.
- Loop strides are hmm (diagonal of HNF).
- Bounds computation involves floor operations.

**UDDP requirement:** HNF decomposition, Fourier-Motzkin projection, bounds computation for nonunimodular transformations. Certificate: the transformation matrix, HNF decomposition, and transformed bounds.

---

## 9.8 Strip-Mining

**What it does:** Decomposes a single loop into two nested loops: strip loop and element loop.

**Transformation:**
```
for I = 1 to N do body endfor
```
becomes:
```
for Is = 1 to N by s do
  for I = Is to min(N, Is+s-1) do
    body
  endfor
endfor
```

**Legality condition:** Always legal.

**Dependence modification:**
- Distance d becomes (ddivs,dmods).
- If dmods0, another dependence with distance (ddivs+1,-((s-d)mods)) is generated.

**Direction vector modification:**
| Old Direction | New Direction |
|---|---|
| (<) | {(<,<)} |
| (=) | (=,<) |
| (>) | {(>,<)} |

**SPP relevance:**
- Bridge to vectorization (Chapter 12)
- Enables cache blocking
- Enables parallel scheduling
- Enables SIMD

**UDDP requirement:** Strip size s, dependence distances. Certificate: the two new distance vectors.

---

## 9.9 Loop Tiling

**What it does:** Like strip-mining but for nested loops. Tile boundaries are parallel to iteration space axes, not to iteration space boundaries.

**Key difference from strip-mining:** Tile start is independent of first iteration.

**Transformation:** Characterized by tile size ts and tile offset to. Each tile starts at i where $i \bmod ts = to$.

**General formula:**
```
for I = lo to hi do
```
becomes:
```
for It = ... by ts do
  for I = max(lo, It) to min(hi, It+ts-1) do
    body
  endfor
endfor
```

**Legality condition:** Same as strip-mining for the tile loop; interchanging tile and element loops uses the same rules as loop interchanging.

**Dependence modification:** Same as strip-mining.

**SPP relevance:**
- **Core transformation for locality and parallelism.**
- Creates blocks that fit in cache.
- Creates parallel units.
- Creates SIMD units.
- Creates GPU blocks.
- Creates NUMA units.

**UDDP requirement:** Tile size, tile offset, dependence distances. Certificate: the transformed dependence vectors and the interchange legality.

---

## 9.10 Other Loop Transformations

### 9.10.1 Circular Loop Skewing

**What it does:** Skews inner loop iterations to wrap around a cylinder. Iteration space shape preserved.

**Legality condition:** Inner loop must carry no dependences.

**Dependence modification:** Piecewise linear indexing; no change to dependence vectors.

### 9.10.2 Striping

**What it does:** Strip-mining followed by interchange of strip and element loops.

**Transformation:**
```
for I = lo to hi do body endfor
```
becomes:
```
for Is = 0 to s-1 do
  for I = lo+Is to hi by s do
    body
  endfor
endfor
```

**Legality condition:** May not be legal (if negative distance vectors result).

### 9.10.3 Loop Collapsing

**What it does:** Collapses two nested loops into a single loop when array bounds match loop limits and memory stride is constant.

**Legality condition:** Array layout known, strides match.

**SPP relevance:** Fewer loops with longer limits for some machines.

### 9.10.4 Loop Coalescing

**What it does:** Collapses nested loops by computing original index values.

**Transformation:**
```
for I = lo_I to hi_I do
  for J = lo_J to hi_J do
    body
  endfor
endfor
```
becomes:
```
tc_I = hi_I - lo_I + 1
tc_J = hi_J - lo_J + 1
for IJ = 0 to tc_I*tc_J - 1 do
  I = IJ / tc_J + lo_I
  J = MOD(IJ, tc_J) + lo_J
  body
endfor
```

**Legality condition:** Always legal if loop limits are invariant.

**SPP relevance:** Reduces loop nest level; enables parallelization of flattened loop.

---

## 9.11 Interprocedural Transformations

### 9.11.1 Procedure Inlining

**What it does:** Copies procedure body in place of call.

**Legality condition:** Always legal (semantics preserved).

**SPP relevance:** Enables loop transformations across procedure boundaries.

### 9.11.2 Loop Embedding and Extraction

**Loop embedding:** Takes loop surrounding a call and embeds it into the procedure.

**Loop extraction:** Takes loop out of procedure and puts it at call site.

**SPP relevance:** Enables transformations involving loops from call site and procedure.

---

## 9.12 In the Pit

- SISAL has no parallel/sequential interpretation — evaluation order determined by dependences.
- Reduction direction can be defined and used for accumulation operations.
- Loops with only reduction direction dependences can be reversed or interchanged.
- Temporary variables convert control dependence to data dependence.
- Loop reversal with step 1 requires trip count computation:
  ```
  for I = 1 to 10 by 2 do
  ```
  becomes:
  ```
  for I = 1 + ((10-1)/2)*2 downto 1 by -2 do
  ```

---

## 9.13 Further Reading

Key references:
- Allen & Cocke (1972): statement reordering
- Loveman (1977): loop transformations
- Cohagan (1973): automatic loop interchanging (TI ASC)
- Banerjee (1989): general discussion
- Ancourt & Irigoin (1991): Fourier-Motzkin for loop limits after restructuring
- Polychronopoulos (1987): loop coalescing
- Irigoin & Triolet (1988): tiling
- Wolf & Lam (1991): unimodular transformations
- Li & Pingali (1994): nonunimodular transformations
- Cooper et al. (1990, 1991): interprocedural transformations

---

## Mapping to SPP Layer 2

| Wolfe Section | SPP Component | UDDP Requirement |
|---|---|---|
| 9.1.1 Statement Reordering | Instruction scheduling, locality | Dependence graph, topological sort |
| 9.1.2 Unswitching | Branch elimination | Loop-invariance proof |
| 9.1.3 Loop Peeling | Trip count adjustment | Trip count, zero-trip guard |
| 9.1.4 Index Set Splitting | Conditional removal | Split point, set partition proof |
| 9.1.5 Scalar Expansion | Dependence breaking | No upward-exposed uses |
| 9.2 Loop Fusion | Locality, overhead reduction | Cross-loop dependence distance |
| 9.3 Loop Fission | Parallelism, locality, cache | SCC computation |
| 9.4 Loop Reversal | Enable fusion/interchange | No loop-carried dependences |
| 9.5 Loop Interchanging | Parallelism, locality | Direction vectors, Fourier-Motzkin |
| 9.6 Loop Skewing | Enable interchange/wavefront | Transformed distances |
| 9.7 Linear Loop Transformations | General framework | HNF, Fourier-Motzkin |
| 9.8 Strip-Mining | SIMD, cache, parallel | Strip size, distance vectors |
| 9.9 Loop Tiling | Locality, parallelism, GPU | Tile size, offset, distances |
| 9.10.1 Circular Skewing | Specialized scheduling | No inner-loop dependences |
| 9.10.2 Striping | Parallel scheduling | Distance vectors |
| 9.10.3 Loop Collapsing | Long loops | Array layout, strides |
| 9.10.4 Loop Coalescing | Flattened parallelism | Loop limits |
| 9.11 Interprocedural | Whole-program optimization | Call graph, summaries |

---

## What Chapter 9 Gives SPP Layer 2

**The complete transformation catalog with:**
1. **Exact algorithms** for each transformation.
2. **Exact legality conditions** in terms of dependence relations.
3. **Exact dependence modifications** — how each transformation changes distance/direction vectors.
4. **The unified framework** — linear transformations with matrix formulation.
5. **Nonunimodular extensions** — HNF decomposition, bounds computation.
6. **Interprocedural transformations** — inlining, embedding, extraction.

**This is SPP Layer 2's specification.** With Chapter 9, you have:
- The transformation algorithms
- The legality conditions
- The dependence modifications
- The code generation for transformed loops

**What UDDP must provide to SPP Layer 2:**
- Dependence distance/direction vectors (from M0–M10)
- Trip counts (from M3, M5)
- Cross-loop dependence distances (new for fusion)
- HNF decomposition (from Ch. 4, now applied)
- Fourier-Motzkin projection (from Ch. 4, now applied)

**What SPP Layer 2 must produce:**
- Transformed loop code
- Certificates of legality (the dependence preservation proof)
- Cost model input (locality, parallelism, vectorization potential)

---

## The One-Sentence Summary

> **Wolfe Chapter 9 gives SPP Layer 2 its complete transformation catalog: fusion, fission, interchange, skewing, strip-mining, tiling, and the unified linear transformation framework — with exact legality conditions, dependence modifications, and code generation algorithms. UDDP provides the dependence analysis; SPP Layer 2 applies the transformations; the certificates prove legality.**

–


# Chapter 4 — Complete Usage Map for UDDP + SPP

## Every Section, Every Formula, Every Algorithm — Where It Is Used

---

## 4.1 Real Vectors and Matrices

### 4.1.1 Vector Ordering

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Sign function** | $\operatorname{sign}(\alpha) \in \{-1,0,1\}$ | **UDDP M0–M10:** Computing direction vectors from distance vectors. The sign of each distance component gives the direction vector. **Checker:** Verifying direction claims. |
| **Vector relation** a<b | ai<bi for all i | **UDDP:** Comparing iteration vectors. **SPP:** Comparing distance vectors for legality. |
| **Lexicographic relation** a<jb | ai=bi for i<j, aj<bj | **UDDP M0–M10:** Dependence level computation. The leftmost non-equal component determines the loop that carries the dependence. **SPP Layer 2:** Legality of loop interchange — depends on lexicographic ordering. |
| **Lexicographically positive** 0<a | a is lexicographically greater than zero | **UDDP:** Theorem 4.5 (Banerjee) — distance vector must be lexicographically nonnegative. **SPP:** Interchange legality test. |
| **Ordering vector** w | Vector of ordering relations | **UDDP:** Direction vector representation. **SPP:** Direction vector tests for interchange, skewing, fusion. |
| **Simple ordering vector** | Only <, =, > entries | **UDDP:** Standard direction vector format. **SPP:** Direction vector tests. |
| **Sign vector** $\operatorname{sign}(\mathbf{a}-\mathbf{b})$ | $(\operatorname{sign}(a_1-b_1), \ldots, \operatorname{sign}(a_n-b_n))$ | **UDDP M0–M10:** Computing direction vectors. **SPP:** Direction vector dispatch. |

### 4.1.2 Matrices

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Transpose** AT | bij=aji | **UDDP:** Dependence equation formulation. **SPP:** Loop interchange (transpose iteration space). |
| **Dot product** xy | ixiyi | **UDDP:** Evaluating linear functions over iteration vectors. |
| **Matrix multiplication** AB | cij=A[i]b[j] | **UDDP:** Applying transformations to dependence vectors. **SPP Layer 2:** Composing loop transformations (interchange, skewing, reversal). |
| **Linear function** f(x)=Fx | Matrix-vector product | **UDDP:** Affine subscript representation. **SPP:** Iteration space transformation. |
| **Identity matrix** I | Ones on diagonal | **UDDP:** Normalized loop representation. **SPP:** Transformation initialization. |
| **Range** $\operatorname{range}\mathbf{A}$ | $\{\mathbf{Ax} \mid \mathbf{x} \in \mathbb{R}^m\}$ | **UDDP:** Existence of real solutions to dependence equations. |
| **Null space** $\operatorname{kernel}\mathbf{A}$ | $\{\mathbf{x} \mid \mathbf{Ax} = \mathbf{0}\}$ | **UDDP:** Characterizing all solutions to dependence equations. **SPP:** Null space basis gives independent variables. |
| **Rank** $\operatorname{rank}(\mathbf{A})$ | Dimension of range | **UDDP:** Number of independent dependence equations. **SPP:** Number of independent loop transformations. |
| **Nullity** $\operatorname{nullity}(\mathbf{A})$ | Dimension of kernel | **UDDP:** Number of independent variables in dependence equation. |
| **Rank-nullity theorem** | $\operatorname{rank}(\mathbf{A}) + \operatorname{nullity}(\mathbf{A}) = n$ | **UDDP:** Characterizing solution space. **SPP:** Number of free parameters in transformed loop. |

### 4.1.3 Elementary Row Operations

| Operation | Transformation Matrix | UDDP/SPP Usage |
|---|---|---|
| **Scale** | tii= | **UDDP:** Normalizing dependence equations. **SPP:** Non-unimodular transformations (loop scaling). |
| **Skew** | tij= | **UDDP:** Eliminating variables from dependence equations. **SPP:** Loop skewing (adding outer loop index to inner). |
| **Transpose** | tii=0,tij=1,tji=1,tjj=0 | **UDDP:** Reordering variables. **SPP:** Loop interchange. |
| **Permutation matrix** | Product of transpositions | **UDDP:** Variable ordering. **SPP:** Loop permutation (outermosting, innermosting). |

### 4.1.4 Polyhedra

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Convex set** | $\alpha\mathbf{x} + (1-\alpha)\mathbf{y} \in C$ | **UDDP:** Iteration space is convex. **SPP:** Tiling requires convex tiles. |
| **Convex hull** | Smallest convex set containing all vectors | **UDDP:** Iteration space boundary. **SPP:** Tiling of non-rectangular spaces. |
| **Affine half-space** | ax | **UDDP:** Loop bound constraints. **SPP:** Tile boundaries. |
| **Polyhedron** | Axb | **UDDP:** Iteration space representation. **SPP:** Tiling legality. |
| **Polytope** | Bounded polyhedron | **UDDP:** Bounded iteration space. **SPP:** Tile size selection. |
| **Vertex** | Extreme point of polytope | **UDDP:** Extreme values of affine functions (bounds test). **SPP:** Tiling corner cases. |
| **Unit cube** | 0xi1 | **UDDP:** Normalized iteration space. **SPP:** Tile shape. |

### 4.1.5 Determinants

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Cofactor expansion** | (A)=j(-1)i+jaij(A[ij]) | **UDDP:** Unimodularity test. **SPP:** Transformation volume preservation. |
| **22 determinant** | ad-bc | **UDDP:** Two-variable dependence equation. **SPP:** 2D loop interchange. |
| **Properties** | (AB)=(A)(B) | **UDDP:** Composition of transformations. **SPP Layer 2:** Composing loop transformations. |
| **Volume change** | $\|\det(\mathbf{A})\|$ = volume change | **UDDP:** Unimodular transformations preserve volume. **SPP:** Non-unimodular transformations change iteration space density. |

### 4.1.6 Matrix Inverse — Algorithm Invert (Figure 4.2)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm Invert** | Compute A-1 or rank | **UDDP:** Solving dependence equations via matrix inverse. **SPP Layer 2:** Inverting transformation matrices to compute new loop limits. |

**Detailed usage:**
- **UDDP M0–M5:** Solving linear dependence equations when the coefficient matrix is square and nonsingular.
- **SPP Layer 2:** Computing new loop limits after interchange/tiling: if j=Ti, then i=T-1j.
- **UDDP M4:** Computing the inverse of the transformation matrix for non-unimodular transformations.
- **SPP-HX:** Inverting cost model matrices.

---

## 4.2 Integer Matrices and Lattices

### 4.2.1 Euclid's Algorithm

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Euclid** | (a,b) | **UDDP M0–M2:** Gcd test for dependence equation. |
| **Euclid2** | (a,b) and x,y such that ax+by=(a,b) | **UDDP M0–M2:** Finding a particular solution to the dependence equation. **UDDP M4:** Extended gcd for multiple variables. |
| **Algorithm findgcd** (Figure 4.3) | g=(a,b), x,y such that ax+by=g | **UDDP M0–M10:** Core algorithm for solving dependence equations. **UDDP M9:** Modular inverse computation (CRT). **SPP Layer 4:** Version counter increments. |

**Detailed usage:**
- **UDDP M0–M2:** The gcd test: if $\gcd(a,b) \nmid c$, no dependence.
- **UDDP M0–M2:** Finding a particular solution to ax-by=c.
- **UDDP M4:** Generalized gcd test for systems.
- **UDDP M9:** Computing modular inverses for $(i \cdot c) \bmod p$.
- **UDDP M6:** Finding factors of products.
- **SPP Layer 4:** Version counters need modular arithmetic.
- **Checker:** Verifying gcd claims.

### 4.2.2 Integer Matrices

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Integer matrix** | All entries integer | **UDDP:** Dependence equation coefficient matrix. **SPP:** Transformation matrix. |
| **Unimodular matrix** | (A)=1 | **UDDP:** Preserves integer lattice. **SPP Layer 2:** Loop interchange, reversal, skewing are unimodular. |
| **Unimodular row operations** | Negate, skew, interchange | **UDDP:** Reducing dependence equations. **SPP:** Composing loop transformations. |
| **Product of unimodular** | Unimodular | **UDDP:** Composition of integer-preserving transformations. **SPP:** Composing loop transformations preserves integer points. |

### 4.2.3 Hermite Normal Form — Algorithm Hermite (Figure 4.4)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm Hermite** | Compute HNF H and unimodular U such that AU=H | **UDDP M4:** Solving integer dependence equations. **SPP Layer 2:** Non-unimodular loop transformations. |
| **Hermite Trapezoidal Form (HTF)** | HNF without full reduction | **UDDP M4:** Faster integer equation solving when full HNF not needed. |
| **Procedure Reduce** | Reduce a row to a single nonzero | **UDDP M4:** Core operation for HNF. **UDDP M6–M10:** Reducing polynomial/modular equations. |

**Detailed usage:**
- **UDDP M4:** Solving systems of integer equations via HNF.
- **UDDP M6:** Reducing quadratic Diophantine equations.
- **UDDP M9:** Solving modular equations via HNF over Zp.
- **SPP Layer 2:** Computing loop limits after non-unimodular transformations (Li & Pingali method).
- **SPP Layer 2:** Tiling with non-unit strides.

### 4.2.4 Lattices

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Lattice point** | Integer vector | **UDDP:** Iteration point. |
| **Lattice** L(A) | Integer combinations of columns | **UDDP:** Set of all possible dependence distances. |
| **Dense lattice** | Contains all integer points between any two points | **UDDP:** Normalized iteration space. **SPP:** Unimodular transformations preserve density. |
| **Sparse lattice** | Not dense | **UDDP:** Non-unit strides. **SPP:** Non-unimodular transformations create sparse lattices. |
| **Basis set** | Linearly independent generators | **UDDP:** Characterizing dependence distances. **SPP:** Characterizing transformed iteration space. |
| **HNF columns as basis** | Nonzero columns of HNF | **UDDP M4:** Lattice basis for dependence distances. **SPP Layer 2:** Lattice basis for transformed iteration space. |

---

## 4.3 Linear System of Equations

### 4.3.1 Variable Elimination

| Method | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Variable elimination** | Solve linear system by eliminating one variable at a time | **UDDP M0–M5:** Solving dependence equations. **SPP:** Simplifying constraint systems. |
| **Gaussian elimination** | Matrix method for elimination | **UDDP M4:** Solving linear dependence equations. **SPP Layer 2:** Computing loop limits. |
| **Elementary lower triangular matrices** | Annihilate below diagonal | **UDDP M4:** Gaussian elimination steps. **SPP:** Transformation composition. |
| **Algorithm GE** (Figure 4.5) | Reduce to upper trapezoidal | **UDDP M4:** Solving linear dependence equations. **SPP:** Computing loop limits. |
| **Algorithm BackSubstitute** (Figure 4.6) | Back substitution | **UDDP M4:** Completing the solution. **SPP:** Computing loop bounds. |

**Detailed usage:**
- **UDDP M0–M5:** Solving dependence equations when the system is linear.
- **UDDP M4:** The core linear algebra engine.
- **SPP Layer 2:** Computing loop limits after interchange (Fourier-Motzkin + Gaussian elimination).
- **SPP Layer 2:** Tiling with non-rectangular iteration spaces.

### 4.3.2 Example 4.12 — Null Space Basis

| Concept | Usage |
|---|---|
| **Null space basis** | **UDDP M0–M5:** Characterizing all solutions to dependence equations. **SPP:** Independent variables in transformed loops. |

---

## 4.4 System of Integer Equations

### 4.4.1 Single Equation

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Gcd condition** | $\gcd(a_1,a_2) \mid b$ | **UDDP M0–M2:** Gcd test. |
| **General solution** | x1=x1'-(a2/g)t, x2=x2'+(a1/g)t | **UDDP M0–M2:** Characterizing all dependence distances. |
| **Algorithm GCD** | Find gcd and particular solution | **UDDP M0–M10:** Core algorithm. |

### 4.4.2 Many Unknowns

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Gcd of many coefficients** | $\gcd(a_1, \ldots, a_n) \mid b$ | **UDDP M4:** Generalized gcd test. |
| **Variable substitution** | yk=i(aidivak)xi | **UDDP M4:** Reducing integer equations. |
| **Unimodular matrix approach** | AU=(g,0,,0) | **UDDP M4:** Algebraic solution to integer equations. |
| **Algorithm ReduceRow** (Figure 4.7) | Reduce a single equation | **UDDP M4:** Core reduction algorithm. **UDDP M6–M10:** Reducing nonlinear equations. |

### 4.4.3 Many Equations

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Column echelon form** | rj+1>rj | **UDDP M4:** Canonical form for integer systems. |
| **Algorithm ReduceMatrix** (Figure 4.8) | Reduce matrix to column echelon form | **UDDP M4:** Solving systems of integer equations. **SPP Layer 2:** Computing loop limits for non-unimodular transformations. |
| **Forward substitution** | Solve from first equation | **UDDP M4:** Completing the solution. |

**Detailed usage:**
- **UDDP M4:** The complete integer equation solver.
- **UDDP M6:** Reducing quadratic Diophantine equations (after substitution).
- **UDDP M8:** Reducing recurrence equations.
- **UDDP M9:** Reducing modular equations.
- **SPP Layer 2:** Computing loop limits for non-unimodular transformations.
- **SPP Layer 2:** Tiling with non-unit strides.
- **Checker:** Verifying integer solution certificates.

---

## 4.5 Systems of Linear Inequalities

### 4.5.1 Fourier-Motzkin Projection — Algorithm FM (Figure 4.10)

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Upper bound** | aij>0 | **UDDP M0–M2:** Bounds test. |
| **Lower bound** | aij<0 | **UDDP M0–M2:** Bounds test. |
| **Simple bound** | Single nonzero coefficient | **UDDP M0–M2:** Loop limits. |
| **Redundant inequality** | Same coefficients, looser bound | **UDDP:** Constraint simplification. **SPP:** Tile size optimization. |
| **Unconstrained variable** | No upper or lower bound | **UDDP:** Infinite loop bounds. **SPP:** Dynamic bounds. |
| **Algorithm FM** | Fourier-Motzkin projection | **UDDP M4:** Core real solution engine. **SPP Layer 2:** Computing loop limits after interchange. |
| **Projection** | Eliminate one variable | **UDDP M4:** Reducing dimension. **SPP Layer 2:** Transposing iteration space. |

**Detailed usage:**
- **UDDP M0–M5:** Bounds test for affine dependence.
- **UDDP M4:** Real solution to dependence equations + inequalities.
- **UDDP M7:** Bit-vector range analysis (via integer encoding).
- **SPP Layer 2:** Loop interchange with non-rectangular limits.
- **SPP Layer 2:** Tiling with non-rectangular iteration spaces.
- **SPP Layer 2:** Skewing with transformed bounds.
- **SPP Layer 2:** Strip-mining with symbolic bounds.
- **SPP Layer 3:** Cache blocking with non-rectangular tiles.

### 4.5.2 Solution Procedure

| Step | Usage |
|---|---|
| **Delete unconstrained variables** | **UDDP:** Simplify dependence constraints. **SPP:** Dynamic bounds. |
| **Check simple bounds** | **UDDP M0–M2:** Bounds test. |
| **Delete redundant inequalities** | **UDDP:** Constraint simplification. **SPP:** Tile size optimization. |
| **Check inconsistent inequalities** | **UDDP:** Prove no dependence. **SPP:** Prove no legal transformation. |
| **Apply Fourier-Motzkin** | **UDDP M4:** Core engine. **SPP Layer 2:** Loop limits. |

---

## 4.6 Systems of Integer Linear Inequalities

### 4.6.1 Integer Tightening

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Tightening** | aixic becomes $(a_i/g) x_i \le \lfloor c/g \rfloor$ | **UDDP M0–M10:** Tightening dependence constraints. **SPP:** Tightening tile bounds. |
| **Integer elimination** | Multiply to equalize coefficients | **UDDP M4:** Integer Fourier-Motzkin. |
| **Algorithm FMint** (Figure 4.11) | Integer Fourier-Motzkin projection | **UDDP M4:** Core integer solution engine. **SPP Layer 2:** Integer loop limits. |
| **Inexact flag** | Projection may be inexact | **UDDP:** Track precision. **SPP:** Cost model uncertainty. |

### 4.6.2 Dark Shadows

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Dark shadow condition** | cu-cl--+1 | **UDDP M4:** Guarantee integer solution. **SPP:** Guarantee legal transformation. |
| **Real shadow** | Fourier-Motzkin without dark condition | **UDDP M4:** Necessary condition. |
| **Dark shadow** | Subset of real shadow guaranteeing integer solution | **UDDP M4:** Sufficient condition. |
| **Algorithm IntegerInequality** (Figure 4.12) | Integer solution procedure | **UDDP M4:** Core integer solver. **SPP:** Legal transformation verification. |

**Detailed usage:**
- **UDDP M4:** Exact integer solution to dependence equations + inequalities.
- **UDDP M9:** Modular equation solving.
- **SPP Layer 2:** Exact loop limits for non-unimodular transformations.
- **SPP Layer 4:** Speculation guard verification (dark shadow = guard condition).
- **Checker:** Verifying integer solution certificates.

---

## 4.7 Extreme Values of Affine Functions

### 4.7.1 Positive and Negative Parts

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Positive part** r+ | (r,0) | **UDDP M0–M2:** Bounds test. **SPP:** Cost model. |
| **Negative part** r- | (-r,0) | **UDDP M0–M2:** Bounds test. **SPP:** Cost model. |
| **Bounds for ax** | a+l+a-uaxa+u+a-l | **UDDP M0–M2:** Bounds test for single variable. |
| **Algorithm Extreme** (Figure 4.13) | Compute extreme values of affine function | **UDDP M0–M2:** Bounds test. **SPP:** Tile size optimization. |

### 4.7.2 Nonrectangular Regions

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Lower triangular bound matrix** L | (I-L)xl | **UDDP M0–M5:** Triangular loops. |
| **Upper triangular bound matrix** U | (I-U)xu | **UDDP M0–M5:** Trapezoidal loops. |
| **Bounding functions** bn,an | Recursive elimination | **UDDP M0–M5:** Bounds test for nonrectangular regions. |
| **Algorithm Extreme** | Compute lower and upper bounds | **UDDP M0–M5:** Bounds test. **SPP Layer 2:** Tile size for triangular loops. |

**Detailed usage:**
- **UDDP M0–M2:** Bounds test for affine dependence.
- **UDDP M4:** Extreme values for nonrectangular iteration spaces.
- **UDDP M8:** Extreme values for recurrence subscripts.
- **SPP Layer 2:** Tile size selection for triangular/trapezoidal loops.
- **SPP Layer 3:** Cache blocking for non-rectangular iteration spaces.
- **SPP-HX:** Cost model estimation for non-rectangular loops.

---

## 4.8 In the Pit

| Topic | UDDP/SPP Usage |
|---|---|
| **HNF definition conflict** | **UDDP M4:** Need to choose consistent definition. |
| **Integer division ambiguity** | **UDDP M9:** Modular arithmetic. **SPP Layer 4:** Version counters. |
| **Positive/negative parts** | **UDDP M0–M2:** Bounds test. Note: Banerjee's definition differs from Wolfe's. |

---

## Complete Algorithm Index

| Algorithm | Figure | Section | UDDP/SPP Component |
|---|---|---|---|
| **Invert** | 4.2 | 4.1.6 | UDDP M0–M5, SPP Layer 2 |
| **Euclid** | — | 4.2.1 | UDDP M0–M2 |
| **Euclid2** | — | 4.2.1 | UDDP M0–M2 |
| **findgcd** | 4.3 | 4.2.1 | UDDP M0–M10, SPP Layer 4 |
| **Hermite** | 4.4 | 4.2.3 | UDDP M4, SPP Layer 2 |
| **GE** | 4.5 | 4.3.1 | UDDP M4, SPP Layer 2 |
| **BackSubstitute** | 4.6 | 4.3.1 | UDDP M4, SPP Layer 2 |
| **ReduceRow** | 4.7 | 4.4.2 | UDDP M4, M6–M10 |
| **ReduceMatrix** | 4.8 | 4.4.3 | UDDP M4, SPP Layer 2 |
| **FM** | 4.10 | 4.5.1 | UDDP M4, SPP Layer 2 |
| **FMint** | 4.11 | 4.6 | UDDP M4, SPP Layer 2 |
| **IntegerInequality** | 4.12 | 4.6.2 | UDDP M4, SPP Layer 2 |
| **Extreme** | 4.13 | 4.7 | UDDP M0–M5, SPP Layer 2 |

---

## Complete Formula Index

| Formula | Section | UDDP/SPP Usage |
|---|---|---|
| $\operatorname{sign}(\alpha)$ | 4.1 | Direction vector computation |
| a<jb | 4.1 | Dependence level |
| (A) | 4.1.5 | Unimodularity test |
| cij=A[i]b[j] | 4.1.2 | Transformation composition |
| $f(\alpha\mathbf{x}+\beta\mathbf{y}) = \alpha f(\mathbf{x})+\beta f(\mathbf{y})$ | 4.1.2 | Linear function property |
| $\operatorname{rank}(\mathbf{A}) + \operatorname{nullity}(\mathbf{A}) = n$ | 4.1.2 | Solution space dimension |
| (a,b) | 4.2.1 | Gcd test |
| ax+by=(a,b) | 4.2.1 | Particular solution |
| (A)=1 | 4.2.1 | Unimodularity |
| AU=H | 4.2.3 | HNF decomposition |
| L(A)={ia[i]} | 4.2.3 | Lattice |
| a1x1+a2x2=b | 4.4.1 | Single equation |
| x1=x1'-(a2/g)t | 4.4.1 | General solution |
| $\gcd(a_1, \ldots, a_n) \mid b$ | 4.4.2 | Many unknowns |
| yk=i(aidivak)xi | 4.4.2 | Variable substitution |
| rj+1>rj | 4.4.3 | Column echelon form |
| Axc | 4.5 | System of inequalities |
| clxcu | 4.6.1 | Elimination |
| cu-cl--+1 | 4.6.1 | Dark shadow |
| a+l+a-uaxa+u+a-l | 4.7 | Bounds test |
| (I-L)xl | 4.7.1 | Nonrectangular bounds |
| (I-U)xu | 4.7.1 | Nonrectangular bounds |
| bn-1,j=bn,j+bnn+lnj+bnn-unj | 4.7.1 | Extreme value recursion |
| an-1,j=an,j+ann+un,j+ann-ln,j | 4.7.1 | Extreme value recursion |

---

## Summary: What Chapter 4 Gives UDDP + SPP

| Chapter 4 Section | UDDP Component | SPP Component |
|---|---|---|
| 4.1 Vector ordering | M0–M10 (direction vectors) | Layer 2 (interchange legality) |
| 4.1.2 Matrices | M0–M10 (dependence equations) | Layer 2 (transformation composition) |
| 4.1.3 Elementary row ops | M4 (equation reduction) | Layer 2 (skewing, interchange) |
| 4.1.4 Polyhedra | M0–M5 (iteration space) | Layer 2 (tiling) |
| 4.1.5 Determinants | M4 (unimodularity) | Layer 2 (volume preservation) |
| 4.1.6 Matrix inverse | M0–M5 (solving equations) | Layer 2 (new loop limits) |
| 4.2.1 Euclid | M0–M2 (gcd test) | Layer 4 (version counters) |
| 4.2.2 Integer matrices | M4 (integer equations) | Layer 2 (transformation matrices) |
| 4.2.3 HNF | M4 (integer solutions) | Layer 2 (non-unimodular transforms) |
| 4.2.4 Lattices | M4 (dependence distances) | Layer 2 (transformed iteration space) |
| 4.3 Linear systems | M4 (equation solving) | Layer 2 (loop limits) |
| 4.4 Integer equations | M4 (integer solutions) | Layer 2 (integer loop limits) |
| 4.5 Inequalities | M4 (real solutions) | Layer 2 (real loop limits) |
| 4.6 Integer inequalities | M4 (integer solutions) | Layer 2 (integer loop limits) |
| 4.6.2 Dark shadows | M4 (exact integer solutions) | Layer 4 (speculation guards) |
| 4.7 Extreme values | M0–M2 (bounds test) | Layer 2 (tile size) |
| 4.7.1 Nonrectangular | M0–M5 (triangular loops) | Layer 2 (triangular tiles) |

---

## What Chapter 4 Unlocks for UDDP

| UDDP Level | Chapter 4 Provides |
|---|---|
| **M0** | Bounds test (4.7), gcd test (4.2.1), direction vectors (4.1) |
| **M1** | Symbolic bounds via matrices (4.1.2) |
| **M2a** | Coupled subscripts via matrix equations (4.1.2, 4.3) |
| **M2b** | Parametric solutions via HNF (4.2.3) |
| **M3** | Scalar analysis via unimodular transformations (4.2.2) |
| **M4** | Fourier-Motzkin (4.5), dark shadows (4.6.2), HNF (4.2.3), integer equations (4.4) |
| **M5** | Loop-variant variables via variable substitution (4.4.2) |
| **M6** | Products via HNF reduction (4.2.3) |
| **M7** | Hash via bit-vector encoding (4.5) |
| **M8** | Recurrence via integer equations (4.4) |
| **M9** | Mod-p via integer inequalities (4.6) |
| **M10** | Fixed-point via nonrectangular regions (4.7.1) |

---

## What Chapter 4 Unlocks for SPP

| SPP Layer | Chapter 4 Provides |
|---|---|
| **Layer 2** | Fourier-Motzkin for loop limits (4.5), HNF for non-unimodular transforms (4.2.3), matrix inverse for transformation inversion (4.1.6) |
| **Layer 3** | Extreme values for tile size (4.7), nonrectangular bounds for triangular tiles (4.7.1) |
| **Layer 4** | Dark shadows for speculation guards (4.6.2), integer inequalities for version counters (4.6) |
| **Layer 5** | Unimodular transformations for vectorization (4.2.2), HNF for non-unimodular vectorization (4.2.3) |

---

## The One-Sentence Summary

> **Chapter 4 gives UDDP its mathematical engine (gcd, HNF, Fourier-Motzkin, dark shadows, extreme values) and SPP its transformation engine (unimodular matrices, matrix inverse, non-unimodular transformations, nonrectangular bounds). Every formula and algorithm in Chapter 4 is used somewhere in UDDP M0–M10 or SPP Layers 2–5.**
   # Chapters 5 & 6 — Complete Usage Map for UDDP + SPP

## Every Section, Every Formula, Every Algorithm — Where It Is Used

---

# Chapter 5 — Data Dependence

## 5.0 Core Dependence Concepts

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Dependence graph** | Vertices = statements; edges = ordering constraints | **UDDP M0–M10:** The fundamental representation. **SPP Layer 2:** Foundation for all transformations. |
| **Flow dependence (true)** | S1 defines, S2 uses | **UDDP:** `Claim::Exact` for RAW. **SPP:** Must preserve ordering. |
| **Anti-dependence** | S1 uses, S2 redefines | **UDDP:** `Claim::Exact` for WAR. **SPP:** Can be broken by scalar expansion. |
| **Output dependence** | Both define | **UDDP:** `Claim::Exact` for WAW. **SPP:** Can be broken by renaming. |
| **False dependences** | Anti + output | **UDDP:** Distinguished from true dependences. **SPP:** Targets for elimination. |
| **Notation** f,a,o | Flow, anti, output | **UDDP:** Claim type annotation. **Checker:** Verifying claim type. |
| **Precedence graph** | Acyclic dependence graph | **UDDP:** Loop-free program representation. **SPP:** Statement reordering. |
| **Address-based dependence** | Same address | **UDDP M0–M10:** The core question. |
| **Value-based dependence** | Same value | **UDDP:** Refinement of address-based. **SPP:** Enables more transformations. |
| **IN(S)** | Read set | **UDDP:** Dependence equation input. **SPP:** Effect summary. |
| **OUT(S)** | Write set | **UDDP:** Dependence equation output. **SPP:** Effect summary. |
| **Flow test** | $OUT(S_1) \cap IN(S_2) \neq \emptyset$ | **UDDP M0–M2:** Basic dependence test. |
| **Anti test** | $IN(S_1) \cap OUT(S_2) \neq \emptyset$ | **UDDP M0–M2:** Basic dependence test. |
| **Output test** | $OUT(S_1) \cap OUT(S_2) \neq \emptyset$ | **UDDP M0–M2:** Basic dependence test. |
| **Input dependence** | $IN(S_1) \cap IN(S_2) \neq \emptyset$ | **UDDP:** Usually ignored (does not constrain reordering). **SPP:** Used for locality (fission for locality). |

---

## 5.1 Data Dependence in Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Loop-carried dependence** | Between different iterations | **UDDP M0–M10:** The critical case. **SPP Layer 2:** Determines parallelizability. |
| **Loop-independent dependence** | Same iteration | **UDDP:** Distance vector = 0. **SPP:** Does not prevent parallelism. |
| **Lexically forward** | Source precedes target lexically | **UDDP:** Direction vector entry <. **SPP:** Fusion legality. |
| **Lexically backward** | Otherwise | **UDDP:** Direction vector entry >. **SPP:** Prevents fusion. |
| **Iteration space** | One point per iteration | **UDDP M0–M10:** Domain of dependence equation. **SPP Layer 2:** Domain of transformation. |
| **Iteration space dependence graph** | Edge from source to target iteration | **UDDP:** Full dependence representation. **SPP:** Transformation validity. |
| **Index variable iteration vector** | Values of loop index variables | **UDDP:** Index-space representation. |
| **Normalized iteration vector** | Iterations labeled 0,1,2, | **UDDP M0–M10:** Standard representation. **SPP:** Normalization for transformation. |
| **Seminormalized iteration vector** | Distance is 1, lower limit may depend on outer | **UDDP:** Triangular/trapezoidal loops. **SPP Layer 2:** Interchange of non-rectangular loops. |
| **Normalization formula** | ikn=(ikv-lk)sk | **UDDP M0–M10:** Converting index to iteration space. **SPP:** Loop normalization pass. |
| **Distance vector** | d=iT-iS | **UDDP:** `Claim::Exact(Vec<i64>)`. **SPP Layer 2:** Transformation legality. |
| **Distance notation** | Sp(d)*Sq | **UDDP:** Claim annotation. **Checker:** Verification. |
| **Direction vector** | Ordering vector of source vs. target | **UDDP:** Sign abstraction of distance. **SPP Layer 2:** Direction vector tests. |
| **Simple direction vector** | Only <,=,> | **UDDP:** Standard format. **SPP:** Standard tests. |
| **Direction with ** | Unknown direction | **UDDP:** Conservative approximation. **SPP:** Speculation trigger. |
| **Dependence level** | Loop nest level carrying dependence | **UDDP:** Leftmost non-equal component. **SPP Layer 2:** Determines which loop can be parallelized. |
| **Level rule** | All-equal → loop-independent, level  | **UDDP:** Level computation. **SPP:** Parallelization decision. |
| **Nontightly nested loops** | Outer loop contains statements outside inner | **UDDP:** Common loops only. **SPP Layer 2:** Code sinking (Ch. 9). |
| **Code sinking** | Sink outer body into inner loop | **SPP Layer 2:** Enables interchange. **UDDP:** Must add dependence vector entries. |

---

## 5.2 Data Dependence with Conditionals

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Alternate paths** | No dependence between exclusive branches | **UDDP M1:** Conditional dependence. **SPP:** Guarded execution. |
| **Conservative approximation** | Add all possible dependences | **UDDP:** Default when condition unknown. **SPP:** Speculation trigger. |
| **Conditional in loop** | Branch around statements | **UDDP M1:** Conditional dependence. **SPP Layer 4:** Runtime guards. |
| **Loop-carried unaffected** | Conditionals don't affect carried | **UDDP:** Simplification. **SPP:** Parallelization still possible. |

---

## 5.3 Data Dependence in Parallel Loops

| Construct | Semantics | UDDP/SPP Usage |
|---|---|---|
| **Data access conflict** | Two accesses same location | **UDDP:** Core problem. **SPP:** Conflict detection. |
| **Forall loop** | RHS computed before stores | **UDDP:** Negative distances possible. **SPP Layer 2:** Array assignment generation. |
| **Dopar loop** | Each iteration copies variables | **UDDP:** No flow dependences between iterations. **SPP Layer 2:** Parallel execution. |
| **Doall loop** | Dopar with no dependences | **UDDP:** `Claim::None`. **SPP:** Fully parallel. |
| **Dosingle loop** | Single-assignment rule | **UDDP:** No output/anti dependences. **SPP Layer 2:** Reduction pattern. |
| **Conflict distance vector** | dij=i-j | **UDDP:** Distance computation. **SPP:** Resolving conflicts. |
| **Nested loop resolution** | Outermost loop with nonzero conflict distance | **UDDP:** Level computation. **SPP:** Parallelization at correct level. |

---

## 5.4 Program Dependence Graph

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **PDG** | Data + control dependence subgraph | **UDDP:** Full dependence representation. **SPP:** Transformation validity. |
| **Control dependence edges** | Annotated with c | **UDDP:** Control flow constraints. **SPP:** Reordering legality. |

---

## 5.5 In the Pit (Implementation Notes)

| Topic | Usage |
|---|---|
| **Notation variations** | **UDDP:** Choose consistent notation. |
| **Direction vector representations** | **UDDP:** (+,-,0) vs (<,>,=) vs sign vector. |
| **Dependence graph data structure** | **UDDP:** Source, target, type, distance/direction. |
| **Distance vector flags** | **UDDP:** Unknown constants, min-max flags. |
| **Bit-vector direction vectors** | **UDDP:** 3 bits per loop level; 32-bit = 10 loops; 64-bit = 21 loops. |
| **Bitwise OR problem** | **UDDP:** Can introduce spurious (=,=). **SPP:** Conservative approximation. |
| **Extra bit for loop-independent** | **UDDP:** Distinguishes loop-independent from all-equal. |
| **Reduction direction** | **UDDP:** Special direction S2(R)fS2. **SPP:** Reduction parallelization. |

---

## 5.6 Further Reading

| Reference | Contribution |
|---|---|
| **Bernstein (1966)** | Conditions for parallelism |
| **Karp et al. (1967)** | Systolic arrays |
| **Allen & Kennedy (1987)** | Dependence level, loop-carried, loop-independent |
| **Kuck et al. (1981)** | Dependence analysis |
| **Allen et al. (1987)** | Dependence analysis |
| **Brandes (1988)** | Direct dependence = value-based dependence |

---

# Chapter 6 — Scalar Analysis with Factored Use-Def Chains

## 6.0 Core Concepts

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **FUD chains** | Sparse use-def representation | **UDDP M3, M5:** Scalar value flow. **SPP Layer 2:** Induction variable substitution. |
| **Reaching definitions** | Def reaches use if path with no other defs | **UDDP M3:** Scalar dependence. **SPP:** Constant propagation. |
| **Use-def chain** | Links from use to reaching defs | **UDDP M3:** Classical (non-sparse) representation. |
| **Def-use chain** | Links from def to reached uses | **UDDP M3:** Forward value flow. |
| **-term** | Pseudo-assignment at merge points | **UDDP M3:** Factoring multiple reaching definitions. **SPP:** SSA form. |

---

## 6.1 Constructing Factored Use-Def Chains

### 6.1.1 Conditions for -term

| Condition | Definition | UDDP/SPP Usage |
|---|---|---|
| **Condition 1** | Z has two distinct predecessors Y1,Y2 | **UDDP M3:** Merge point detection. |
| **Condition 2** | Paths $p_1: X_1 \to Y_1$, $p_2: X_2 \to Y_2$ | **UDDP M3:** Path analysis. |
| **Condition 3** | $p_1 \cap p_2 = \emptyset$ | **UDDP M3:** Disjoint paths. |
| **Condition 4** | No other defs along either path | **UDDP M3:** Reaching definition condition. |
| **Relaxation** | If another def on one path, use subpaths | **UDDP M3:** Algorithm robustness. |

### 6.1.2 Join Sets

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| J(X1,X2) | Set of merge points for defs X1,X2 | **UDDP M3:** -placement. |
| J(S)=J(X1,X2) | Join set of a set of defs | **UDDP M3:** Multi-def merge. |
| J1(S)=J(S) | First iteration | **UDDP M3:** Iterated join. |
| Ji(S)=J(SJi-1(S)) | i-th iteration | **UDDP M3:** Fixed point. |
| $J^+(S) = \lim_{i\to\infty} J^i(S)$ | Fixed point | **UDDP M3:** Complete -placement. |
| **Convention** $J(X,X) = \emptyset$ | Self-join is empty | **UDDP M3:** Avoiding self-loops. |

### 6.1.3 Equivalence to Iterated Dominance Frontier

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| DF(X) | Dominance frontier of X | **UDDP M3:** -placement via dominators. |
| DF(S)=DF(X) | Frontier of set | **UDDP M3:** Multi-def frontier. |
| DFi(S) | Iterated frontier | **UDDP M3:** Fixed point. |
| DF+(S) | Complete iterated frontier | **UDDP M3:** Complete -placement. |
| **Theorem** DF+(S)=J+(S) if EntryS | Equivalence | **UDDP M3:** Use iterated dominance frontier. |

### 6.1.4 Algorithm Placement (Figure 6.5)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm Placement** | Place -terms at DF+(D(M)) | **UDDP M3:** FUD chain construction. **SPP:** SSA construction. |

**Detailed usage:**
- **UDDP M3:** Scalar value flow analysis.
- **UDDP M5:** Loop-variant variable tracking.
- **UDDP M3.5:** Mutual recursion (companion analysis).
- **SPP Layer 2:** Induction variable substitution.
- **SPP Layer 2:** Scalar expansion.
- **Checker:** Verifying -placement claims.

### 6.1.5 Algorithm Chain (Figure 6.7)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm Chain** | Link each use to unique reaching def or -term | **UDDP M3:** FUD chain construction. **SPP:** SSA construction. |

**Data structures:**
| Structure | Purpose | UDDP/SPP Usage |
|---|---|---|
| Child(X) | Dominator children | **UDDP M3:** Tree traversal. |
| SUCC(X) | CFG successors | **UDDP M3:** Control flow. |
| $WhichPred(X \to Y)$ | Index of predecessor | **UDDP M3:** Edge labeling. |
| CurrDef(M) | Current definition of M | **UDDP M3:** Running state. |
| Chain(R) | Link from use to reaching def | **UDDP M3:** The FUD chain. |
| -Chain(R)[J] | Links from -term | **UDDP M3:** Merge factoring. |
| SaveChain(R) | Temporary save | **UDDP M3:** Backtracking. |

---

## 6.2 FUD Chains for Arrays

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Scalar assignment** | Killing definition | **UDDP M3:** Scalar FUD chains. |
| **Array assignment** | Nonkilling definition | **UDDP M3:** Array FUD chains. |
| **Def-def link** | Chain from def to previous def | **UDDP M3:** Finding all reaching definitions. |
| **Modification to Chain** | Add `Chain(R) = CurrDef(M)` for defs | **UDDP M3:** Array chain construction. |

**Detailed usage:**
- **UDDP M3:** Array scalar dependence.
- **UDDP M4:** Array dependence with aliasing.
- **UDDP M5:** Loop-variant arrays.
- **SPP Layer 2:** Array privatization.
- **SPP Layer 2:** Scalar replacement.

---

## 6.3 Finding All Reaching Definitions

### Algorithm FollowChain (Figure 6.10)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm FollowChain** | Find all reaching defs for use u | **UDDP M3:** Scalar dependence. **SPP:** Dependence refinement. |

**Data structures:**
| Structure | Purpose | UDDP/SPP Usage |
|---|---|---|
| Marked(d) | Visited flag | **UDDP M3:** Avoiding cycles. |
| Defs(u) | Set of reaching definitions | **UDDP M3:** The result. |

**Tests:**
| Test | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Can define?** | Whether d can define location at u | **UDDP M3:** Feasibility. |
| **Killing definition?** | Whether d kills location at u | **UDDP M3:** Chain termination. |

**Detailed usage:**
- **UDDP M3:** Scalar dependence analysis.
- **UDDP M5:** Loop-variant scalar tracking.
- **SPP Layer 2:** Constant propagation.
- **SPP Layer 2:** Dead code elimination.
- **Checker:** Verifying reaching definition claims.

---

## 6.4 Implicit References in FUD Chains

### 6.4.1 Input/Output

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Output statements** | Use explicitly named variables | **UDDP M3:** Explicit references. |
| **Input statements** | Modify arguments | **UDDP M3:** Implicit definitions. |
| **Error/EOF handlers** | Special cases | **UDDP M3:** Conservative approximation. |
| **Incomplete input** | Fortran list-directed/namelist | **UDDP M3:** Nonkilling definition. |

### 6.4.2 Procedure Calling

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Pass-by-value** | C semantics | **UDDP:** No modification. |
| **Pass-by-reference** | Fortran semantics | **UDDP:** Nonkilling definition. |
| **Reference arguments** | C++ references | **UDDP:** Mixed semantics. |
| **Globals** | Without info, assume all may be used/modified | **UDDP:** Conservative approximation. |
| **Phantom global variable** | Model invisible static/global state | **UDDP:** Interprocedural summary. **SPP:** Effect summary. |

### 6.4.3 Aliases

| Alias source | Definition | UDDP/SPP Usage |
|---|---|---|
| **Equivalence** | Fortran equivalence | **UDDP:** Must-alias. |
| **Variant records** | Pascal variant records | **UDDP:** May-alias. |
| **Union** | C union | **UDDP:** May-alias. |
| **Structure assignment** | Structure name alias for fields | **UDDP:** Field-sensitive aliasing. |
| **Formal parameters** | Aliased with each other or globals | **UDDP:** Interprocedural aliasing. |
| **Dereferenced pointers** | P distinct from P | **UDDP:** Points-to analysis. |
| **Static vs dynamic** | Compile-time vs runtime | **UDDP:** Static analysis preference. |
| **Must-alias vs may-alias** | Certain vs possible | **UDDP:** Precision levels. |

**Two methods for FUD chains with aliases:**

| Method | Approach | UDDP/SPP Usage |
|---|---|---|
| **One name per alias set** | Partition into equivalence classes | **UDDP M4:** Conservative. |
| **One chain per alias** | Keep separate chains | **UDDP M4:** More precise. |

**Detailed usage:**
- **UDDP M4:** Aliasing in dependence analysis.
- **UDDP M5:** Loop-variant aliases.
- **SPP Layer 2:** Scalar replacement with aliasing.
- **SPP Layer 4:** Speculation with aliasing.
- **Checker:** Verifying alias claims.

---

## 6.5 Induction Variables Using FUD Chains

### 6.5.1 Core Concepts

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Induction variable** | Value on next iteration = function of loop invariants and current value | **UDDP M3, M5:** Loop-variant scalars. **SPP Layer 2:** Induction variable substitution. |
| **Linear induction variable** | Cycle with only additions of loop-invariant values, one -term at header | **UDDP M3:** Linear IV detection. **SPP Layer 2:** Closed-form expression. |
| **Basic induction variable** | Cycle in FUD chain graph | **UDDP M3:** IV detection. |
| **Loop-header-** | Exactly two links: back edge and preheader | **UDDP M3:** IV detection. |

### 6.5.2 Induction Variable Algebra

| Rule | Formula | UDDP/SPP Usage |
|---|---|---|
| **Addition** | (a+bi)+c=(a+c)+bi | **UDDP M3:** IV substitution. **SPP Layer 2:** Closed-form expression. |
| **Multiplication** | (a+bi)c=(ac)+bci | **UDDP M3:** IV substitution. |
| **Addition of two IVs** | Sum sequences | **UDDP M3:** IV combination. |

### 6.5.3 Algorithm Induct (Figure 6.12)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm Induct** | Detect induction variables via SCC | **UDDP M3, M5:** IV detection. **SPP Layer 2:** IV substitution. |

**Data structures:**
| Structure | Purpose | UDDP/SPP Usage |
|---|---|---|
| Loop | Set of basic blocks | **UDDP M3:** Loop body. |
| Operations(X) | Set of operations in block | **UDDP M3:** Operation graph. |
| NPre(P) | Preorder number | **UDDP M3:** SCC algorithm. |
| Lowlink(P) | Lowlink value | **UDDP M3:** SCC algorithm. |
| InStack(P) | Stack membership | **UDDP M3:** SCC algorithm. |

### 6.5.4 Procedure Check (Figure 6.13)

| Procedure | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Procedure Check** | Update lowlink for edge | **UDDP M3:** SCC algorithm. |

### 6.5.5 Classification Rules

| Rule | Classification | UDDP/SPP Usage |
|---|---|---|
| 1 | Literal constant → constant | **UDDP M3:** Constant propagation. |
| 2 | Store operation → classification/value of RHS | **UDDP M3:** Value flow. |
| 3 | Fetch of loop-invariant address → classification/value of reaching def | **UDDP M3:** IV detection. |
| 4 | Other fetches → complex | **UDDP M3:** Non-IV. |
| 5 | Adding/multiplying two constants → constant | **UDDP M3:** Constant folding. |
| 6 | Adding two linear IVs → linear IV | **UDDP M3:** IV combination. |
| 7 | Adding complex to anything → complex | **UDDP M3:** Non-IV. |
| 8 | Loop-header- terms → complex | **UDDP M3:** Header handling. |
| 9 | Other -terms → same if operands identical, else complex | **UDDP M3:** Merge handling. |
| 10 | Nontrivial SCC → linear IV if all operations are additions of loop-invariant values | **UDDP M3:** IV classification. |

### 6.5.6 Wrap-Around Variables

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Wrap-around variable** | Loop-header- not in cycle | **UDDP M3.5:** Mutual recursion. **SPP Layer 2:** Peeling. |
| **Level 1** | Back-edge link is linear IV | **UDDP M3.5:** Recursion classification. |
| **Level k+1** | Back-edge link is wrap-around level k | **UDDP M3.5:** Nested recursion. |
| **Signal to peel** | Peel first iteration(s) and optimize | **SPP Layer 2:** Loop peeling. |

### 6.5.7 Trip Counts

| Concept | Formula | UDDP/SPP Usage |
|---|---|---|
| **Trip count (tc)** | Number of back edge executions | **UDDP M3:** Loop bound. **SPP Layer 2:** Fusion, peeling. |
| **Single loop exit** | Find minimum tc satisfying inequality | **UDDP M3:** Trip count computation. |
| **Inequality** | n1+s1tc<n2+s2tc | **UDDP M3:** Exit condition. |
| **Countable loop** | Trip count independent of loop-computed values | **UDDP M3:** Static trip count. **SPP:** Static scheduling. |
| **Exit value** | init+steptc | **UDDP M3:** Final value. **SPP Layer 2:** Post-loop value. |

### 6.5.8 Nested Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Process innermost first** | Bottom-up traversal | **UDDP M3:** Nested IV detection. |
| **Outer loop modification** | If link to inner-loop IV with exit value, treat as link to exit value | **UDDP M3:** Inter-loop value flow. |
| **No exit value** | Treat as complex | **UDDP M3:** Conservative. |
| **Initial value links to outer** | Trace to constant if possible | **UDDP M3:** Constant propagation. |

**Detailed usage:**
- **UDDP M3:** Scalar dependence analysis.
- **UDDP M5:** Loop-variant variable tracking.
- **UDDP M3.5:** Mutual recursion (companion analysis).
- **UDDP M8:** Nonlinear recurrence (via IV substitution).
- **SPP Layer 2:** Induction variable substitution.
- **SPP Layer 2:** Loop peeling.
- **SPP Layer 2:** Loop fusion.
- **SPP Layer 2:** Strip-mining.
- **SPP Layer 2:** Loop collapsing.
- **SPP Layer 3:** Locality optimization (IV for address computation).
- **Checker:** Verifying IV claims.

---

## 6.6 Constant Propagation with FUD Chains

### 6.6.1 Abstract Values

| Value | Meaning | UDDP/SPP Usage |
|---|---|---|
| **Constant** | Known value | **UDDP M3:** Value refinement. **SPP:** Constant folding. |
|  (top) | Not evaluated yet or unreachable | **UDDP M3:** Initial state. |
| $\perp$ (bottom) | May take two or more values | **UDDP M3:** Conservative. |

### 6.6.2 Rules

| Rule | Condition | Result | UDDP/SPP Usage |
|---|---|---|---|
| 1 | Any operand $\perp$ | $\perp$ | **UDDP M3:** Conservative propagation. |
| 2 | Any operand  |  | **UDDP M3:** Unknown propagation. |
| 3 | All operands constant | Constant | **UDDP M3:** Constant folding. |
| **-value** | All operands  |  | **UDDP M3:** Merge handling. |
| **-value** | All non- same constant | Constant | **UDDP M3:** Merge refinement. |
| **-value** | Otherwise | $\perp$ | **UDDP M3:** Conservative merge. |

### 6.6.3 Algorithm ConstantProp (Figure 6.18)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm ConstantProp** | Sparse constant propagation | **UDDP M3:** Value refinement. **SPP Layer 2:** Constant folding. |

**Data structures:**
| Structure | Purpose | UDDP/SPP Usage |
|---|---|---|
| V | CFG nodes | **UDDP M3:** Program representation. |
| E | CFG edges | **UDDP M3:** Control flow. |
| SUCC(v) | Successors | **UDDP M3:** Control flow. |
| Count(SUCC(v)) | Number of successors | **UDDP M3:** Branch handling. |
| Target(s), Source(s) | Def-use link endpoints | **UDDP M3:** Value flow. |
| Reachable(e), Reachable(v) | Reachability flags | **UDDP M3:** Dead code detection. |
| Value(p) | Abstract value | **UDDP M3:** The result. |
| CFGWorkList, DefUseWorkList | Work lists | **UDDP M3:** Algorithm state. |

### 6.6.4 Procedures (Figure 6.19)

| Procedure | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Visit-** | Update -term value | **UDDP M3:** Merge handling. |
| **VisitExpression** | Update expression value | **UDDP M3:** Value propagation. |
| **AddCFGEdge** | Add edge to work list | **UDDP M3:** Control flow. |

### 6.6.5 Termination

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Abstract value changes** | $\top \to$ constant or $\perp$, then constant $\to \perp$ | **UDDP M3:** Termination guarantee. |
| **At most twice per expression** | Bounded changes | **UDDP M3:** Complexity bound. |

**Detailed usage:**
- **UDDP M3:** Scalar value analysis.
- **UDDP M5:** Loop-variant constant detection.
- **SPP Layer 2:** Constant folding.
- **SPP Layer 2:** Dead code elimination.
- **SPP Layer 2:** Loop unswitching.
- **SPP Layer 3:** Trip count computation.
- **Checker:** Verifying constant claims.

---

## 6.7 Data Dependence for Scalars

### 6.7.1 Four Scalar Dependence Situations

| Situation | Distance/Direction | UDDP/SPP Usage |
|---|---|---|
| **Only current iteration** | (0) | **UDDP M3:** Loop-independent. |
| **Only one previous iteration** | (1) | **UDDP M3:** Distance 1. |
| **Any previous iteration** | (<) | **UDDP M3:** Direction <. |
| **Current or any previous** | () | **UDDP M3:** Direction . |

### 6.7.2 Algorithm ScalarDD (Figure 6.21)

| Algorithm | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Algorithm ScalarDD** | Scalar dependence analysis via FUD chains | **UDDP M3, M5:** Scalar dependence. **SPP:** Dependence refinement. |

**Data structures:**
| Structure | Purpose | UDDP/SPP Usage |
|---|---|---|
| Chain(p) | FUD chain | **UDDP M3:** Value flow. |
| -Chain(p)$ | -term chains | **UDDP M3:** Merge flow. |
| Uses | Set of uses | **UDDP M3:** Dependence targets. |
| Reaching(lh) | Definitions reaching loop-header- | **UDDP M3:** Loop-carried analysis. |
| Self(lh) | Flag if loop-header- reaches itself | **UDDP M3:** Self-dependence. |
| Marked(d) | Visited flag | **UDDP M3:** Avoiding cycles. |
| MarkedReaching(d) | Reaching visited flag | **UDDP M3:** Avoiding cycles. |

### 6.7.3 Procedure FindReaching (Figure 6.22)

| Procedure | Purpose | UDDP/SPP Usage |
|---|---|---|
| **Procedure FindReaching** | Find definitions reaching loop-header- | **UDDP M3:** Loop-carried analysis. |

### 6.7.4 Output Dependence

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Def-def links** | Links between definitions | **UDDP M3:** Output dependence. |
| **ScalarDD from Chain(d)** | Start from definition | **UDDP M3:** Output dependence computation. |

### 6.7.5 Anti-Dependence

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **FRUD chains** | Factored redef-use chains | **UDDP M3:** Anti-dependence. |
| **-terms** | Upsilon-terms (analog of ) | **UDDP M3:** Merge for uses. |
| **Algorithm Placement with uses(M)** | Place -terms | **UDDP M3:** Anti-dependence construction. |
| **Algorithm FRUD-Chain** | Modification of Chain | **UDDP M3:** Anti-dependence chain. |
| **Every use linked to CurrUse(M)** | Current use tracking | **UDDP M3:** Anti-dependence. |
| **Killing definition** | $CurrUse(M) = \perp$ | **UDDP M3:** Chain termination. |

**Detailed usage:**
- **UDDP M3:** Scalar dependence.
- **UDDP M5:** Loop-variant scalar dependence.
- **SPP Layer 2:** Scalar expansion.
- **SPP Layer 2:** Renaming.
- **Checker:** Verifying scalar dependence claims.

---

## 6.8 In the Pit (Implementation Notes)

| Topic | Usage |
|---|---|
| **FUD = SSA interpretation** | **UDDP M3:** SSA construction. |
| **-term count** | **UDDP M3:** Linear in practice, nonlinear worst-case. |
| **IV detection targets** | **UDDP M3:** Array dependence, not strength reduction. |
| **Constant propagation** | **UDDP M3:** Includes constant folding; floating-point caveats (IEEE NaN, rounding mode). |

---

## 6.9 Further Reading

| Reference | Contribution |
|---|---|
| **Cytron et al. (1991)** | FUD/SSA algorithms |
| **Choi et al. (1994)** | Factored SSA |
| **Johnson & Pingali (1993)** | Sparse data-flow graph |
| **Ballance et al. (1990)** | Program dependence web |
| **Havlak (1993)** | Gated single assignment |
| **Wolfe (1992)** | Induction variables using FUD chains |
| **Wegman & Zadeck (1991)** | Constant propagation |
| **Callahan et al. (1986)** | Interprocedural constant propagation |
| **Cytron & Gershbein (1993)** | Aliases in SSA/FUD |

---

# Complete Algorithm Index

| Algorithm | Figure | Chapter | UDDP/SPP Component |
|---|---|---|---|
| **Placement** | 6.5 | 6.1 | UDDP M3, SPP Layer 2 |
| **Chain** | 6.7 | 6.1 | UDDP M3, SPP Layer 2 |
| **FollowChain** | 6.10 | 6.3 | UDDP M3, SPP Layer 2 |
| **Induct** | 6.12 | 6.5 | UDDP M3, M5, SPP Layer 2 |
| **Check** | 6.13 | 6.5 | UDDP M3, SPP Layer 2 |
| **ConstantProp** | 6.18 | 6.6 | UDDP M3, SPP Layer 2 |
| **Visit-** | 6.19 | 6.6 | UDDP M3 |
| **VisitExpression** | 6.19 | 6.6 | UDDP M3 |
| **AddCFGEdge** | 6.19 | 6.6 | UDDP M3 |
| **ScalarDD** | 6.21 | 6.7 | UDDP M3, M5 |
| **FindReaching** | 6.22 | 6.7 | UDDP M3 |
| **FRUD-Chain** | 6.23 | 6.7 | UDDP M3 |

---

# Complete Formula Index

| Formula | Chapter | Section | UDDP/SPP Usage |
|---|---|---|---|
| d=iT-iS | 5 | 5.1.3 | Distance vector |
| Sp(d)*Sq | 5 | 5.1.3 | Dependence notation |
| ikn=(ikv-lk)sk | 5 | 5.1.2 | Normalization |
| dij=i-j | 5 | 5.3.5 | Conflict distance |
| J(X1,X2) | 6 | 6.1.2 | Join set |
| J(S)=J(X1,X2) | 6 | 6.1.2 | Join set of set |
| Ji(S) | 6 | 6.1.2 | Iterated join |
| J+(S) | 6 | 6.1.2 | Fixed point |
| DF(X) | 6 | 6.1.3 | Dominance frontier |
| DF+(S) | 6 | 6.1.3 | Iterated frontier |
| DF+(S)=J+(S) | 6 | 6.1.3 | Equivalence |
| (a+bi)+c=(a+c)+bi | 6 | 6.5.2 | IV addition |
| (a+bi)c=(ac)+bci | 6 | 6.5.2 | IV multiplication |
| n1+s1tc<n2+s2tc | 6 | 6.5.7 | Trip count inequality |
| init+steptc | 6 | 6.5.7 | Exit value |

---

# Summary: What Chapters 5 & 6 Give UDDP + SPP

| Chapter | Section | UDDP Component | SPP Component |
|---|---|---|---|
| **5** | 5.0 Core concepts | M0–M10 (dependence types) | Layer 2 (transformation legality) |
| **5** | 5.1 Loops | M0–M10 (distance, direction, level) | Layer 2 (parallelization) |
| **5** | 5.2 Conditionals | M1 (conditional dependence) | Layer 4 (guards) |
| **5** | 5.3 Parallel loops | M0–M10 (forall, dopar, doall) | Layer 2 (parallel execution) |
| **5** | 5.4 PDG | M0–M10 (full representation) | Layer 2 (transformation) |
| **5** | 5.5 In the pit | M0–M10 (implementation) | Layer 2 (encoding) |
| **6** | 6.1 FUD chains | M3 (scalar value flow) | Layer 2 (SSA, IV subst) |
| **6** | 6.2 Arrays | M3 (array value flow) | Layer 2 (privatization) |
| **6** | 6.3 Reaching defs | M3 (scalar dependence) | Layer 2 (constant prop) |
| **6** | 6.4 Implicit refs | M3 (I/O, calls, aliases) | Layer 2 (effect summary) |
| **6** | 6.5 IVs | M3, M5, M3.5 (IV detection) | Layer 2 (IV subst, peeling) |
| **6** | 6.6 Constant prop | M3 (value refinement) | Layer 2 (folding, DCE) |
| **6** | 6.7 Scalar DD | M3, M5 (scalar dependence) | Layer 2 (expansion, renaming) |
| **6** | 6.8 In the pit | M3 (SSA interpretation) | Layer 2 (implementation) |

---

# What Chapters 5 & 6 Unlock for UDDP

| UDDP Level | Chapter 5 Provides | Chapter 6 Provides |
|---|---|---|
| **M0** | Dependence types, distance, direction, level | — |
| **M1** | Conditional dependence | — |
| **M2a** | Nested loop dependences | — |
| **M2b** | Parallel loop semantics | — |
| **M3** | — | FUD chains, reaching defs, scalar DD |
| **M3.5** | — | Wrap-around variables, mutual recursion |
| **M4** | — | Array FUD chains, aliases |
| **M5** | — | IV detection, loop-variant scalars |
| **M6** | — | Constant propagation |
| **M7** | — | — |
| **M8** | — | IV substitution, recurrence detection |
| **M9** | — | — |
| **M10** | — | Wrap-around variables, trip counts |

---

# What Chapters 5 & 6 Unlock for SPP

| SPP Layer | Chapter 5 Provides | Chapter 6 Provides |
|---|---|---|
| **Layer 2** | Dependence types, distance, direction, level, parallel loop semantics | FUD chains, IV substitution, constant propagation, scalar DD |
| **Layer 3** | — | IV for address computation, trip counts |
| **Layer 4** | Conditional dependence, parallel loop conflicts | — |
| **Layer 5** | — | Constant folding, dead code elimination |

---

# The One-Sentence Summary

> **Chapter 5 gives UDDP its dependence vocabulary (distance, direction, level, loop-carried vs. loop-independent, parallel loop semantics) and SPP its transformation legality foundation. Chapter 6 gives UDDP its scalar value-flow engine (FUD chains, reaching definitions, induction variables, constant propagation, scalar dependence) and SPP its SSA-based optimization foundation (IV substitution, constant folding, dead code elimination, scalar expansion). Every formula and algorithm in Chapters 5 and 6 is used somewhere in UDDP M0–M10 or SPP Layers 2–5.**
  # Chapters 10–12 — Complete Usage Map for UDDP + SPP

## Every Section, Every Formula, Every Algorithm — Where It Is Used

---

# Chapter 10 — Optimizing for Locality

## 10.0 Core Concepts

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Memory hierarchy** | Register → L1 → L2 → L3 → DRAM → Disk | **SPP Layer 3:** Hardware model. **SPP-HX:** Cache-aware scheduling. |
| **Inclusion principle** | Higher level ⊂ next level | **SPP Layer 3:** Cache model. |
| **Cache line** | Unit of transfer | **SPP Layer 3:** Tile size selection. |
| **Reuse** | Element fetched/stored more than once | **SPP Layer 3:** Locality analysis. |
| **Reuse distance** | Distance in memory locations between two references | **SPP Layer 3:** Locality optimization. |
| **Temporal reuse** | Same location at different times | **SPP Layer 3:** Cache blocking. |
| **Spatial reuse** | Nearby locations | **SPP Layer 3:** Cache line utilization. |
| **Sequential reuse** | Consecutive locations (distance ±1) | **SPP Layer 3:** Stride-1 access. |
| **Locality** | Location still in higher level at reuse time | **SPP Layer 3:** The goal. |
| **Temporal locality** | Reuse of same location | **SPP Layer 3:** Tiling. |
| **Spatial locality** | Reuse of nearby locations | **SPP Layer 3:** Cache line utilization. |
| **Sequential locality** | Consecutive accesses | **SPP Layer 3:** Stride-1 inner loop. |

**Detailed usage:**
- **UDDP M0–M10:** Dependence analysis determines what reuse is legal.
- **SPP Layer 3 (SPP-HX):** Locality is the primary optimization target.
- **SPP Layer 2:** Fusion/fission for locality.
- **Checker:** Verifying locality claims.

---

## 10.1 Single Reference to Each Array

### 10.1.1 Loop Reordering for Single Reference

| Rule | Condition | UDDP/SPP Usage |
|---|---|---|
| **Stride-1 innermost** | Loop index appears only in stride-1 dimension with coefficient ±1 | **SPP Layer 3:** Memory access optimization. |
| **Invariant address** | Memory address independent of loop | **SPP Layer 3:** Temporal reuse. |
| **Scalar replacement** | Address invariant in inner loop | **SPP Layer 3:** Register allocation. |
| **Loop step assumption** | Step = 1; otherwise multiply coefficient | **SPP Layer 3:** Stride computation. |

### 10.1.2 Tiling

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Tiling** | Block iteration space for cache | **SPP Layer 3:** Cache blocking. |
| **Tile size B** | Data reused B times before moving | **SPP Layer 3:** Cache capacity. |
| **Tile loop order** | Enhances locality between tiles | **SPP Layer 3:** Tile scheduling. |

### 10.1.3 Quantifying Reuse

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Self-temporal reuse** | Ad=0 | **SPP Layer 3:** Reuse detection. |
| **Null space** | A | **SPP Layer 3:** Reuse directions. |
| **Loop carries reuse** | dA with dk first nonzero | **SPP Layer 3:** Reuse-carrying loop. |
| **Zero coefficients** | A has zero in kth column | **SPP Layer 3:** Practical self-temporal test. |
| **Spatial reuse** | ASd=0 | **SPP Layer 3:** Spatial reuse detection. |
| **Reduced matrix** | AS = eliminate stride-1 row | **SPP Layer 3:** Spatial reuse. |
| **Spatial reuse condition** | ekAS and coefficient < cache line | **SPP Layer 3:** Spatial reuse. |

#### Reuse Factor

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Temporal reuse factor** | Nk if ekA, else 1 | **SPP Layer 3:** Quantifying reuse. |
| **Spatial reuse factor** | (1,l/ask) if ekAS, else 1 | **SPP Layer 3:** Spatial reuse quantification. |
| **Self-reuse factor** Rk | Temporal if > 1, else spatial | **SPP Layer 3:** Combined reuse. |
| **Cumulative self-reuse** Rk* | Product of self-reuse factors for loop k and inner loops | **SPP Layer 3:** Total reuse. |
| **Cumulative reuse factor** | Sum of cumulative self-reuse for all references | **SPP Layer 3:** Loop ordering. |

#### Data Footprint

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Footprint** | Fk*=li=knNiRi | **SPP Layer 3:** Cache capacity check. |
| **Cache locality condition** | Footprint fits into cache | **SPP Layer 3:** Tile size selection. |

#### Degree of Reuse

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Temporal locality degree** | $R_k^D = \dim(\operatorname{span}(e_k, \ldots, e_n) \cap \ker A)$ | **SPP Layer 3:** Quantifying reuse. |
| **Footprint degree** | d-RkD | **SPP Layer 3:** Reuse dimension. |
| **Spatial reuse degree** | Use AS | **SPP Layer 3:** Spatial reuse. |
| **Computation** | HNF of kernel basis; count zero entries | **SPP Layer 3:** Algorithm. |

### 10.1.4 Other Considerations

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Limited associativity** | Cache line conflicts | **SPP Layer 3:** Conflict avoidance. |
| **Copying arrays** | Copy to conflict-free locations | **SPP Layer 3:** Cache optimization. |
| **Prefetch** | Insert prefetch for next iteration | **SPP Layer 5:** Prefetch insertion. |
| **Prefetch distance** | Iterations between prefetch and fetch | **SPP Layer 5:** Prefetch tuning. |
| **Cache bypass** | Bypass cache for no-reuse references | **SPP Layer 5:** Cache optimization. |

---

## 10.2 Multiple References

### 10.2.1 Group Reuse

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Group reuse** | Reuse between different references | **SPP Layer 3:** Multi-reference locality. |
| **Same access matrix** | X[Ai+c1], X[Ai+c2] | **SPP Layer 3:** Group reuse condition. |
| **Temporal reuse** | Ad=c1-c2 | **SPP Layer 3:** Group temporal reuse. |
| **Solutions** | d+x, xA | **SPP Layer 3:** Reuse distance. |
| **Orthogonal condition** | dx=0 | **SPP Layer 3:** Preferred solution. |
| **d=0** | Same location same iteration | **SPP Layer 3:** Loop-independent reuse. |
| **Reuse-carrying loop** | Outermost nonzero entry in d | **SPP Layer 3:** Loop ordering. |

#### Group-Temporal Reuse Factor

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **d=0** |  for all loops | **SPP Layer 3:** Full reuse. |
| **d0** | Nk/dk for loop k, 1 for others | **SPP Layer 3:** Reuse quantification. |
| **Sorting** | Lexicographically nonnegative reuse distance | **SPP Layer 3:** Reference ordering. |

#### Group-Spatial Reuse

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Distance** | ASd=cS,1-cS,2 | **SPP Layer 3:** Group spatial reuse. |
| **Condition** | Stride-1 dimension, distance < cache line | **SPP Layer 3:** Spatial reuse. |
| **Self-spatial reuse** | If one reference has it, all do | **SPP Layer 3:** Simplification. |
| **Improvement condition** | gcd of coefficients > 1 and offset not multiple | **SPP Layer 3:** Group spatial reuse. |
| **Orthogonal condition** | dx=0, xAS | **SPP Layer 3:** Preferred solution. |
| **Factor** | 1 if self-spatial or gcd divides offset | **SPP Layer 3:** Reuse factor. |
| **d=0** | Nk for innermost loop | **SPP Layer 3:** Full reuse. |
| **d0** | Nk/dk for loop k | **SPP Layer 3:** Reuse quantification. |
| **Total group reuse** | Product of group-temporal and group-spatial | **SPP Layer 3:** Combined reuse. |
| **Cumulative group reuse** | Product for loop and inner loops | **SPP Layer 3:** Total reuse. |
| **Generalized reuse factor** | Multiply cumulative self-reuse by cumulative group reuse | **SPP Layer 3:** Complete quantification. |
| **Generalized footprint** | Divide footprint by cumulative group reuse | **SPP Layer 3:** Complete footprint. |

### 10.2.2 Reducing the Reuse Distance

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Striping** | Stripe factor = reuse distance | **SPP Layer 2:** Reduce reuse distance. |
| **Multiple distances** | Stripe factor = gcd of distances | **SPP Layer 2:** Multiple reuse distances. |
| **Legality** | Dependence tests necessary | **UDDP:** Legality check. |
| **Alignment** | Change reuse distance | **SPP Layer 2:** Alignment. |

### 10.2.3 Reordering Loops for Locality

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Degree of reuse** | $\dim(\operatorname{span}(e_k,\ldots,e_n) \cap \{d + \ker A\})$ | **SPP Layer 3:** Reuse degree. |
| **Move reuse loop inner** | When ek{d+A} | **SPP Layer 3:** Loop ordering. |
| **Self-temporal reduction** | Order of trip count | **SPP Layer 3:** Memory traffic. |
| **Self-spatial reduction** | No more than cache line length | **SPP Layer 3:** Memory traffic. |
| **Group reduction** | At most number of references | **SPP Layer 3:** Memory traffic. |

#### Sorting Loops by Reuse Factor

| Step | Definition | UDDP/SPP Usage |
|---|---|---|
| **Compute innermost** | As if innermost | **SPP Layer 3:** Heuristic. |
| **Not reuse-carrying** | Reuse distance > 1 nonzero | **SPP Layer 3:** Conservative. |
| **Group-spatial** | Treat as innermost | **SPP Layer 3:** Spatial reuse. |
| **Choose highest** | Sum innermost reuse factors | **SPP Layer 3:** Loop ordering. |
| **Fallback** | Next best if best cannot be innermost | **SPP Layer 3:** Constraint handling. |

---

## 10.3 General Tiling

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Tiling** | Improves self and group locality | **SPP Layer 3:** Cache blocking. |
| **No reuse** | No benefit tiling | **SPP Layer 3:** Profitability. |
| **Skewing** | Enable interchanges for tiling | **SPP Layer 2:** Skewing. |
| **Fully permutable** | All dependence distances nonnegative | **SPP Layer 2:** Tiling condition. |
| **Nontightly nested** | Code sinking may be required | **SPP Layer 2:** Code sinking. |
| **Element loop order** | Independent of tile | **SPP Layer 3:** Tile scheduling. |
| **Temporal locality** | Reorder for temporal | **SPP Layer 3:** Element order. |
| **Sequential locality** | Inner loops | **SPP Layer 3:** Memory stride. |
| **Tile loop order** | Inner tile carries most reuse | **SPP Layer 3:** Tile scheduling. |
| **Multiple levels** | Tile for each cache level | **SPP Layer 3:** Hierarchical tiling. |
| **Virtual memory** | Optimize for VM | **SPP Layer 3:** VM-aware tiling. |

---

## 10.4 Fission and Fusion for Locality

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Tiling** | Addresses loop limits | **SPP Layer 2:** Tiling. |
| **Fission** | Reduces arrays per loop | **SPP Layer 2:** Fission. |
| **Regions** | SCCs in dependence graph | **SPP Layer 2:** Fission algorithm. |
| **Larger partitions** | Disjoint data | **SPP Layer 2:** Locality. |
| **Ignore stride-1** | When computing dependence graph | **SPP Layer 3:** Reuse dependence graph. |
| **Input dependence** | Reuse dependence graph | **SPP Layer 3:** Fission for locality. |
| **Fission before tiling** | Allows larger tiles | **SPP Layer 2:** Fission first. |
| **Fusion** | Enhances reuse | **SPP Layer 2:** Fusion. |

---

## 10.5 In the Pit (Implementation Notes)

| Topic | Usage |
|---|---|
| **Stride-1 dimension** | **SPP Layer 3:** Must determine for each array. |
| **Higher-level constructs** | **SPP Layer 3:** Dynamically allocated arrays, array pointers, assumed-shape. |
| **Register level** | **SPP Layer 3:** Temporal locality for registers. |
| **Pair loads** | **SPP Layer 5:** Sequential locality + unroll. |

---

## 10.6 Further Reading

| Reference | Contribution |
|---|---|
| **McKellar & Coffman (1969)** | Submatrix formulation (tiling) for paged VM |
| **Abu-Sufah et al. (1981)** | Earliest automatic restructuring for memory hierarchy |
| **Dongarra et al. (1990)** | Level-3 BLAS |
| **Callahan & Porterfield (1990)** | Cache misses waste up to half time |
| **Goodman (1983)** | Cache performance characteristics |
| **Lam et al. (1991)** | Optimizing for locality, tile size, copying |
| **Temam et al. (1993)** | Heuristics for copying data |
| **Gannon et al. (1988)** | Reference window |
| **Wolf & Lam (1991b)** | Reuse vs locality, self-reuse vs group-reuse |
| **Carr & Kennedy (1992)** | Strengths/weaknesses of tiling |
| **Gornish et al. (1990)** | Inserting prefetch operations |
| **Callahan et al. (1991)** | Prefetch insertion |

---

# Chapter 11 — Concurrency Analysis

## 11.0 Core Concepts

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Master** | Single process executing main program | **SPP Layer 2:** Execution model. |
| **Slaves** | Workers initiated on parallel region | **SPP Layer 2:** Worker pool. |
| **Barrier** | Synchronization point | **SPP Layer 2:** Synchronization. |
| **Load imbalance** | Uneven work distribution | **SPP Layer 3:** Load balancing. |
| **Heavyweight worker** | Start once, park at barrier | **SPP Layer 2:** Worker implementation. |
| **Lightweight worker** | Start/terminate at barrier | **SPP Layer 2:** Worker implementation. |
| **Master participates** | Master also does work | **SPP Layer 2:** Execution model. |
| **Dedicated environment** | One worker per processor | **SPP Layer 2:** Scheduling. |
| **Multiprogramming** | Workers scheduled competitively | **SPP Layer 2:** Scheduling. |

---

## 11.1 Code for Concurrent Loops

### 11.1.1 Scheduling Policies

| Policy | Definition | UDDP/SPP Usage |
|---|---|---|
| **Static iteration scheduling** | Each worker gets info to characterize iterations | **SPP Layer 2:** Prescheduling. |
| **Block scheduling** | $\lceil N/P \rceil$ consecutive iterations | **SPP Layer 2:** Block scheduling. |
| **Cyclic scheduling** | Every Pth iteration | **SPP Layer 2:** Cyclic scheduling. |
| **Dynamic scheduling** | Small units assigned; workers compete | **SPP Layer 2:** Self-scheduling. |
| **Chunk scheduling** | Assign chunks of iterations | **SPP Layer 2:** Chunk scheduling. |
| **Chunk size = N/P** | Reduces to prescheduling | **SPP Layer 2:** Degenerate case. |

#### Tapering

| Formula | Definition | UDDP/SPP Usage |
|---|---|---|
| **Guided self-scheduling** | R0=N, $C_i = \lceil R_{i-1}/P \rceil$, Ri=Ri-1-Ci | **SPP Layer 2:** Guided scheduling. |
| **Variation** | $C_i = \lceil R_{i-1}/(2P) \rceil$ | **SPP Layer 2:** Guided variation. |
| **Factoring** | R0=N, $C_i = \lceil R_{i-1}/(fP) \rceil$, Ri=Ri-1-PCi | **SPP Layer 2:** Factoring. |
| **Fixed fraction** | f = e.g. 2; schedules P chunks per operation | **SPP Layer 2:** Factoring parameter. |

#### Master/Slave Code

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Prescheduled** | Master sets shared variables, releases workers | **SPP Layer 2:** Prescheduled code. |
| **Self-scheduled** | Workers enter critical section for next iteration | **SPP Layer 2:** Self-scheduled code. |
| **Nested concurrent loops** | Modify worker code for multidimensional iteration space | **SPP Layer 2:** Nested loops. |
| **Generator queue** | Each generator returns next parallel task | **SPP Layer 2:** Dynamic scheduling. |

### 11.1.3 Private Variables

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Private variable** | Value not available to other workers | **SPP Layer 2:** Variable classification. |
| **Worker procedure** | Allocate once per worker | **SPP Layer 2:** Allocation. |
| **Registers** | Private scalars can be in registers | **SPP Layer 5:** Register allocation. |
| **No upward-exposed uses** | Private variables cannot have them | **UDDP M3:** Dependence check. |
| **Loop index private** | Loop index is private | **SPP Layer 2:** Classification. |

### 11.1.4 Reductions

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Associative and commutative** | Efficient reduction condition | **UDDP:** Reduction detection. **SPP Layer 2:** Parallel reduction. |
| **Private partial sum** | Each worker accumulates | **SPP Layer 2:** Reduction implementation. |
| **Global sum** | Accumulate from partial sums | **SPP Layer 2:** Reduction finalization. |
| **Order of accumulation** | Depends on scheduling and finish order | **SPP Layer 2:** Roundoff consideration. |

### 11.1.5 Synchronization for Dependence

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **No synchronization** | Data-independent iterations | **SPP Layer 2:** Parallel execution. |
| **Critical section** | Only one worker active | **SPP Layer 2:** Synchronization primitive. |
| **Lock** | Protects critical section | **SPP Layer 2:** Synchronization primitive. |
| **Ordered critical section** | Workers enter in predetermined order | **SPP Layer 2:** Ordered synchronization. |
| **`await`** | Block until counter reaches value | **SPP Layer 2:** Synchronization primitive. |
| **`advance`** | Atomically increment counter | **SPP Layer 2:** Synchronization primitive. |
| **Memory consistency** | Store + advance; await + fetch | **SPP Layer 2:** Memory model. |
| **Lexically backward** | Enclose source and target in critical section | **SPP Layer 2:** Synchronization. |
| **Lexically forward** | Empty ordered critical section | **SPP Layer 2:** Synchronization. |
| **Distance d known** | Iteration i awaits until i-d advanced | **SPP Layer 2:** Distance-based sync. |
| **Distance unknown** | Await until i-1 complete | **SPP Layer 2:** Conservative sync. |
| **No chunk scheduling** | Synchronized loop scheduled one at a time | **SPP Layer 2:** Scheduling constraint. |
| **Synchronization counter** | Shared register or fast memory | **SPP Layer 2:** Sync implementation. |
| **Prescheduled** | One counter per worker | **SPP Layer 2:** Sync implementation. |
| **Synchronization vector** | One word/bit per iteration | **SPP Layer 2:** Fine-grained sync. |

#### Busy-Waiting

| Algorithm | Definition | UDDP/SPP Usage |
|---|---|---|
| **Swap lock** | `mylock = 1; repeat swap(lock, mylock) until mylock = 0` | **SPP Layer 2:** Lock implementation. |
| **Shadow read-only copy** | Read local copy until lock = 0, then swap | **SPP Layer 2:** Cache-coherent optimization. |
| **Queue lock** | Array of locks, pointer to last process | **SPP Layer 2:** Scalable lock. |
| **Enter queue** | Small critical section; busy-wait for previous | **SPP Layer 2:** Queue lock enter. |
| **Release** | Change value of Lock[myid] | **SPP Layer 2:** Queue lock release. |

### 11.1.6 Procedure Calls

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Own stack frame** | Each worker needs one | **SPP Layer 2:** Procedure call handling. |
| **Cactus stack** | Worker's stack logically connected | **SPP Layer 2:** Stack management. |
| **Nested parallel constructs** | Test if already in parallel | **SPP Layer 2:** Recursive parallelism. |
| **Two versions** | Parallel and sequential | **SPP Layer 2:** Code generation. |
| **Data dependences** | Satisfy by caller | **SPP Layer 2:** Synchronization. |

### 11.1.7 Strength Reduction

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Generating concurrent code** | Prevents some scalar optimizations | **SPP Layer 2:** Optimization tradeoff. |
| **Self-scheduled** | Can't determine next iteration | **SPP Layer 2:** Strength reduction limitation. |
| **Prescheduling or chunk** | Recover strength reduction | **SPP Layer 2:** Optimization recovery. |

---

## 11.2 Concurrency from Sequential Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **No dependence relations** | Iterations parallel | **UDDP:** `Claim::None`. **SPP:** Parallelization. |
| **Scalars assigned** | Loop-carried output dependence | **UDDP:** Scalar dependence. |
| **Options** | Remove, synchronize, sequential | **SPP Layer 2:** Strategy selection. |

### 11.2.1 Handling Scalars

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Induction variable substitution** | Remove scalar dependence | **UDDP M3:** IV substitution. **SPP Layer 2:** Optimization. |
| **Privatization** | Private variable per worker | **SPP Layer 2:** Scalar privatization. |
| **Scalar expansion** | Expand to array | **SPP Layer 2:** Scalar expansion. |
| **Reductions** | Pattern recognition | **UDDP:** Reduction detection. **SPP Layer 2:** Parallel reduction. |
| **Simple reductions** | Sum, product, max, min | **UDDP:** Reduction classification. |
| **Coupled reductions** | Span more than one statement | **UDDP:** Complex reduction. |
| **Array contraction** | Replace array by privatized scalars | **SPP Layer 2:** Array contraction. |

### 11.2.2 Loop Carried Dependence Relations

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Synchronization** | Ordered critical section | **SPP Layer 2:** Synchronization. |
| **Reduce critical section** | Minimize size | **SPP Layer 2:** Optimization. |
| **Alignment** | Change loop-carried to loop-independent | **SPP Layer 2:** Alignment. |
| **Alignment graph** | One node per statement, two edges per dependence | **SPP Layer 2:** Alignment algorithm. |
| **Cycle distance zero** | Alignment removes loop-carried | **SPP Layer 2:** Alignment condition. |
| **First-order linear recurrence** | xi=aixi-1+ci | **UDDP M8:** Recurrence detection. **SPP Layer 2:** Parallel recurrence. |
| **Substitution** | xi=ai(ai-1xi-2+ci-1)+ci | **UDDP M8:** Recurrence substitution. |
| **Block scheduling** | Worker computes local coefficients | **SPP Layer 2:** Recurrence blocking. |
| **Final parallel computation** | xi=bixlo(p)-1+di | **SPP Layer 2:** Recurrence completion. |
| **Fission** | Split across lexically forward dependences | **SPP Layer 2:** Fission. |
| **No lexically backward** | No cycles → fission succeeds | **SPP Layer 2:** Fission condition. |
| **Scalar privatization** | Complication | **SPP Layer 2:** Fission with scalars. |
| **Strip-mining** | d>1, strip size sd | **SPP Layer 2:** Strip-mining. |
| **Distance divisible** | $(d \operatorname{div} s, 0)$ | **SPP Layer 2:** Strip-mining distance. |
| **Distance not divisible** | $(d \operatorname{div} s, d \bmod s)$ and $(d \operatorname{div} s + 1, (d \bmod s) - s)$ | **SPP Layer 2:** Strip-mining distances. |
| **Only loop-carried** | Element loop parallel | **SPP Layer 2:** Parallel element loop. |
| **Several dependences** | Strip size = minimum distance | **SPP Layer 2:** Strip-mining parameter. |
| **Striping** | Interchange element and strip loops | **SPP Layer 2:** Striping. |
| **Element distance zero** | Element loop parallel | **SPP Layer 2:** Striping condition. |
| **Multiple version loops** | Sequential and parallel versions | **SPP Layer 2:** Versioning. |
| **While loops** | Schedule one at a time | **SPP Layer 2:** Noncountable loops. |

---

## 11.3 Concurrency from Parallel Loops

| Construct | Definition | UDDP/SPP Usage |
|---|---|---|
| **Dopar** | Anti-dependences only | **UDDP:** Dependence type. **SPP Layer 2:** Parallelization. |
| **Fetch sources ahead** | Save in temporaries | **SPP Layer 2:** Dopar compilation. |
| **Barrier** | Between fetch and execute | **SPP Layer 2:** Synchronization. |
| **Forall** | RHS into temp, then LHS | **SPP Layer 2:** Forall compilation. |
| **Barrier between** | Two parallel loops | **SPP Layer 2:** Synchronization. |
| **Dosingle** | Flow dependences | **UDDP:** Dependence type. **SPP Layer 2:** Dosingle compilation. |
| **Dependence cycles** | Can carry | **UDDP:** Cycle detection. |
| **Distance positive/negative** | Possible | **UDDP:** Distance computation. |
| **No legal ordering** | Some single-assignment programs | **UDDP:** Undecidable. |

---

## 11.4 Nested Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Inner loop parallel** | Smaller granularity, less imbalance | **SPP Layer 2:** Parallelization choice. |
| **Outer loop parallel** | Larger granularity, amortize overhead | **SPP Layer 2:** Parallelization choice. |
| **No dependences** | No synchronization except scheduling | **SPP Layer 2:** Parallelization. |
| **Bit-matrices** | Sync1, Sync2 for dependences | **SPP Layer 2:** Fine-grained sync. |
| **Eliminate sync** | If same iterations synchronized | **SPP Layer 2:** Sync optimization. |
| **ab** | Transitivity | **SPP Layer 2:** Sync analysis. |
| **Interchanging** | Move parallel loop inner/outer | **SPP Layer 2:** Interchange. |
| **All distances zero** | Interchange to any level | **SPP Layer 2:** Interchange condition. |
| **Nonzero distance** | Move loop to outer level | **SPP Layer 2:** Interchange for parallelism. |
| **Linear transformation** | T[1]dk>0 | **SPP Layer 2:** Transformation. |
| **Minimize trip count** | T[1]c+1 | **SPP Layer 2:** Transformation optimization. |
| **Index set partitioning** | Dependence matrix D; HNF H=DU | **SPP Layer 2:** Partitioning. |
| **Zero diagonal** | Interchange to bottom | **SPP Layer 2:** HNF structure. |
| **hjj>0** | hjj independent partitions | **SPP Layer 2:** Partition count. |
| **hjj=0** | Completely independent | **SPP Layer 2:** Full parallelism. |
| **Parallel loops** | `doall P_j = 0 to h_jj-1` | **SPP Layer 2:** Generated code. |
| **Serial loops** | I1=P1+h11q1 | **SPP Layer 2:** Generated code. |
| **Tiling** | Each tile atomic | **SPP Layer 2:** Tiling. |
| **Fully permutable** | No dependence cycles between tiles | **SPP Layer 2:** Tiling condition. |
| **Performance** | Memory stride, interference, chunk, sync, granularity | **SPP Layer 3:** Performance tuning. |

---

## 11.5 Roundoff Error

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Same answers** | Except reassociation | **SPP Layer 5:** Numerical semantics. |
| **Reassociation** | Reduce critical section | **SPP Layer 2:** Optimization. |
| **Different roundoff** | Self-scheduled may vary | **SPP Layer 5:** Nondeterminism. |
| **More operations** | Advanced recurrence | **SPP Layer 5:** Numerical error. |
| **Compiler switch** | Disable reassociation | **SPP Layer 5:** User control. |
| **Sequential or ordered** | Reductions must be | **SPP Layer 5:** Numerical constraint. |

---

## 11.6 Exceptions and Debuggers

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Parallel exception** | Halting one worker may require others | **SPP Layer 2:** Exception handling. |
| **Debugging optimized** | Difficult | **SPP Layer 2:** Debug support. |
| **Inform debugger** | Values/iterations | **SPP Layer 2:** Debug information. |

---

## 11.7 In the Pit (Implementation Notes)

| Topic | Usage |
|---|---|
| **Scheduling policies** | **SPP Layer 2:** Decision to use parallelism. |
| **Multiprocessor OS** | **SPP Layer 2:** Competitive scheduling. |
| **Memory word tags** | **SPP Layer 2:** Synchronization. |
| **Roundoff differences** | **SPP Layer 5:** Surprising results. |
| **Critical section** | **SPP Layer 2:** Can be faster than sequential. |
| **Roundoff switch** | **SPP Layer 5:** Cannot eliminate roundoff. |

---

## 11.8 Further Reading

| Reference | Contribution |
|---|---|
| **Padua et al. (1980)** | Early parallelizing compilers |
| **Banerjee (1993, 1994)** | Parallelization and loop restructuring |
| **Polychronopoulos (1988b)** | Parallelization techniques |
| **Polychronopoulos & Kuck (1987)** | Guided self-scheduling |
| **Hummel et al. (1992)** | Factoring scheduling |
| **Tzen & Ni (1993)** | Trapezoidal scheduling |
| **Polychronopoulos (1988a)** | Cycle-shrinking optimization |
| **Peir & Cytron (1989)** | Partition loop based on unimodular transformations |
| **D'Hollander (1992)** | Unimodular transformations on dependence distance matrix |
| **Wolf & Lam (1991a)** | Unimodular transformations, coarse/fine grain |
| **Hill & Larus (1990)** | Caches in multiprocessor systems |
| **Jalby & Meier (1986)** | Compiling nested loops into tiles |
| **Hatcher & Quinn (1991)** | Parallel code from Dataparallel C |
| **Bik & Wijshoff (1993)** | Compiling sparse matrix operations |

---

# Chapter 12 — Vector Analysis

## 12.0 Core Concepts

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Vector instructions** | Two vector operands, or vector + scalar | **SPP Layer 5:** SIMD. |
| **Vector register** | Starting position, stride 1, length in VL | **SPP Layer 5:** SIMD model. |
| **VL = 0** | No-op | **SPP Layer 5:** Edge case. |
| **Memory vector** | Starting position and stride in scalar registers | **SPP Layer 5:** SIMD memory access. |
| **Strip-mining** | Trip count unknown | **SPP Layer 5:** Vectorization. |
| **Conditionals** | Vector compare generates bit-vector | **SPP Layer 5:** SIMD conditionals. |
| **Mask register** | Cray | **SPP Layer 5:** SIMD conditionals. |
| **Bit-vector registers** | Fujitsu | **SPP Layer 5:** SIMD conditionals. |
| **Bit vector density** | Ratio of 1-bits | **SPP Layer 5:** SIMD efficiency. |
| **Conditional vector instructions** | Additional bit vector operand | **SPP Layer 5:** SIMD conditionals. |
| **Reductions** | Intrinsic or accumulate vector register | **SPP Layer 5:** SIMD reductions. |
| **Four steps** | Store, load partial sums, accumulate, scalar | **SPP Layer 5:** SIMD reduction. |
| **Compressed iota** | Index vector of 1-bits in mask | **SPP Layer 5:** SIMD indexed load/store. |

---

## 12.1 Vector Code

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Vector instructions** | Two vector operands | **SPP Layer 5:** SIMD. |
| **Vector register** | Starting position, stride 1, VL | **SPP Layer 5:** SIMD model. |
| **Memory vector** | Starting position and stride in scalar registers | **SPP Layer 5:** SIMD memory access. |
| **Strip-mining** | Trip count unknown | **SPP Layer 5:** Vectorization. |
| **Conditionals** | Vector compare generates bit-vector | **SPP Layer 5:** SIMD conditionals. |
| **Mask register** | Cray | **SPP Layer 5:** SIMD conditionals. |
| **Bit-vector registers** | Fujitsu | **SPP Layer 5:** SIMD conditionals. |
| **Bit vector density** | Ratio of 1-bits | **SPP Layer 5:** SIMD efficiency. |
| **Conditional vector instructions** | Additional bit vector operand | **SPP Layer 5:** SIMD conditionals. |
| **Reductions** | Intrinsic or accumulate vector register | **SPP Layer 5:** SIMD reductions. |
| **Four steps** | Store, load partial sums, accumulate, scalar | **SPP Layer 5:** SIMD reduction. |
| **Compressed iota** | Index vector of 1-bits in mask | **SPP Layer 5:** SIMD indexed load/store. |

---

## 12.2 Vector Code from Sequential Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Vector execution** | Preserves lexically forward dependences | **UDDP:** Dependence condition. **SPP Layer 5:** Vectorization. |
| **Acyclic dependence graph** | Only loops with this can be vectorized | **UDDP:** Acyclicity check. **SPP Layer 5:** Vectorization condition. |
| **Topological sort** | Remove lexically backward dependences | **UDDP:** Reordering. **SPP Layer 5:** Vectorization. |
| **Scalars** | Must be handled | **SPP Layer 5:** Scalar handling. |
| **Loop-carried** | Not a problem | **SPP Layer 5:** Vectorization. |
| **Dependence cycles** | Must be handled | **SPP Layer 5:** Cycle handling. |

### 12.2.1 Handling Scalars

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Induction variable cycles** | Remove by substitution | **UDDP M3:** IV substitution. **SPP Layer 5:** Vectorization. |
| **Scalar temporaries** | Remove by scalar expansion | **SPP Layer 5:** Scalar expansion. |
| **Scalar expansion after strip-mining** | Size of vector register | **SPP Layer 5:** Scalar expansion. |
| **No memory allocated** | Expanded scalar in vector register | **SPP Layer 5:** Register allocation. |
| **Last value assignments** | Added | **SPP Layer 5:** Post-loop value. |
| **Scalar reductions** | Recognized as for concurrent loops | **UDDP:** Reduction detection. **SPP Layer 5:** Reduction vectorization. |
| **Coupled reductions** | Accumulate strips of partial results | **SPP Layer 5:** Complex reduction. |

### 12.2.2 Dependence Cycles

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Scalar assignment** | Creates dependence cycle | **UDDP:** Cycle detection. |
| **Methods above** | Satisfy most | **SPP Layer 5:** Cycle handling. |
| **Leave sequential** | Or partial vectorization | **SPP Layer 5:** Conservative. |
| **Loop fission with temporaries** | Split cycle | **SPP Layer 5:** Fission. |
| **Dynamically allocated vector** | If split across two loops | **SPP Layer 5:** Temporaries. |
| **Strip-mining before fission** | Avoid dynamic allocation | **SPP Layer 5:** Strip-mining. |
| **Loop fission with conditionals** | Condition saved in temporary vector | **SPP Layer 5:** Conditional fission. |
| **Control dependence replaced** | By data dependence | **UDDP:** Dependence conversion. |
| **Boolean recurrence** | Compute true/false values | **SPP Layer 5:** Conditional vectorization. |
| **Vectorization despite cycles** | Anti-dependence cycles, multiple statements, partial expression, strip-mining, index set splitting, run-time testing, ignoring | **SPP Layer 5:** Cycle handling. |
| **Anti-dependence cycles** | Self anti-dependence cannot form cycle | **UDDP:** Dependence analysis. |
| **Operation-level graph** | Resolves false cycles | **UDDP:** Fine-grained analysis. |
| **Strip-mining** | Strip size s satisfies dependences s | **SPP Layer 5:** Strip-mining. |
| **Index set splitting** | Split where direction changes | **SPP Layer 5:** Index-set splitting. |
| **Run-time dependence testing** | Two versions | **SPP Layer 5:** Versioning. |
| **Ignoring output dependence cycles** | If stored value loop-invariant and identical | **UDDP:** Dependence analysis. |

---

## 12.3 Vector Code from Forall Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Forall loop** | Array assignments | **SPP Layer 5:** Vectorization. |
| **Strip-mining** | May not be sufficient | **SPP Layer 5:** Strip-mining. |
| **Scalarized** | Must know if possible | **SPP Layer 5:** Scalarization check. |
| **Temporary arrays** | Store intermediate values | **SPP Layer 5:** Temporaries. |
| **Loop reversal** | Can make forall scalarizable | **SPP Layer 2:** Reversal. |

---

## 12.4 Nested Loops

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Early compilers** | Only innermost loop | **SPP Layer 5:** Historical limitation. |
| **Nested vector loops** | Converting to array assignments | **SPP Layer 5:** Vectorization. |
| **Acyclic** | All loops vectorized | **UDDP:** Acyclicity check. |
| **Loop interchanging** | Move dependence-carrying loop outer | **SPP Layer 2:** Interchange. |
| **Best loop** | Based on stride, startup, limits | **SPP Layer 3:** Cost model. |
| **Multiple versions** | Depending on run-time conditions | **SPP Layer 4:** Versioning. |
| **Outer loop vectorization** | Legal if all dependences satisfied | **UDDP:** Legality check. |
| **Equivalent to strip-mining** | Outer loop + interchange | **SPP Layer 5:** Vectorization. |
| **Supervector performance** | Improves memory/arithmetic ratio | **SPP Layer 3:** Performance. |
| **Loop collapsing** | Increase vector length | **SPP Layer 2:** Collapsing. |

---

## 12.5 Roundoff Error, Exceptions, and Debuggers

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Reductions** | Change roundoff error accumulation | **SPP Layer 5:** Numerical semantics. |
| **Exceptions** | May be raised in different order | **SPP Layer 5:** Exception handling. |
| **Few compilers preserve** | Exception behavior | **SPP Layer 5:** Conservative. |
| **Debugging easier** | Single processor | **SPP Layer 5:** Debug support. |

---

## 12.6 Multivector Computers

| Concept | Definition | UDDP/SPP Usage |
|---|---|---|
| **Multiprocessor + vector** | Two levels of parallelism | **SPP Layer 5:** Hierarchical parallelism. |
| **Outer concurrent loop** | Across processors | **SPP Layer 2:** Parallelization. |
| **Inner vector loop** | Within processor | **SPP Layer 5:** Vectorization. |
| **Single loop** | Strip-mine, run strip loop concurrently | **SPP Layer 2 + 5:** Combined. |

---

## 12.7 In the Pit (Implementation Notes)

| Topic | Usage |
|---|---|
| **Carry-around variable** | **SPP Layer 5:** T=B[0]; loop uses S=B[I], C[I]=(S+T)/2, T=S. Replace S and T by references to B; vectorize. Peel first iteration if T not B(I-1). |
| **Loop fission** | **SPP Layer 5:** Minimize sequential loops. |
| **Node splitting** | **SPP Layer 5:** Fine granularity dependence graph. |
| **Cyclic reduction** | **SPP Layer 5:** Execute linear recurrences in vector mode. |
| **Early Cray** | **SPP Layer 5:** No conditional vector operations; compute dense, vector merge. |
| **TI-ASC** | **SPP Layer 5:** Complex instruction set for three nested vector loops. |

---

## 12.8 Further Reading

| Reference | Contribution |
|---|---|
| **Schneck (1972)** | Earliest automatic parallelism |
| **Kuck et al. (1980)** | Parafrase project |
| **Allen & Kennedy (1987)** | PFC project, data dependence theory |
| **Allen & Kennedy (1992)** | Strip-mining, vector register locality |
| **Erbacci et al. (1989)** | Stride optimization on cached vector machine |
| **Tsuda & Kunieda (1990)** | Vectorizing compiler with indirect addressing |
| **Banerjee & Gajski (1984)** | Vectorizing loops with conditionals in dependence cycle |

---

# Complete Algorithm Index (Chapters 10–12)

| Algorithm | Chapter | Section | UDDP/SPP Component |
|---|---|---|---|
| **Guided self-scheduling** | 11 | 11.1.1 | SPP Layer 2 (scheduling) |
| **Factoring** | 11 | 11.1.1 | SPP Layer 2 (scheduling) |
| **Master/slave prescheduled** | 11 | 11.1.1 | SPP Layer 2 (code generation) |
| **Master/slave self-scheduled** | 11 | 11.1.1 | SPP Layer 2 (code generation) |
| **Generator queue** | 11 | 11.1.1 | SPP Layer 2 (dynamic scheduling) |
| **Swap lock** | 11 | 11.1.5 | SPP Layer 2 (synchronization) |
| **Shadow read-only copy** | 11 | 11.1.5 | SPP Layer 2 (synchronization) |
| **Queue lock** | 11 | 11.1.5 | SPP Layer 2 (synchronization) |
| **Alignment** | 11 | 11.2.2 | SPP Layer 2 (alignment) |
| **First-order linear recurrence** | 11 | 11.2.2 | UDDP M8, SPP Layer 2 |
| **Substitution** | 11 | 11.2.2 | UDDP M8, SPP Layer 2 |
| **Block scheduling** | 11 | 11.2.2 | SPP Layer 2 |
| **Fission** | 11 | 11.2.2 | SPP Layer 2 |
| **Strip-mining** | 11 | 11.2.2 | SPP Layer 2 |
| **Striping** | 11 | 11.2.2 | SPP Layer 2 |
| **Multiple version loops** | 11 | 11.2.2 | SPP Layer 2 |
| **Index set partitioning** | 11 | 11.4 | SPP Layer 2 |
| **Linear transformation** | 11 | 11.4 | SPP Layer 2 |
| **Tiling** | 11 | 11.4 | SPP Layer 2 |
| **Loop fission with temporaries** | 12 | 12.2.2 | SPP Layer 5 |
| **Loop fission with conditionals** | 12 | 12.2.2 | SPP Layer 5 |
| **Boolean recurrence** | 12 | 12.2.2 | SPP Layer 5 |
| **Node splitting** | 12 | 12.7 | SPP Layer 5 |
| **Cyclic reduction** | 12 | 12.7 | SPP Layer 5 |

---

# Complete Formula Index (Chapters 10–12)

| Formula | Chapter | Section | UDDP/SPP Usage |
|---|---|---|---|
| Ad=0 | 10 | 10.1.2 | Self-temporal reuse |
| ASd=0 | 10 | 10.1.2 | Spatial reuse |
| Nk if ekA | 10 | 10.1.2 | Temporal reuse factor |
| (1,l/ask) | 10 | 10.1.2 | Spatial reuse factor |
| Rk* | 10 | 10.1.2 | Cumulative self-reuse |
| Fk*=li=knNiRi | 10 | 10.1.2 | Data footprint |
| $R_k^D = \dim(\operatorname{span}(e_k, \ldots, e_n) \cap \ker A)$ | 10 | 10.1.2 | Degree of reuse |
| Ad=c1-c2 | 10 | 10.2 | Group temporal reuse |
| Nk/dk | 10 | 10.2 | Group temporal reuse factor |
| ASd=cS,1-cS,2 | 10 | 10.2 | Group spatial reuse |
| $\dim(\operatorname{span}(e_k,\ldots,e_n) \cap \{d + \ker A\})$ | 10 | 10.2.3 | Degree of reuse |
| R0=N, $C_i = \lceil R_{i-1}/P \rceil$, Ri=Ri-1-Ci | 11 | 11.1.1 | Guided self-scheduling |
| $C_i = \lceil R_{i-1}/(2P) \rceil$ | 11 | 11.1.1 | Guided variation |
| R0=N, $C_i = \lceil R_{i-1}/(fP) \rceil$, Ri=Ri-1-PCi | 11 | 11.1.1 | Factoring |
| xi=aixi-1+ci | 11 | 11.2.2 | First-order linear recurrence |
| xi=(aiai-1)xi-2+(aici-1+ci) | 11 | 11.2.2 | Recurrence substitution |
| xi=bixlo(p)-1+di | 11 | 11.2.2 | Final parallel computation |
| $(d \operatorname{div} s, 0)$ | 11 | 11.2.2 | Strip-mining distance |
| $(d \operatorname{div} s, d \bmod s)$ | 11 | 11.2.2 | Strip-mining distance |
| $(d \operatorname{div} s + 1, (d \bmod s) - s)$ | 11 | 11.2.2 | Strip-mining distance |
| T[1]dk>0 | 11 | 11.4 | Linear transformation |
| T[1]c+1 | 11 | 11.4 | Transformed trip count |
| H=DU | 11 | 11.4 | HNF of dependence matrix |
| I1=P1+h11q1 | 11 | 11.4 | Serial loop |
| Ij=Pj+k=1j-1hjkqk+hjjqj | 11 | 11.4 | Serial loop |
| $\lceil (LO_1 - P_1)/h_{11} \rceil \le q_1 \le \lceil (HI_1 - P_1)/h_{11} \rceil$ | 11 | 11.4 | Loop bounds |
| $\lceil \frac{LO_j - P_j - \sum_{k=1}^{j-1} h_{jk} q_k}{h_{jj}} \rceil \le q_j \le \lceil \frac{HI_j - P_j - \sum_{k=1}^{j-1} h_{jk} q_k}{h_{jj}} \rceil$ | 11 | 11.4 | Loop bounds |

---

# Summary: What Chapters 10–12 Give UDDP + SPP

| Chapter | Section | UDDP Component | SPP Component |
|---|---|---|---|
| **10** | 10.0 Core concepts | — | Layer 3 (hardware model) |
| **10** | 10.1 Single reference | — | Layer 3 (loop reordering, tiling, reuse) |
| **10** | 10.2 Multiple references | — | Layer 3 (group reuse) |
| **10** | 10.3 General tiling | — | Layer 2 (tiling) |
| **10** | 10.4 Fission/fusion | — | Layer 2 (fission/fusion) |
| **11** | 11.1 Concurrent loops | — | Layer 2 (scheduling, sync) |
| **11** | 11.2 Sequential loops | M3, M8 (dependences) | Layer 2 (parallelization) |
| **11** | 11.3 Parallel loops | — | Layer 2 (parallelization) |
| **11** | 11.4 Nested loops | — | Layer 2 (transformations) |
| **11** | 11.5 Roundoff | — | Layer 5 (numerical semantics) |
| **11** | 11.6 Exceptions | — | Layer 2 (exception handling) |
| **12** | 12.1 Vector code | — | Layer 5 (SIMD) |
| **12** | 12.2 Sequential loops | M0–M10 (dependences) | Layer 5 (vectorization) |
| **12** | 12.3 Forall loops | — | Layer 5 (vectorization) |
| **12** | 12.4 Nested loops | — | Layer 5 (vectorization) |
| **12** | 12.5 Roundoff | — | Layer 5 (numerical semantics) |
| **12** | 12.6 Multivector | — | Layer 5 (hierarchical) |

---

# What Chapters 10–12 Unlock for UDDP

| UDDP Level | Chapter 10 Provides | Chapter 11 Provides | Chapter 12 Provides |
|---|---|---|---|
| **M0** | — | — | — |
| **M1** | — | — | — |
| **M2a** | — | — | — |
| **M2b** | — | — | — |
| **M3** | — | — | IV substitution |
| **M3.5** | — | — | — |
| **M4** | — | — | — |
| **M5** | — | — | — |
| **M6** | — | — | — |
| **M7** | — | — | — |
| **M8** | — | Recurrence substitution | — |
| **M9** | — | — | — |
| **M10** | — | — | — |

---

# What Chapters 10–12 Unlock for SPP

| SPP Layer | Chapter 10 Provides | Chapter 11 Provides | Chapter 12 Provides |
|---|---|---|---|
| **Layer 2** | Tiling, fission/fusion | Scheduling, sync, alignment, strip-mining, transformations | Fission, reversal |
| **Layer 3** | Reuse distance, footprint, loop ordering | — | — |
| **Layer 4** | — | — | — |
| **Layer 5** | Prefetch, cache bypass | Roundoff, exceptions | SIMD, conditionals, reductions, vectorization |

---

# The Complete Book (Chapters 4–12) — Final Summary

| Chapter | UDDP Component | SPP Component |
|---|---|---|
| **Ch. 4** | Mathematical engine (gcd, HNF, FME, dark shadows) | Transformation engine (unimodular, matrix inverse, non-unimodular) |
| **Ch. 5** | Dependence vocabulary (distance, direction, level) | Transformation legality foundation |
| **Ch. 6** | Scalar value flow (FUD chains, IVs, constant prop) | SSA-based optimization |
| **Ch. 9** | — | Transformation catalog (fusion, fission, interchange, skewing, tiling) |
| **Ch. 10** | — | Locality (reuse distance, footprint, loop ordering) |
| **Ch. 11** | Recurrence detection | Concurrency (scheduling, sync, parallelization) |
| **Ch. 12** | — | Vectorization (SIMD, conditionals, reductions) |

---

# The One-Sentence Summary

> **Chapters 10–12 complete the SPP stack: Chapter 10 gives SPP Layer 3 its locality engine (reuse distance, footprint, loop ordering, tiling, fission/fusion for locality); Chapter 11 gives SPP Layer 2 its concurrency engine (scheduling, synchronization, alignment, strip-mining, linear transformations, index set partitioning); Chapter 12 gives SPP Layer 5 its vectorization engine (SIMD code generation, conditionals, reductions, cycle-breaking, nested loop vectorization). Together with Chapters 4, 5, 6, and 9, these chapters provide the complete mathematical and algorithmic foundation for UDDP M0–M10 and SPP Layers 2–5.**
Added. Here's the final phase layout with Ch. 7 and Ch. 8 folded in.

## What the two new chapters add

**Chapter 7 — most is already in Phase 2.** 7.1 (dependence system), 7.2 (single equation solvers), 7.3 (column echelon + FM) are all Phase 2's content. Two new pieces:

| § | What | New? |
|---|---|---|
| 7.1–7.3 | Dependence system, solvers, general solver | ✅ already Phase 2 |
| 7.5 | Storage association (linearization, equivalence, common blocks, allocatables) | **NEW** |
| 7.6 | Run-time dependence testing (`A[I] = A[I-K] + B[I]`, emit runtime check) | **NEW — new architecture** |

**Chapter 8 — all new.**

| § | What | Where it goes |
|---|---|---|
| 8.1 | Array region analysis (MayUse/MayMod/Kill, triplets) | New phase |
| 8.2 | Points-to (relations, dope vectors, pointer arithmetic, allocation) | New phase |
| 8.3 | I/O dependence | Small — Phase 5 |
| 8.4 | Procedure call classification (pure functions) | Small — Phase 5 |
| 8.5 | Interprocedural (call graph, MOD, constant prop) | New phase |

## The final nine phases

```
Phase 1  Certificate foundations
             │
             ▼
Phase 2  Math engine (Ch. 4 + Ch. 7.1–7.3)
             │
             ▼
Phase 3  FUD chains (Ch. 6.1–6.3)
             │
             ▼
Phase 4  Scalar analysis (Ch. 6.4–6.7)
             │
             ├─────────────────────┐
             ▼                     ▼
Phase 5  Subsystems             Phase 6  Array regions
         (Ch. 7.5, 7.6,                (Ch. 8.1)
          8.3, 8.4,
          symbolic chase,
          dynamic bounds,
          cubic+, reductions,
          alignment)
             │                     │
             └─────────┬───────────┘
                       ▼
Phase 7  Points-to / aliasing (Ch. 8.2)
                       │
                       ▼
Phase 8  Interprocedural (Ch. 8.5)
                       │
                       ▼
Phase 9  Frontend extensions
```

**Why this order is forced:**

- Ch. 7.5 storage association → prerequisite for Ch. 8.2 (linearization must exist before points-to can reason about arrays)
- Ch. 8.1 array regions → prerequisite for Ch. 8.2 and Ch. 8.5 (summaries need region composition)
- Ch. 8.2 points-to → prerequisite for Ch. 8.5 (call sites need alias info)
- Ch. 8.3 / 8.4 effect summaries → prerequisite for Ch. 8.5 (call graph needs pure-function classification)
- Ch. 8.5 interprocedural → needs all of 7.5, 8.1, 8.2, 8.3, 8.4

Phases 5 and 6 can run **in parallel** — they're independent. Phase 5 is the "subsystem" phase; Phase 6 is the "region/summary" phase. Both feed Phase 7.

---

## Phase-by-phase, with the new content

### Phase 1 — Certificate foundations (1.5 weeks, 1,200 lines)
No change from before. New `Claim` variants (Direction, Level, Parametric, CrossLoop), new `Witness` carriers. **Everything downstream emits into this format.**

### Phase 2 — Math engine (10.5 weeks, 3,600 lines)
Now explicitly includes Ch. 7.1–7.3. HNF, inverse, dark shadows, full `Extreme`, gcd, column echelon, Fourier-Motzkin, integer tightening. Wired into M0, M1, M4, M6, M9.

### Phase 3 — FUD chains (8.5 weeks, 2,950 lines)
No change. CFG, dominators, dominance frontier, φ-placement, chain construction, array chains, FRUD chains.

### Phase 4 — Scalar analysis (11 weeks, 3,750 lines)
No change. `Algorithm Induct`, IV algebra, trip counts, constant propagation, `ScalarDD`. M3/M5/M8 rewired.

### Phase 5 — Subsystems (12 weeks, 4,500 lines)

Now includes **Ch. 7.5 storage association, Ch. 7.6 run-time testing, Ch. 8.3 I/O, Ch. 8.4 pure-function classification** in addition to the original subsystem list.

| # | Item | Lines |
|---|---|---|
| 5.1 | Symbolic pointer chase | 600 |
| 5.2 | Dynamic bounds (M12 → symbolic) | 800 |
| 5.3 | Cubic+ (M6c → degree ≥ 5) | 300 |
| 5.4 | Reduction detection | 500 |
| 5.5 | Alignment | 400 |
| 5.6 | Cross-loop distance | 150 |
| 5.7 | Loop-kind classification | 200 |
| 5.8 | **Ch. 7.5 storage association** — linearization, equivalence, common blocks | 500 |
| 5.9 | **Ch. 7.6 run-time dependence testing** — emit runtime check version | 600 |
| 5.10 | **Ch. 8.3 I/O dependence** — preserve order, effect summary | 150 |
| 5.11 | **Ch. 8.4 pure-function classification** | 200 |

**Note on 5.9:** run-time testing is architecturally different from everything else. It doesn't emit a certificate — it emits a **runtime check** (a small loop that tests independence at execution time) that, if it passes, licenses parallelization. This needs a **new output type** (`RuntimeCheckCertificate`) and a **new checker arm** (verifies the test is sound — i.e., if the test says "independent," then the accesses really are). This is a small but distinct piece of infrastructure.

### Phase 6 — Array regions (6 weeks, 2,000 lines)

**Ch. 8.1** — new phase.

| # | Item | Lines |
|---|---|---|
| 6.1 | `MayUse`, `MayMod`, `Kill` set dataflow | 400 |
| 6.2 | Simple bounds (union/intersection over `[l:u]`) | 250 |
| 6.3 | Triplet notation (`[l:u:s]`, union/intersection with strides) | 400 |
| 6.4 | Array kill analysis (self-conflict, rank test) | 300 |
| 6.5 | Region summary data structure + checker | 400 |
| 6.6 | Fuzz: verify region composition against brute force | 250 |

**Deliverable:** every array in a function has a `MayUse`/`MayMod`/`Kill` summary. This is the input to Phase 7 (points-to) and Phase 8 (interprocedural).

### Phase 7 — Points-to / aliasing (7 weeks, 4,000 lines)

**Ch. 8.2** — new phase.

| # | Item | Lines |
|---|---|---|
| 7.1 | Points-to relations `(p, q, d)` with definiteness | 700 |
| 7.2 | Assignment effects + CFG merge | 500 |
| 7.3 | Fixpoint iteration over loops | 300 |
| 7.4 | Array pointers / dope vectors | 500 |
| 7.5 | Pointer arithmetic (IV-based reconstruction) | 600 |
| 7.6 | Allocation dependence (Fortran 90 vs C semantics) | 400 |
| 7.7 | Alias certificate + checker | 500 |
| 7.8 | Fuzz: verify alias sets against brute force on synthetic programs | 500 |

**Deliverable:** `A[p.field]` and `A[q.field]` either provably alias or provably don't.

### Phase 8 — Interprocedural (10 weeks, 5,500 lines)

**Ch. 8.5** — new phase.

| # | Item | Lines |
|---|---|---|
| 8.1 | Call graph construction (triplet `⟨q, k, p⟩`) | 800 |
| 8.2 | Interprocedural MOD (binding graph, SCC) | 700 |
| 8.3 | Interprocedural constant propagation (jump functions) | 600 |
| 8.4 | Bottom-up summary computation | 800 |
| 8.5 | Top-down context refinement | 600 |
| 8.6 | Compose summaries at call sites | 500 |
| 8.7 | Interprocedural certificate + checker | 600 |
| 8.8 | Fuzz: verify summary composition against brute force | 900 |

**Deliverable:** a loop calling `helper()` no longer collapses to `MAY_DEPEND`. Summary composition gives the caller the callee's effect.

### Phase 9 — Frontend extensions (11.5 weeks, 5,350 lines)
No change. Loop kinds, control edges, dynamic bounds IR, JS→IR for closures/destructuring/for-of, object property accesses, array-of-structs, frontend fuzzing.

---

## Final tally

| Phase | Focus | Lines | Weeks |
|---|---|---|---|
| 1 | Certificate foundations | 1,200 | 1.5 |
| 2 | Math engine (Ch. 4 + 7.1–7.3) | 3,600 | 10.5 |
| 3 | FUD chains (Ch. 6.1–6.3) | 2,950 | 8.5 |
| 4 | Scalar analysis (Ch. 6.4–6.7) | 3,750 | 11 |
| 5 | Subsystems (Ch. 7.5, 7.6, 8.3, 8.4) | 4,500 | 12 |
| 6 | Array regions (Ch. 8.1) | 2,000 | 6 |
| 7 | Points-to (Ch. 8.2) | 4,000 | 7 |
| 8 | Interprocedural (Ch. 8.5) | 5,500 | 10 |
| 9 | Frontend extensions | 5,350 | 11.5 |
| **Total** | | **~32,850** | **~78 weeks** |

Multiply by 1.5–2× for reality (fuzz findings, refactors, scope creep). Realistic: **18–24 months**.

---

## What changed vs. the previous 6-phase plan

**Added:**
- Ch. 7.5 storage association (500 lines, Phase 5)
- Ch. 7.6 runtime testing (600 lines, Phase 5) — **new output type**
- Ch. 8.1 array regions (2,000 lines, new Phase 6)
- Ch. 8.2 points-to (4,000 lines, new Phase 7)
- Ch. 8.3 I/O dependence (150 lines, Phase 5)
- Ch. 8.4 pure-function classification (200 lines, Phase 5)
- Ch. 8.5 interprocedural (5,500 lines, new Phase 8)

**Total added:** ~12,950 lines, ~25 weeks.

The original 6-phase plan was ~20,500 lines / ~53 weeks. This brings it to **~32,850 lines / ~78 weeks**. Almost 60% more work.

---

## Two honest notes

**1. Ch. 7.6 run-time testing doesn't fit the certificate model.** Every other phase emits a `Certificate` that `m0-check` verifies by replay. Runtime testing emits code that runs *later*, at execution time, on real data. That's not a certificate — it's a **program transformation**. It needs its own output format, its own checker semantics ("if this runtime test passes, independence is guaranteed"), and its own fuzzing infrastructure. I flagged it in Phase 5 but it may deserve its own sub-phase or a full phase.

**2. Interprocedural is where the plan will slip the most.** Phase 8's 5,500-line estimate is optimistic. A real context-sensitive interprocedural analysis with summaries, points-to, and effect composition is what `rustc`'s borrow checker, LLVM's `-steensgaard`, and Soot's whole-program analysis took **years** to get right. The line count is small; the correctness bar is very high. Expect Phase 8 to take 2–3× the estimate.

---

## The one-paragraph summary

> **Nine phases, ~32,850 lines, ~78 weeks ideal (multiply by 1.5–2× for reality). Phase 1 = certificate foundations. Phase 2 = math engine (Ch. 4 + Ch. 7.1–7.3). Phase 3 = FUD chains. Phase 4 = scalar analysis. Phases 5 and 6 run in parallel: Phase 5 = subsystems including storage association (Ch. 7.5), runtime testing (Ch. 7.6, new output type), I/O (8.3), pure-function classification (8.4); Phase 6 = array region analysis (Ch. 8.1). Phase 7 = points-to / aliasing (Ch. 8.2). Phase 8 = interprocedural (Ch. 8.5) — the hardest phase, likely 2–3× its estimate. Phase 9 = frontend extensions. Everything before Phase 6 is single-procedure analysis; Phase 6 unlocks whole-function summaries; Phases 7–8 unlock whole-program analysis. Ch. 7.5, 7.6, 8.1, 8.2, 8.3, 8.4, 8.5 together add ~12,950 lines and ~25 weeks to the original six-phase plan. The two biggest single risks are (a) Ch. 7.6's runtime testing needing a new certificate/output model, and (b) Phase 8 interprocedural analysis being far harder than its line count suggests.**


# SPP — SPECULATIVE POLYHEDRAL PARALLELIZATION

## Complete Specification — All Five Layers, All Formulas, All Algorithms

---

# PART 0 — WHAT SPP IS

SPP parallelizes loops that cannot be parallelized statically.

It is a stack of five layers that select the cheapest mechanism capable of validating each assumption.

**The core question:**

> **"What execution configuration maximizes useful work per cycle while satisfying the proof?"**

---

# PART 1 — THE FIVE LAYERS

```
SPP
│
├── 1. Dependence / legality
│   ├── UDDP claims (consumed)
│   ├── Interprocedural alias analysis
│   ├── Dependence polyhedra
│   ├── Reduction generalization
│   ├── Recurrence decomposition
│   └── Effect / library summaries
│
├── 2. Parallelization
│   ├── Wavefront
│   ├── Loop parallelism
│   ├── Task parallelism
│   └── Hierarchical decomposition
│
├── 3. SPP-HX — Hierarchical Execution Optimizer
│   ├── Hardware model
│   ├── Learned cost model
│   ├── Bayesian search space
│   ├── Execution optimizer
│   ├── Online re-optimization
│   └── Profitability gate
│
├── 4. Speculation
│   ├── Tier 0: Static proof
│   ├── Tier 1: Runtime guards
│   ├── Tier 2: Version / range checks
│   ├── Tier 3: Chunk speculation
│   ├── Tier 4: Page versioning
│   ├── Tier 5: Software TM
│   └── Tier 6: Optimized speculation
│
└── 5. Acceleration
    ├── Portable vector IR
    ├── Prefetch
    ├── CPU / GPU partitioning
    └── GPU kernels
```

---

# LAYER 1 — DEPENDENCE ENGINE

## 1.1 What It Does

Given a loop, determine the exact dependence structure. Ten types:

| Type | Meaning | Action |
|---|---|---|
| `EXACT(d)` | Distance is exactly d | Wavefront with step d |
| `BOUNDED(d_max)` | Distance is at most d_max | Wavefront with d_max |
| `PARAMETRIC(d(n))` | Distance is a function of parameters | Specialize at runtime |
| `CONDITIONAL(guard)` | Dependence exists only if guard holds | Runtime check |
| `REDUCTION(op)` | Recognized reduction | Private partial states |
| `RECURRENCE(scan)` | Recognized recurrence | Prefix scan |
| `MAY_DEPEND` | May depend | Speculate |
| `MUST_DEPEND` | Must depend | Serial |
| `RUNTIME_CHECKABLE(test)` | Cheap runtime test decides | Version B |
| `UNKNOWN` | Genuinely undecidable | Speculate or serial |

## 1.2 Components

| # | Component | Lines |
|---|---|---|
| 1.1 | Static dependence proof (UDDP) | 4,900 |
| 1.2 | Interprocedural alias analysis | 3,500 |
| 1.3 | Dependence polyhedra | 700 |
| 1.4 | Non-affine dependence (gray zone) | 1,200 |
| 1.5 | Runtime-checkable dependence | 800 |
| 1.6 | Reduction generalization | 1,000 |
| 1.7 | Recurrence decomposition | 900 |
| 1.8 | Cross-iteration alias speculation | 800 |
| 1.9 | Memory-model awareness | 600 |
| 1.10 | Effect summaries | 500 |
| 1.11 | Library-effect summaries | 400 |

**Total: ~15,300 lines**

## 1.3 Interprocedural Alias Analysis

**The single biggest determinant of real-world coverage.**

Without this, any loop calling a non-inlined function collapses to `MAY_DEPEND` and the whole pipeline is starved.

| Property | What it means |
|---|---|
| **Interprocedural** | Analyzes across function boundaries |
| **Context-sensitive** | Different call sites get different results |
| **Field-sensitive** | Distinguishes `obj.x` from `obj.y` |
| **Summary-cached** | Function summaries reused across calls |

**Formula — Summary composition:**

```
For a call site c calling procedure p with arguments a₁, ..., aₙ:
    Summary(c) = Compose(Summary(p), Map(a₁, ..., aₙ))

For a chain of calls c₁ → c₂ → ... → cₖ:
    Summary(c₁) = Compose(Summary(c₁), ..., Compose(Summary(cₖ₋₁), Summary(cₖ)))
```

**Lines: 3,500**

## 1.4 Dependence Polyhedra

**What it is:** Multi-dimensional dependence analysis.

**Formula — Dependence polyhedron:**

```
For two accesses A[f(i)] and A[g(j)] with f, g affine:
    D = { (i, j) | f(i) = g(j), i ∈ IterationSpace(S₁), j ∈ IterationSpace(S₂) }

D is a polyhedron in ℤ^(2m) where m = number of loops.
```

**Formula — Distance polyhedron:**

```
Distance polyhedron: D_d = { d | ∃ (i, j) ∈ D: j - i = d }
```

**Lines: 700**

## 1.5 Non-Affine Dependence (Gray Zone)

**Real code has:**
- Array-of-structs
- Indirect indexing (`A[B[i]]`)
- Pointer chasing

**The gray zone tier:** range-based runtime disambiguation with symbolic bounds.

**Formula — Symbolic range:**

```
For subscript f(i) with symbolic bounds:
    range(f(i)) = [min f(i), max f(i)]

For indirect indexing A[B[i]]:
    range(A[B[i]]) = [min(B), max(B)]
    where B[i] is bounded by [min(B), max(B)]
```

**Formula — Range disjointness:**

```
Independence iff: max(range(f(i))) < min(range(g(j)))
               or: max(range(g(j))) < min(range(f(i)))
```

**Lines: 1,200**

## 1.6 Reduction Generalization

**What it is:** Arbitrary associative-commutative user-defined reductions detected via idempotence/commutativity proofs on the operation.

**Formula — Associativity:**

```
op(op(a, b), c) = op(a, op(b, c))  for all a, b, c
```

**Formula — Commutativity:**

```
op(a, b) = op(b, a)  for all a, b
```

**Formula — Idempotence:**

```
op(a, a) = a  for all a
```

**Formula — Reduction parallelization:**

```
Sequential: acc = op(acc, a[i])  for i = 0 to N-1

Parallel:
    partial[p] = neutral  for p = 0 to P-1
    for i = p to N-1 step P:
        partial[p] = op(partial[p], a[i])
    barrier
    acc = neutral
    for p = 0 to P-1:
        acc = op(acc, partial[p])
```

**Lines: 1,000**

## 1.7 Recurrence Decomposition

**What it is:** Prefix scans, parallel scans.

**Formula — First-order linear recurrence:**

```
x_i = a_i · x_{i-1} + c_i
```

**Formula — Substitution:**

```
x_i = a_i · (a_{i-1} · x_{i-2} + c_{i-1}) + c_i
    = (a_i · a_{i-1}) · x_{i-2} + (a_i · c_{i-1} + c_i)

Repeat to increase dependence distance.
```

**Formula — Block scheduling:**

```
Worker p computes iterations lo(p) to hi(p):
    local coefficients (b_i, d_i) for i ∈ [lo(p), hi(p)]
barrier
Smaller recurrence solved on b_i, d_i
Final parallel computation:
    x_i = b_i · x_{lo(p)-1} + d_i  for lo(p) ≤ i ≤ hi(p)
```

**Lines: 900**

## 1.8 Cross-Iteration Alias Speculation

**What it is:** Instrument once. Cache the dependence profile. Use PGO-style data to seed the classifier.

**Formula — Profile cache:**

```
ProfileCache[loop_id] = (dependence_profile, hit_count, miss_count)

On new loop:
    if ProfileCache contains loop_id:
        use cached profile
    else:
        cold-start with conservative approximation
```

**Lines: 800**

## 1.9 Memory-Model Awareness

**What it is:** Atomics, fences, acquire/release.

**Formula — Acquire semantics:**

```
Acquire load: subsequent reads/writes cannot be reordered before it.
```

**Formula — Release semantics:**

```
Release store: preceding reads/writes cannot be reordered after it.
```

**Formula — Sequentially consistent:**

```
SC load/store: all operations appear in a single total order.
```

**Lines: 600**

## 1.10 Effect Summaries

**Every operation carries an effect summary:**

```
READ
WRITE
ALLOCATE
THROW
BLOCK
ATOMIC
IO
UNKNOWN
```

**Formula — Effect composition:**

```
Effect(seq(s₁, s₂)) = Effect(s₁) ⊔ Effect(s₂)

where ⊔ is the effect join:
    READ ⊔ READ = READ
    READ ⊔ WRITE = READ_WRITE
    WRITE ⊔ WRITE = WRITE
    ANY ⊔ UNKNOWN = UNKNOWN
```

**Lines: 500**

## 1.11 Library-Effect Summaries

```
memcpy:       READ src, WRITE dst, NO_THROW, NO_GLOBAL_SIDE_EFFECT
network_send: IO, UNKNOWN_EXTERNAL_STATE, BLOCKING
malloc:       ALLOCATE, NO_THROW (usually)
free:         DEALLOCATE, NO_THROW
```

**Lines: 400**

---

# LAYER 2 — PARALLEL VERSION GENERATION

## 2.1 What It Does

Every loop generates a **lattice of versions**, not a fixed A/B/C/D. Versions are parameterized by which assumptions are checked.

## 2.2 The Version Lattice

```
Version = (assumption_set, guard_set, dispatch_cost)
```

| Version | Assumptions checked | When used |
|---|---|---|
| A | All (static) | Always fast |
| B | Runtime checks | When static fails |
| C | Speculative | When checks fail |
| D | Serial | Fallback |

**The compiler picks the Pareto-optimal set at compile time based on expected input distributions.**

**Formula — Pareto optimality:**

```
Version v is Pareto-optimal iff:
    ¬∃ v': (cost(v') ≤ cost(v)) ∧ (coverage(v') ≥ coverage(v))
             with at least one strict inequality
```

## 2.3 Cross-Version Code Sharing

Dedupe the hot inner loop body across A–D. Only the guard/dispatch shell differs.

**Formula — Code sharing benefit:**

```
Without dedup: code_size = Σ_i body_size(v_i)
With dedup:    code_size = body_size(shared) + Σ_i guard_size(v_i)

Savings: Σ_i body_size(v_i) - body_size(shared)
```

**Why:** Instruction cache pressure. Code size. Without dedup, 4 versions = 4x code.

## 2.4 Components

| # | Component | Lines |
|---|---|---|
| 2.1 | Version lattice generator | 600 |
| 2.2 | Pareto selector | 500 |
| 2.3 | Cross-version dedup | 700 |
| 2.4 | Runtime dispatcher | 300 |

**Total: ~2,100 lines**

## 2.5 The Transformations — Full Catalog

### 2.5.1 Statement Reordering

**What it does:** Reorders statements (or loops) at any granularity.

**Legality condition:** Any topological sort of the dependence graph (including both data and control dependences) is a legal ordering.

**Dependence modification:** None — the dependence graph is preserved; only the linear order changes.

**Formula — Topological sort:**

```
Given dependence graph G = (V, E):
    Linear order L is legal iff:
        ∀ (u, v) ∈ E: position_L(u) < position_L(v)
```

---

### 2.5.2 Unswitching

**What it does:** Removes a loop-invariant conditional from a loop.

**Transformation:**
```
loop
  statements
  if test then then-part else else-part
  more statements
endloop
```
becomes:
```
if test then
  loop
    statements
    then-part
    more statements
  endloop
else
  loop
    statements
    else-part
    more statements
  endloop
endif
```

**Legality condition:** The conditional test must be **completely independent of the loop**.

**Dependence modification:** None — the dependence relations are preserved.

**Formula — Loop invariance:**

```
test is loop-invariant iff:
    ∀ i, j ∈ LoopDomain: test(i) = test(j)
```

---

### 2.5.3 Loop Peeling

**What it does:** Removes the first (or last) iteration(s) of the loop into separate code.

**Transformation:**
```
compute tc
for i = 0 to tc-1 do
  body
endfor
```
becomes:
```
compute tc
if tc > 0 then
  i = 0
  body
  for i = 1 to tc-1 do
    body
  endfor
endif
```

**Legality condition:** Always legal; may require a zero-trip test if the trip count may be zero.

**Dependence modification:** None — the iterations are the same, just separated.

---

### 2.5.4 Index Set Splitting

**What it does:** Divides the index set of a loop into two portions, replicating the body.

**Transformation:**
```
compute tc
for i = 0 to tc-1 do
  body
endfor
```
becomes:
```
compute tc
for i = 0 to s-1 do
  body
endfor
for i = s to tc-1 do
  body
endfor
```

**Legality condition:** Always legal; the union of the split index sets must equal the original.

**Formula — Set partition:**

```
[0, tc-1] = [0, s-1] ∪ [s, tc-1]
[0, s-1] ∩ [s, tc-1] = ∅
```

---

### 2.5.5 Scalar Expansion

**What it does:** Promotes a scalar to an array to break anti-dependences and output dependences.

**Transformation:**
```
for I = 1 to N do
  T = A(I) + B(I)
  C(I) = T + 1/T
endfor
```
becomes:
```
if N >= 1 then
  allocate Tx(1:N)
  for I = 1 to N do
    Tx(I) = A(I) + B(I)
    C(I) = Tx(I) + 1/Tx(I)
  endfor
  T = Tx(N)
endif
```

**Legality condition:**
- The scalar has **no upward-exposed uses** in the loop.
- If the scalar is assigned conditionally, last-value assignment may not be possible.

**Formula — No upward-exposed uses:**

```
∀ uses u of T in loop:
    ∃ def d of T: d dominates u and d is in the same iteration
```

---

### 2.5.6 Loop Fusion

**What it does:** Combines two adjacent countable loops with compatible limits into one.

**Transformation:**
```
for I = ... do body1 endfor
for I = ... do body2 endfor
```
becomes:
```
for I = ... do body1 body2 endfor
```

**Legality condition:** All dependence relations must be preserved. Before fusion, all dependences must flow from body1 to body2.

**Formula — Cross-loop dependence distance:**

```
d_cross = iv_target - iv_source

Fusion is legal iff:
    d_cross > 0
    or d_cross = 0 and body1 lexically precedes body2
```

**Formula — Fusion with array assignments:**

```
Fusion of computation and copy loop is always legal if the LHS array is not used on the RHS.
```

---

### 2.5.7 Loop Alignment

**What it does:** Chooses which iteration of the second loop fuses with which iteration of the first.

**Alignment parameter m:** Iteration i of the first loop fuses with iteration i+m of the second.

**Formula — Alignment:**

```
Aligned iteration: (i, i + m)

Legality: m is chosen to eliminate negative distance dependences.

After alignment: d' = d - m
```

---

### 2.5.8 Loop Fission

**What it does:** Breaks a single loop into two or more smaller loops. Inverse of fusion.

**Transformation:**
```
for I = 1 to N do
  A(I) = A(I) + B(I-1)
  B(I) = C(I-1)*X + C
  C(I) = 1/B(I)
  D(I) = sqrt(C(I))
endfor
```
becomes:
```
for ib = 0 to N-1 do
  B(ib+1) = C(ib)*X + C
  C(ib+1) = 1/B(ib+1)
endfor
for ib = 0 to N-1 do
  A(ib+1) = A(ib+1) + B(ib)
endfor
for ib = 0 to N-1 do
  D(ib+1) = sqrt(C(ib+1))
endfor
```

**Legality condition:**
- Construct statement-level dependence graph.
- Find strongly connected components (SCCs).
- Nodes in the same SCC must stay in the same loop.
- If all nodes are in one SCC, fission cannot divide the loop.
- Ignore loop-carried anti-dependences and output dependences for scalars that will be expanded.
- Legal order: acyclic condensation of the dependence graph; any topological order.

**Formula — SCC condensation:**

```
SCC(G) = {C₁, C₂, ..., Cₖ}
Condensation(G) = (SCC(G), E')
where (Cᵢ, Cⱼ) ∈ E' iff ∃ u ∈ Cᵢ, v ∈ Cⱼ: (u, v) ∈ E
```

**Fission for parallel loops:**
- Fission for forall loops: always legal.
- Fission for doall loops: always legal.
- Fission for dopar loops: legal if SCCs respect the parallelism.

---

### 2.5.9 Loop Reversal

**What it does:** Runs a loop backward.

**Legality condition:**
- Reversal of forall, dopar, or dosingle loops: always legal.
- Reversal of sequential loop: legal only if the loop carries **no dependence relations**.

**Formula — Reversal:**

```
Distance d becomes -d
Direction < becomes >
Direction > becomes <
```

**Formula — Reversal with step ≠ 1:**

```
for I = 1 to 10 by 2 do
becomes:
for I = 1 + ((10-1)/2)*2 downto 1 by -2 do
```

---

### 2.5.10 Loop Interchanging

**What it does:** Switches inner and outer loop of a tightly nested pair.

**Legality condition (tightly nested):**
- Loop-independent dependences: cannot prevent interchanging.
- Dependence carried by inner loop before interchange (zero distance in outer): still carried by same loop after interchange — legal.
- Dependence carried by outer loop with zero distance in inner: still carried by same loop — legal.
- **Interchange-sensitive dependence:** Nonzero distance in both loops.
- Only direction vector (<,>) is violated by interchange.

**Formula — Interchange condition:**

```
Interchange is legal iff no dependence has direction (<,>)
```

**Formula — Loop limits after interchange (triangular/trapezoidal):**

```
Before interchange:
    A · i ≤ c

After interchange (j = T · i):
    A · T⁻¹ · j ≤ c

Apply Fourier-Motzkin projection:
    eliminate variables in order i_n, i_{n-1}, ..., i_2
    construct new limits from inequalities involving i₁, ..., i_k
```

**Formula — Outermosting/innermosting:**

```
Outermosting loop k:
    direction vector σ = (σ₁, ..., σₙ)
    legal iff for all j < k: σⱼ ≥ 0

Innermosting loop k:
    direction vector σ = (σ₁, ..., σₙ)
    legal iff for all j > k: σⱼ ≤ 0
```

---

### 2.5.11 Loop Skewing

**What it does:** Changes iteration vectors by adding outer loop index to inner loop index.

**Transformation:** Iteration (i, j) becomes (i, j + f·i) for skew factor f.

**Formula — Skewing:**

```
(i, j) → (i, j + f·i)

Distance (d₁, d₂) → (d₁, d₂ + f·d₁)
```

**Legality condition:** Always legal (unimodular transformation). But the resulting dependence distances must allow the desired subsequent transformation.

---

### 2.5.12 Linear Loop Transformations

**What it does:** Unifies interchanging, reversal, and skewing as linear transformations of the iteration space.

**Framework:**
- Transformation matrix T.
- Interchanging: T_int = [[0, 1], [1, 0]]
- Reversal: T_rev negates the index.
- Skewing: T_skew adds f·i to j.
- Composition: matrix multiplication.

**Formula — Legality:**

```
For sequential loops:
    0 ⪯ T · d for all dependence distance vectors d
```

**Formula — Loop limits:**

```
A · i ≤ c becomes A · T⁻¹ · j ≤ c
Apply Fourier-Motzkin projection.
```

**Formula — Unimodular transformations:**

```
det(T) = ±1
Preserves integer points.
Volume preserved.
```

**Formula — Nonunimodular transformations:**

```
T = H · U
where U is unimodular and H is in Hermite Normal Form.

Transform limits by U first, then H.
Loop strides are h_mm (diagonal of HNF).
Bounds computation involves floor operations.
```

---

### 2.5.13 Strip-Mining

**What it does:** Decomposes a single loop into two nested loops: strip loop and element loop.

**Transformation:**
```
for I = 1 to N do body endfor
```
becomes:
```
for Is = 1 to N by s do
  for I = Is to min(N, Is+s-1) do
    body
  endfor
endfor
```

**Formula — Dependence modification:**

```
Distance d becomes (d div s, d mod s)
If d mod s ≠ 0:
    another dependence with distance (d div s + 1, -((s - d) mod s))
```

**Formula — Direction vector modification:**

| Old Direction | New Direction |
|---|---|
| (<) | {(<,<)} |
| (=) | (=,<) |
| (>) | {(>,<)} |

---

### 2.5.14 Loop Tiling

**What it does:** Like strip-mining but for nested loops.

**Transformation:** Characterized by tile size ts and tile offset to.

**Formula — Tiling:**

```
for I = lo to hi do
becomes:
for It = ... by ts do
  for I = max(lo, It) to min(hi, It+ts-1) do
    body
  endfor
endfor

Each tile starts at i where i mod ts = to
```

**Legality condition:** Same as strip-mining for the tile loop; interchanging tile and element loops uses the same rules as loop interchanging.

---

### 2.5.15 Circular Loop Skewing

**What it does:** Skews inner loop iterations to wrap around a cylinder.

**Formula — Circular skewing:**

```
j^n = mod(j^c + i^c, N)
```

**Legality condition:** Inner loop must carry no dependences.

---

### 2.5.16 Striping

**What it does:** Strip-mining followed by interchange of strip and element loops.

**Transformation:**
```
for I = lo to hi do body endfor
```
becomes:
```
for Is = 0 to s-1 do
  for I = lo+Is to hi by s do
    body
  endfor
endfor
```

**Legality condition:** May not be legal (if negative distance vectors result).

---

### 2.5.17 Loop Collapsing

**What it does:** Collapses two nested loops into a single loop when array bounds match loop limits and memory stride is constant.

**Legality condition:** Array layout known, strides match.

---

### 2.5.18 Loop Coalescing

**What it does:** Collapses nested loops by computing original index values.

**Transformation:**
```
for I = lo_I to hi_I do
  for J = lo_J to hi_J do
    body
  endfor
endfor
```
becomes:
```
tc_I = hi_I - lo_I + 1
tc_J = hi_J - lo_J + 1
for IJ = 0 to tc_I*tc_J - 1 do
  I = IJ / tc_J + lo_I
  J = MOD(IJ, tc_J) + lo_J
  body
endfor
```

**Legality condition:** Always legal if loop limits are invariant.

---

### 2.5.19 Interprocedural Transformations

**Procedure Inlining:**
```
Copies procedure body in place of call.
Legality condition: Always legal.
```

**Loop Embedding:**
```
Takes loop surrounding a call and embeds it into the procedure.
```

**Loop Extraction:**
```
Takes loop out of procedure and puts it at call site.
```

---

# LAYER 3 — SPP-HX (HIERARCHICAL EXECUTION OPTIMIZER)

## 3.1 A — Hardware Model

| Component | What it reads | Lines |
|---|---|---|
| CPU topology | P-cores, E-cores, SMT | 400 |
| Cache hierarchy | L1/L2/L3 sizes, associativity | 300 |
| SIMD capabilities | AVX2, AVX-512, NEON | 200 |
| Memory bandwidth | Channels, speed | 300 |
| NUMA topology | Nodes, distances | 300 |
| Sync costs | Barrier, atomic, launch | 400 |

**Total: ~1,900 lines**

## 3.2 B — Analytic Cost Model

**Formula — Cost model:**

```
log2(speedup) = log2(baseline_time / transformed_time)

baseline_time = total_ops * baseline_cycles_per_op

transformed_time = max(compute_time, memory_time) + sync_time + tile_overhead

compute_time = total_ops / (flops_per_cycle_per_core * cores * simd_factor)

memory_time = total_bytes / (bytes_per_cycle_per_core * cores) + cache_miss_cycles

cache_miss_cycles = hierarchical cache model (L1/L2/L3/DRAM)

sync_time = (if parallel) thread_start_cycles + sync_barrier_cycles * log2(threads)

tile_overhead = (if tiled) tile_overhead_cycles * num_tiles

simd_factor = (if simd_width > 0) simd_width/128 * simd_efficiency
             + (if simd_width == 0) 1.0
```

No ML model, no training, no retraining at install. Coefficients are calibrated by hand against benchmarks.

**Lines: 1,500**

## 3.3 C — Bayesian Search Space

**Formula — Bayesian optimization:**

```
argmax_{x ∈ X} f(x)

where:
    f(x) = predicted_speedup
    X = (thread_count, simd_width, tile_size, chunk_size, fusion, schedule, placement)
```

| Dimension | Range |
|---|---|
| Thread count (P) | 1 to num_cores |
| SIMD width (V) | 128, 256, 512 bits |
| Tile size (B) | L1 / L2 / L3-resident |
| Chunk size (C) | Adaptive |
| Fusion | Which loops to fuse |
| Schedule | Static / dynamic / guided |
| Placement | NUMA node, core type |

**Lines: 2,000**

## 3.4 D — Execution Optimizer

| Component | What it does | Lines |
|---|---|---|
| Static partitioning | Compile-time partition | 700 |
| Hierarchical scheduling | Program → Region → Loop → Tile → Worker → SIMD | 800 |
| Barrier minimization | Only sync where dependence requires | 600 |
| NUMA placement | First-touch, local scheduling | 500 |
| Cache locality | Tile placement | 600 |
| Adaptive load balancing | Work stealing, chunk adjustment | 700 |

**Total: ~3,900 lines**

## 3.5 E — Online Re-Optimization

**Formula — Re-JIT trigger:**

```
if calibration_reveals(initial_static_choice is wrong):
    re-JIT hot loop mid-execution
```

**Lines: 1,200**

## 3.6 F — Energy/Power as First-Class Cost

**Formula — Energy cost:**

```
TotalCost = LatencyCost + α · EnergyCost

where α depends on hardware type:
    α = 0.0 for desktop
    α = 0.5 for mobile SoC
    α = 1.0 for battery-constrained
```

**Lines: 600**

## 3.7 G — Profitability Gate

**Formula — Profitability:**

```
if predicted_speedup >= 10:
    emit SPP parallel version
else:
    choose SIMD / serial / other version
```

**Lines: 800**

## 3.8 Components

| # | Component | Lines |
|---|---|---|
| 3.1 | Hardware model | 1,900 |
| 3.2 | Learned cost model | 1,500 |
| 3.3 | Bayesian search space | 2,000 |
| 3.4 | Execution optimizer | 3,900 |
| 3.5 | Online re-optimization | 1,200 |
| 3.6 | Energy cost model | 600 |
| 3.7 | Profitability gate | 800 |

**Total: ~11,900 lines**

## 3.9 Locality Optimization (Layer 3)

### 3.9.1 Core Concepts

**Formula — Temporal reuse:**

```
Ad = 0
```

**Formula — Spatial reuse:**

```
A_S · d = 0
where A_S = A with stride-1 row removed
```

**Formula — Self-reuse factor:**

```
R_k = { temporal reuse factor  if > 1
      { spatial reuse factor    otherwise

Temporal reuse factor:
    N_k if e_k ∈ ker A
    1 otherwise

Spatial reuse factor:
    max(1, l / a_sk) if e_k ∈ ker A_S
    1 otherwise
```

**Formula — Cumulative self-reuse:**

```
R_k* = ∏_{i=k}^{n} R_i
```

**Formula — Data footprint:**

```
F_k* = l · ∏_{i=k}^{n} (N_i / R_i)
```

**Formula — Cache locality condition:**

```
Footprint fits into cache: F_k* ≤ cache_size
```

**Formula — Degree of reuse:**

```
R_k^D = dim(span(e_k, ..., e_n) ∩ ker A)
```

**Formula — Group reuse:**

```
Distance d if A · d = c₁ - c₂
Solutions: d + x, x ∈ ker A
Orthogonal condition: d · x = 0
```

**Formula — Group-temporal reuse factor:**

```
If d = 0: ∞ for all loops
If d ≠ 0: N_k / d_k for loop k, 1 for others
```

**Formula — Group-spatial reuse:**

```
Distance d if A_S · d = c_{S,1} - c_{S,2}
```

**Formula — Total group reuse:**

```
GroupReuse_total = GroupReuse_temporal · GroupReuse_spatial
```

**Formula — Generalized reuse factor:**

```
Reuse_generalized = Reuse_self · Reuse_group
```

### 3.9.2 Sorting Loops by Reuse Factor

```
For each loop k:
    Compute innermost reuse factor R_k^innermost
    Treat loop as not carrying reuse when reuse distance has > 1 nonzero entry
    Group-spatial locality: treat loop as innermost
Choose loop with highest innermost reuse factor
Sum innermost reuse factors for all arrays
If best loop cannot be innermost, choose next best
```

---

# LAYER 4 — SPECULATION TIERS

## 4.1 The Corrected Tiers

| Tier | Mechanism | Cost | When |
|---|---|---|---|
| 0 | Static proof | 0 | Always |
| 1 | Cheap runtime guards | ~10 cycles | Static fails |
| 2 | Version / range checks | ~100 cycles | Guards fail |
| 3 | Chunk speculation | ~1,000 cycles | Checks fail |
| 4 | Page / version tracking | ~10,000 cycles | Chunks fail |
| 5 | Software TM | ~100,000 cycles | Pages fail |
| 6 | **Optimized speculation** | **~150–550 cycles** | Universal |
| **Hardware fast path** | **TM (TSX/TME) if available** | **~1,000 cycles** | **Opportunistic** |

**The compiler always selects the cheapest tier capable of validating the assumption.**

## 4.2 Tier 6 — Optimized Software Speculation

| Mechanism | What it does |
|---|---|
| **Shadow pages** | Per-speculative-unit shadow page |
| **Page-table swapping** | Commit via page pointer swap |
| **Register save/restore** | Rollback via registers |
| **Version counters** | Conflict detection via counters |
| **MMU traps** | Invalid access detection |
| **Async-signal-safe handler** | Fast rollback |
| **Recovery trampoline** | Non-destructive unwind |

## 4.3 The Three-Layer Rollback

**Layer 1 — Register save/restore:**
```
At speculation start: save all registers to TLS
At rollback: restore all registers from TLS
Cost: ~20–50 cycles
```

**Layer 2 — Shadow pages:**
```
At speculation start: allocate shadow page for writes
                     write to shadow, not real memory
At commit: swap shadow page → real memory
At rollback: discard shadow page
Cost: ~100–500 cycles
```

**Layer 3 — Version counters (fine-grained conflicts):**
```
At each write: increment version counter for the location
At commit: check read-set versions
At rollback: discard write-set
Cost: ~10–50 cycles per operation
```

**Composite rollback cost: 150–550 cycles.**

## 4.4 Correctness Gaps

**Allocator-aware rollback:**
```
Speculative regions need a thread-local bump allocator with a checkpoint/reset.
```

**Nested speculation:**
```
Explicit stack of checkpoints, not a single global one.
```

**I/O and non-idempotent side effects:**
```
Buffering + deferred commit.
Log the syscall, only flush on commit.
```

## 4.5 Hardware TM as Fast Path

Hardware TM is a fast path in front of Tier 6, used opportunistically when available.

## 4.6 Components

| # | Component | Lines |
|---|---|---|
| 4.1 | Tier selector | 500 |
| 4.2 | Runtime guards | 600 |
| 4.3 | Version / range checks | 700 |
| 4.4 | Chunk speculation | 900 |
| 4.5 | Page versioning | 700 |
| 4.6 | Software TM | 1,000 |
| 4.7 | Tier 6 optimized speculation | 1,500 |
| 4.8 | Allocator-aware rollback | 800 |
| 4.9 | Nested checkpoint stack | 600 |
| 4.10 | I/O buffering + deferred commit | 900 |
| 4.11 | Hardware TM fast path | 700 |
| 4.12 | Commit / rollback engine | 1,000 |
| 4.13 | Conflict detection | 700 |
| 4.14 | Rollback trampoline | 400 |
| 4.15 | Speculation cost model | 500 |

**Total: ~11,500 lines**

---

# LAYER 5 — ACCELERATION

## 5.1 Portable Vector IR

**Auto-vectorization should target a portable vector IR** rather than hand-targeting AVX2/AVX-512/NEON separately.

**Formula — Vector length selection:**

```
VL = min(max_vector_width, trip_count)
```

## 5.2 GPU Offload Cost Model

**Formula — Transfer cost:**

```
T_total = T_transfer + T_compute

T_transfer = data_size / bandwidth
T_compute = flops / gpu_flops

Offload iff T_gpu_total < T_cpu_total
```

## 5.3 Components

| # | Component | Lines |
|---|---|---|
| 5.1 | Portable vector IR | 1,200 |
| 5.2 | Software prefetch | 500 |
| 5.3 | Cache-blocked tiling | 700 |
| 5.4 | GPU transfer cost model | 800 |
| 5.5 | CPU / GPU partitioning | 600 |

**Total: ~3,800 lines**

## 5.4 Concurrency Engine (Layer 5)

### 5.4.1 Scheduling Policies

**Formula — Block scheduling:**
```
⌈N/P⌉ consecutive iterations per worker
```

**Formula — Cyclic scheduling:**
```
Every Pth iteration per worker
```

**Formula — Guided self-scheduling:**
```
R₀ = N
C_i = ⌈R_{i-1} / P⌉
R_i = R_{i-1} - C_i
```

**Formula — Guided variation:**
```
C_i = ⌈R_{i-1} / (2P)⌉
```

**Formula — Factoring:**
```
R₀ = N
C_i = ⌈R_{i-1} / (f·P)⌉
R_i = R_{i-1} - P·C_i
where f = fixed fraction (e.g., f = 2)
```

### 5.4.2 Synchronization

**Formula — Await/advance:**
```
await(counter, value): block until counter reaches value
advance(counter): atomically increment counter
```

**Formula — Swap lock:**
```
mylock = 1
repeat
    swap(lock, mylock)
until mylock = 0
```

**Formula — Shadow read-only copy:**
```
mylock = 1
repeat
    repeat
    until lock = 0
    swap(lock, mylock)
until mylock = 0
```

**Formula — Queue lock:**
```
Array of locks, pointer to last process.
Enter queue: small critical section; busy-wait for previous.
Release: change value of Lock[myid].
```

### 5.4.3 Reductions

**Formula — Parallel reduction:**
```
Worker p accumulates private partial sum
Then accumulate global sum from partial sums
Order of accumulation depends on scheduling and finish order
```

**Formula — First-order linear recurrence parallelization:**
```
x_i = a_i · x_{i-1} + c_i

Substitution:
x_i = (a_i · a_{i-1}) · x_{i-2} + (a_i · c_{i-1} + c_i)

Block scheduling:
Worker p computes iterations lo(p) to hi(p):
    local coefficients (b_i, d_i)
barrier
Final parallel computation:
    x_i = b_i · x_{lo(p)-1} + d_i
```

### 5.4.4 Alignment

**Formula — Alignment graph:**
```
One node per statement, two edges per data dependence:
    dependence edge with distance
    inverse edge with negative distance

If all cycles have accumulated distance zero:
    alignment removes all loop-carried dependences
```

### 5.4.5 Strip-Mining for Parallelism

**Formula — Strip-mining distance:**
```
If d divisible by s:
    (d div s, 0)
Otherwise:
    (d div s, d mod s) and (d div s + 1, (d mod s) - s)

If only loop-carried dependence:
    element loop parallel
If several dependences with distance > 1:
    strip size = minimum dependence distance
```

### 5.4.6 Index Set Partitioning

**Formula — HNF of dependence matrix:**
```
D = dependence matrix (columns = dependence distance vectors)
H = D · U  (HNF decomposition)

Row with zero diagonal interchanged to bottom.
First k rows positive diagonal.

Diagonal elements tell number of independent partitions:
    loop j has h_jj independent partitions if h_jj > 0
    if h_jj = 0: iterations of loop j completely independent

Parallel loops:
    For h_jj > 0: doall P_j = 0 to h_jj - 1
    For h_jj = 0: doall P_j = 0 to N_j - 1

Serial loops:
    I₁ = P₁ + h₁₁ · q₁
    q₁ = ⌈(LO₁ - P₁) / h₁₁⌉ to ⌈(HI₁ - P₁) / h₁₁⌉

    I_j = P_j + Σ_{k=1}^{j-1} h_jk · q_k + h_jj · q_j
    ⌈(LO_j - P_j - Σ_{k=1}^{j-1} h_jk · q_k) / h_jj⌉
        ≤ q_j ≤
    ⌈(HI_j - P_j - Σ_{k=1}^{j-1} h_jk · q_k) / h_jj⌉
```

## 5.5 Vectorization Engine (Layer 5)

### 5.5.1 Vector Code

**Formula — Vector execution:**
```
C[i:i+W] = A[i:i+W] + B[i:i+W]
where W = hardware vector width
```

**Formula — Vector register:**
```
Starting position = first element
Stride = 1
Vector length in special register VL
```

### 5.5.2 Conditionals

**Formula — Vector compare:**
```
Vector compare instruction generates bit-vector result.
Mask register (Cray) or bit-vector registers (Fujitsu).
Bit vector density: ratio of 1-bits.
Conditional vector instructions: additional bit vector operand.
Disables operation where bit = 0.
```

### 5.5.3 Reductions

**Formula — Vector reduction:**
```
Four steps:
    1. Store vector register to memory
    2. Load partial sums with shorter vector length
    3. Accumulate short vectors with vector instructions
    4. Scalar instructions to add partial sums
```

### 5.5.4 Vectorization Legality

**Formula — Vectorization condition:**
```
Vector execution preserves lexically forward dependence relations.
Only sequential loops with acyclic dependence graphs can be vectorized.
Topologically sort graph to remove lexically backward dependences.
```

### 5.5.5 Handling Scalars

**Formula — Induction variable cycles:**
```
Remove by substitution:
    Replace IV with closed-form expression
```

**Formula — Scalar temporaries:**
```
Remove by scalar expansion:
    Expanded scalar size = size of vector register
    No memory allocated
    Last value assignments added
```

### 5.5.6 Dependence Cycles

**Formula — Loop fission with temporaries:**
```
If fission splits assignment/use of scalar across two loops:
    scalar may need dynamically allocated vector in memory
Avoid by strip-mining before fission:
    scalar temporary replaced by vector register temporary
```

**Formula — Loop fission with conditionals:**
```
Condition saved in temporary vector
Control dependences replaced by data dependences for fission
If conditional test in dependence cycle:
    compute true and false values
    solve boolean recurrence for bit vector
    then vector assign
```

**Formula — Vectorization despite cycles:**
```
Anti-dependence cycles: self anti-dependence cannot form cycle
Multiple statements: operation granularity resolves false cycles
Partial expression vectorization: compute subexpressions in vector mode
Strip-mining: strip size s satisfies dependences with distance ≥ s
Index set splitting: split where data access conflicts change direction
Run-time dependence testing: generate two versions
Ignoring dependence cycles: output dependence cycles can be ignored
    if stored value loop-invariant and identical
```

### 5.5.7 Nested Loops

**Formula — Loop interchanging for vectorization:**
```
Move dependence-carrying loop to outermost level
Break cycle
Choose best loop for vector mode based on:
    memory stride
    vector startup
    loop limits
Generate multiple versions depending on run-time conditions
```

**Formula — Outer loop vectorization:**
```
Legal if all dependence relations satisfied with sequential code
while ignoring dependence vector element for outer loop.
Equivalent to strip-mining outer loop and interchanging element loop
to inner position.
```

**Formula — Supervector performance:**
```
Loop interchanging improves ratio of memory to arithmetic operations.
```

### 5.5.8 Multivector Computers

**Formula — Two levels of parallelism:**
```
Outer concurrent loop across processors
Inner vector loop within processor
Single loop: strip-mine, run strip loop concurrently across processors
```

---

# THE FULL PIPELINE

```
Input IR
    │
    ▼
┌─────────────────────────────────────────┐
│ 1. Dependence Engine                    │
│    Static proof + alias + reductions    │
│    + effects + runtime checks           │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 2. Parallel Version Generation          │
│    Version lattice + Pareto selector    │
│    + transformations (fusion, fission,  │
│      interchange, skewing, tiling, etc.)│
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 3. SPP-HX                               │
│    A. Hardware model                    │
│    B. Learned cost model                │
│    C. Bayesian search                   │
│    D. Execution optimizer               │
│    E. Online re-optimization            │
│    F. Energy cost                       │
│    G. Profitability gate (10x)          │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 4. Speculation Tiers                    │
│    Tier 0–6, hardware fast path         │
│    + three-layer rollback               │
│    + allocator-aware rollback           │
│    + nested checkpoints                 │
│    + I/O buffering                      │
└────────────────┬────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│ 5. Acceleration                         │
│    Portable vector IR + prefetch        │
│    + cache blocking + GPU partitioning  │
│    + concurrency engine                 │
│    + vectorization engine               │
└────────────────┬────────────────────────┘
                 │
                 ▼
Machine code + certificates
```

---

# THE COMPLETE LINE COUNT

| Layer | Lines |
|---|---|
| 1. Dependence Engine | 15,300 |
| 2. Parallel Version Generation | 2,100 |
| 3. SPP-HX | 11,900 |
| 4. Speculation Tiers | 11,500 |
| 5. Acceleration | 3,800 |
| Integration | 2,900 |
| Tests | 12,000 |
| **Total** | **~59,500** |

With headers and generated code: **~70,000 lines.**

---

# THE BUILD ORDER

| # | What | Time |
|---|---|---|
| 1 | Static dependence proof (done) | ✅ |
| 2 | Remaining dependence levels | 3 weeks |
| 3 | Interprocedural alias analysis | 3 weeks |
| 4 | Non-affine dependence | 1 week |
| 5 | Reduction generalization | 1 week |
| 6 | Cross-iteration alias speculation | 1 week |
| 7 | Dependence Engine integration | 2 weeks |
| 8 | Parallel Version Generation | 1 week |
| 9 | SPP-HX: Hardware model | 1 week |
| 10 | SPP-HX: Learned cost model | 2 weeks |
| 11 | SPP-HX: Bayesian search | 2 weeks |
| 12 | SPP-HX: Execution optimizer | 3 weeks |
| 13 | SPP-HX: Online re-optimization | 2 weeks |
| 14 | SPP-HX: Profitability gate | 2 days |
| 15 | Speculation tiers | 7 weeks |
| 16 | Acceleration | 3 weeks |
| 17 | Integration | 1 week |
| 18 | Tests | 2 weeks |
| **Total** | — | **~35 weeks** |

---

# THE ONE HONEST SENTENCE

> **SPP is five layers: Dependence Engine (15,300 lines with interprocedural alias, non-affine gray zone, reduction generalization, cross-iteration speculation, effect summaries), Parallel Version Generation (2,100, lattice not fixed A/B/C/D, cross-version dedup, full transformation catalog: fusion, fission, interchange, skewing, strip-mining, tiling, circular skewing, striping, collapsing, coalescing, interprocedural), SPP-HX (11,900, analytic cost model (roofline + cache hierarchy), Bayesian search, online re-optimization, energy cost, locality optimization with reuse distance/footprint/loop ordering), Speculation Tiers (11,500, Tier 6 optimized speculation with three-layer rollback, allocator-aware rollback, nested checkpoints, I/O buffering, hardware TM fast path), Acceleration (3,800, portable vector IR, GPU transfer cost model, concurrency engine with scheduling/synchronization/reductions/alignment, vectorization engine with SIMD/conditionals/reductions/cycle-breaking). Total: ~59,500 lines. 35 weeks. 10x is the admission criterion, not a universal guarantee.**
  
