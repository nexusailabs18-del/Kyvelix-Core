# docs/TRIAGE.md

**Project:** KYJIT
**Document type:** One-time triage record
**Date:** 2026-09-13
**Author:** Director
**Status:** Frozen after this run. Amended, not replaced.
**Supersedes:** Nothing

---

## 0. Purpose

Decide what "real" means in this project.

The old tree at `~/kyjit-enterprise/` contains ~90 `.c` files and ~140 `.h` files. Some compile, have tests, and produce correct output. Some are stubs, backups, duplicates, borrowed vendor code, or dead files.

This document:
- Lists the verified keep-list (files with real evidence)
- Lists the discard categories with detection commands
- Records the archive plan

**Rule:** Port only what compiles, has tests, produces correct output, and is yours.

---

## 1. Old tree inventory

### 1.1 Locations

| Field | Value |
|---|---|
| Old tree | `~/kyjit-enterprise/` |
| New tree | `~/kyjit/` |
| Archive target | `~/kyjit-enterprise-archive-2026-09-13.tar.gz` |

### 1.2 Counts

| Metric | Count |
|---|---|
| `.c` files in `src/` | ~90 |
| `.h` files in `include/` | ~140 |
| `.h` files in `src/` | ~15 |
| Headers without matching `.c` | ~120 (stubs) |
| Borrowed files | 4 (V8 + libuv) |
| Real, tested, verified | ~15 |

### 1.3 Structural finding

The tree is **flat in `src/`** with two nested subdirectories:
- `src/jit/` — the LLVM JIT wrapper and tiered compilation
- `src/kyjit_gc/` — the GC (gen + concurrent)
- `src/kyjit_wasm/` — Wasmtime integration
- `src/kyjit_event/` — libuv borrowed code
- `src/kyjit_security/` — V8 borrowed code
- `src/kyjit_eventloop/` — event loop
- `src/kyjit_ring/` — lock-free ring

Everything else is at the top of `src/` with the prefix `kyjit_`.

---

## 2. Keep-list (verified components)

Each entry: source path, evidence, reason to keep.

### 2.1 Inline Cache — the crown jewel

| # | Component | Source | Evidence |
|---|---|---|---|
| 1 | Inline Cache | `src/kyjit_ic.c` + `include/kyjit_ic.h` | 0.239 ns mono, 2.02x V8, 100K ICs stress-tested, 0 fails. Frozen v1.0.0. |
| 2 | IC (v2) | `src/kyjit_ic2.c` + `include/kyjit_ic2.h` | Alternative IC. Verify before keeping; may be superseded by #1. |
| 3 | Self-modifying IC | `src/kyjit_selfmod_ic.c` + `include/kyjit_selfmod_ic.h` | Patches call sites with specialized code. Deferred feature; keep header for reference. |

### 2.2 Allocation

| # | Component | Source | Evidence |
|---|---|---|---|
| 4 | TLAB | `src/kyjit_tlab.c` + `include/kyjit_tlab.h` | 2.25 ns/alloc, 16x malloc. Verified. |
| 5 | Zone | `src/kyjit_zone.c` + `include/kyjit_zone.h` | 2.86 ns/alloc. Verified. |

### 2.3 Garbage Collector

| # | Component | Source | Evidence |
|---|---|---|---|
| 6 | Generational GC | `src/kyjit_gc/kyjit_gc.c` + `.h` | 100K objects in ~5.5 ms. Verified. |
| 7 | Concurrent GC | `src/kyjit_gc/kyjit_gc_concurrent.c` | Concurrent marking. Verified. |

### 2.4 μOp Cache

| # | Component | Source | Evidence |
|---|---|---|---|
| 8 | μOp Cache | `src/kyjit_uop_cache.c` + `include/kyjit_uop_cache.h` | 5 blocks, add/mul/sub/inc/dec, verified. |

### 2.5 Front-end

| # | Component | Source | Evidence |
|---|---|---|---|
| 9 | Parser | `src/kyjit_parser.c` + `include/kyjit_parser.h` | Parses numbers and `+`. Verified. Coverage: ~5% of JS. |
| 10 | Bytecode | `src/kyjit_bytecode.c` + `include/kyjit_bytecode.h` | 11 opcodes. Verified. |
| 11 | Interpreter | `src/kyjit_interpreter.c` + `include/kyjit_interpreter.h` | Direct-threaded, 11 opcodes. Verified. |

### 2.6 Backend

| # | Component | Source | Evidence |
|---|---|---|---|
| 12 | LLVM JIT wrapper | `src/kyjit_llvm_jit.c` + `include/kyjit_llvm_jit.h` | Compiles IR → native. No JS→IR path yet. |
| 13 | LLVM JIT (v21) | `src/jit/llvm_jit_v21.c` + `.h` | Newer wrapper. Verify which supersedes #12. |
| 14 | JIT core | `src/jit/kyjit_core.c` | JIT integration layer. Verify. |
| 15 | Tiered compilation | `src/jit/tiered/kyjit_tiered.c` + `.h` | Tier promotion. Verify. |
| 16 | BLAS JIT | `src/kyjit_blas_jit.c` + `include/kyjit_blas_jit.h` | OpenBLAS wrapper. 82 GFLOP/s @ 512. |
| 17 | Cache-blocked matmul | `src/kyjit_cache_block.c` + `include/kyjit_cache_block.h` | 759 μs for 256×256. Verified. |

### 2.7 Platform

| # | Component | Source | Evidence |
|---|---|---|---|
| 18 | CPU detect | `src/kyjit_cpu_detect.c` + `include/kyjit_cpu_detect.h` | Detects i5-1240P, 16 threads. Verified. |
| 19 | GPU detect | `src/kyjit_gpu_detect.c` + `include/kyjit_gpu_detect.h` | Detects Intel iGPU. Verified. |

### 2.8 Security

| # | Component | Source | Evidence |
|---|---|---|---|
| 20 | Sandbox | `src/kyjit_sandbox.c` + `include/kyjit_sandbox.h` | Blocks over-limit allocations. Verified. |
| 21 | StackGuard | `src/kyjit_stackguard.c` + `include/kyjit_stackguard.h` | Guard page trip. Verified. |

### 2.9 WASM / WASI

| # | Component | Source | Evidence |
|---|---|---|---|
| 22 | WASI | `src/kyjit_wasi.c` + `include/kyjit_wasi.h` | 32 syscalls. Verified. |
| 23 | WASM exec | `src/kyjit_wasm_exec.c` + `include/kyjit_wasm_exec.h` | Wasmtime execution. Verify. |

### 2.10 Foundation — utilities (candidates, verify before keeping)

| # | Component | Source | Status |
|---|---|---|---|
| 24 | Hash | `src/kyjit_hash.c` + `include/kyjit_hash.h` | Test failed earlier ("not deterministic"). Verify before keeping. |
| 25 | String table | `src/kyjit_string_table.c` + `include/kyjit_string_table.h` | Interning. Verify. |
| 26 | String core | `src/kyjit_string_core.c` | String ops. Verify. |
| 27 | Base64 | `src/kyjit_base64.c` + `include/kyjit_base64.h` | Verified. |
| 28 | SWAR | `src/kyjit_swar.c` + `include/kyjit_swar.h` | SWAR string ops. Verify. |
| 29 | Branchless | `src/kyjit_branchless.c` + `include/kyjit_branchless.h` | Branchless primitives. Verify. |
| 30 | Utils | `src/kyjit_utils.c` + `include/kyjit_utils.h` | Miscellaneous. Verify. |

### 2.11 Runtime objects (candidates, verify)

| # | Component | Source | Status |
|---|---|---|---|
| 31 | Objects | `src/kyjit_object.c` + `include/kyjit_object.h` | Basic object model. Verify. |
| 32 | Shapes | `src/kyjit_shape.c` + `include/kyjit_shape.h` | Hidden classes. Verify. |
| 33 | String | `src/kyjit_string.c` + `include/kyjit_string.h` | String object. Verify. |
| 34 | Array | `src/kyjit_array.c` + `include/kyjit_array.h` | Array. Verify. |
| 35 | JSON | `src/kyjit_json.c` + `include/kyjit_json.h` | Parse/stringify. Verify. |
| 36 | RegExp | `src/kyjit_regexp.c` + `include/kyjit_regexp.h` | Pattern match. Verify. |
| 37 | BigInt | `src/kyjit_bigint.c` + `include/kyjit_bigint.h` | BigInt. Verify. |
| 38 | Date | `src/kyjit_date.c` + `include/kyjit_date.h` | Date. Verify. |
| 39 | Handles | `src/kyjit_handles.c` + `include/kyjit_handles.h` | Handles. Verify. |
| 40 | Roots | `src/kyjit_roots.c` + `include/kyjit_roots.h` | GC roots. Verify. |

### 2.12 Keep-list summary

| Tier | Count |
|---|---|
| Verified (real evidence) | ~15 |
| Candidates (need one verification run) | ~15 |
| **Total keep-list** | **~30** |

---

## 3. Discard categories

### 3.1 Borrowed code (4 files) — DISCARD

| File | Origin |
|---|---|
| `src/kyjit_security/code-memory-access.h` | V8 |
| `src/kyjit_security/code-memory-access-inl.h` | V8 |
| `src/kyjit_event/uv-common.c` | libuv |
| `src/kyjit_event/uv-common.h` | libuv |
| `src/kyjit_event/strscpy.h` | Linux kernel / libuv helper |

**Disposition:** Discard. If functionality is needed, use as library, not as copied source.

### 3.2 Stub headers (~120 files) — DISCARD

Any `.h` in `include/` whose basename has no matching `.c` in `src/`.

Examples: `kyjit_cuda.h`, `kyjit_dpdk.h`, `kyjit_graphcolor_full.h`, `kyjit_instsel_full.h`, `kyjit_loop_detect_full.h`, `kyjit_osr_full.h`, `kyjit_regalloc_full.h`, `kyjit_safepoint_full.h`, `kyjit_scheduler_full.h`, `kyjit_speculative_full.h`, `kyjit_stackwalk_full.h`, `kyjit_writebarrier_full.h`, plus ~100 others.

**Detect:**
```bash
cd ~/kyjit-enterprise
for h in include/*.h; do
  base=$(basename "$h" .h)
  [ -f "src/${base}.c" ] || echo "STUB: $h"
done | wc -l
```

**Disposition:** Discard. These are design sketches. Rewrite from spec when the feature is actually built.

### 3.3 Backups and duplicates — DISCARD

Files with `.bak`, `.bak[0-9]`, `.before_*`, `.2026*`, `.old` suffixes, plus multiple versions of the same component.

Examples:
- `src/kyjit_ic.c.20260911_185630`
- `src/kyjit_ic.c.bak`
- `src/kyjit_ic.c.before_mega_pack.20260911_191132`
- The `backups/` directory in the old tree

**Detect:**
```bash
find ~/kyjit-enterprise -name '*.bak*' -o -name '*.before_*' -o -name '*.2026*' | wc -l
```

**Disposition:** Archive in git history. Do not port.

### 3.4 Dead / unverified code — DEFER

Files that exist but haven't been tested. The candidate list in Section 2.10–2.11 falls here.

**Disposition:** Run one verification test per file. If it passes, promote to keep-list. If not, discard.

### 3.5 Discard summary

| Category | Count | Disposition |
|---|---|---|
| Borrowed | 5 | Discard |
| Stub headers | ~120 | Discard |
| Backups/duplicates | ~50 | Archive |
| Unverified | ~45 | Verify or discard |

---

## 4. Rationale

### 4.1 The rule
> Port only what compiles, has tests, produces correct output, and is yours.

### 4.2 Why ~30 and not 200
Because ~30 is what has evidence. The rest is claim without proof. The keep-list grows by verification, not by assertion.

### 4.3 Why now
Every decision in `~/kyjit/` depends on knowing what "real" means. Without triage:
- We can't say "the IC is done" if there are 2 ICs
- We can't say "the GC works" if there are gen + concurrent + unclear ownership
- We can't say anything about the project because the tree is flat and mixed

Triage resolves this. Once done, "real" = "on the keep-list."

---

## 5. Archive plan

### 5.1 Commands

```bash
# 1. Archive old tree
tar czf ~/kyjit-enterprise-archive-2026-09-13.tar.gz ~/kyjit-enterprise/

# 2. Port verified files (adapt paths; new tree uses same flat layout for now)
cd ~/kyjit
mkdir -p src/runtime src/frontend src/backend src/platform src/compliance src/safety src/foundation

cp ~/kyjit-enterprise/src/kyjit_ic.c              src/runtime/ic.c
cp ~/kyjit-enterprise/include/kyjit_ic.h          src/runtime/ic.h
cp ~/kyjit-enterprise/src/kyjit_tlab.c            src/runtime/tlab.c
cp ~/kyjit-enterprise/include/kyjit_tlab.h        src/runtime/tlab.h
cp ~/kyjit-enterprise/src/kyjit_zone.c            src/runtime/zone.c
cp ~/kyjit-enterprise/include/kyjit_zone.h        src/runtime/zone.h
cp ~/kyjit-enterprise/src/kyjit_uop_cache.c       src/backend/uop_cache.c
cp ~/kyjit-enterprise/include/kyjit_uop_cache.h   src/backend/uop_cache.h
cp ~/kyjit-enterprise/src/kyjit_parser.c          src/frontend/parser.c
cp ~/kyjit-enterprise/include/kyjit_parser.h      src/frontend/parser.h
cp ~/kyjit-enterprise/src/kyjit_bytecode.c        src/compliance/bytecode.c
cp ~/kyjit-enterprise/include/kyjit_bytecode.h    src/compliance/bytecode.h
cp ~/kyjit-enterprise/src/kyjit_interpreter.c     src/compliance/interpreter.c
cp ~/kyjit-enterprise/include/kyjit_interpreter.h src/compliance/interpreter.h
cp ~/kyjit-enterprise/src/kyjit_llvm_jit.c        src/backend/llvm_jit.c
cp ~/kyjit-enterprise/include/kyjit_llvm_jit.h    src/backend/llvm_jit.h
cp ~/kyjit-enterprise/src/kyjit_blas_jit.c        src/backend/blas_jit.c
cp ~/kyjit-enterprise/include/kyjit_blas_jit.h    src/backend/blas_jit.h
cp ~/kyjit-enterprise/src/kyjit_cpu_detect.c      src/platform/cpu_detect.c
cp ~/kyjit-enterprise/include/kyjit_cpu_detect.h  src/platform/cpu_detect.h
cp ~/kyjit-enterprise/src/kyjit_gpu_detect.c      src/platform/gpu_detect.c
cp ~/kyjit-enterprise/include/kyjit_gpu_detect.h  src/platform/gpu_detect.h
cp ~/kyjit-enterprise/src/kyjit_sandbox.c         src/safety/sandbox.c
cp ~/kyjit-enterprise/include/kyjit_sandbox.h     src/safety/sandbox.h
cp ~/kyjit-enterprise/src/kyjit_stackguard.c      src/safety/stackguard.c
cp ~/kyjit-enterprise/include/kyjit_stackguard.h  src/safety/stackguard.h
cp ~/kyjit-enterprise/src/kyjit_wasi.c            src/compliance/wasi.c
cp ~/kyjit-enterprise/include/kyjit_wasi.h        src/compliance/wasi.h
cp ~/kyjit-enterprise/src/kyjit_gc/kyjit_gc.c     src/runtime/gc.c
cp ~/kyjit-enterprise/src/kyjit_gc/kyjit_gc_concurrent.c src/runtime/gc_concurrent.c
cp ~/kyjit-enterprise/src/kyjit_gc/kyjit_gc.h     src/runtime/gc.h
```

### 5.2 New tree becomes canonical

**2026-09-13.**

---

## 6. Open questions

| # | Question | Status |
|---|---|---|
| 6.1 | Does old tree have a working Makefile? | PENDING |
| 6.2 | Does old tree have a test runner? | PENDING |
| 6.3 | CI config? | PENDING |
| 6.4 | Docs in old tree worth keeping? | PENDING |
| 6.5 | External dependencies to track? | PENDING |

---

## 7. Sign-off

| Field | Value |
|---|---|
| Triage performed by | Director |
| Date | 2026-09-13 |
| Keep-list (verified) | ~15 |
| Keep-list (candidates) | ~15 |
| Total keep-list | ~30 |
| Discard categories | 4 (borrowed, stubs, backups, unverified) |
| Next document | `docs/PROOF_INFRASTRUCTURE.md` |

---

## 8. Amendments

| Date | Amendment | Reason |
|---|---|---|
| 2026-09-13 | Initial triage | — |

---

## 9. What comes next

1. Run the discard detection commands (Section 3).
2. Verify the candidate files (Section 2.10–2.11) with one test each.
3. Run the archive commands (Section 5.1).
4. Write `docs/PROOF_INFRASTRUCTURE.md`.

---

**End of docs/TRIAGE.md**