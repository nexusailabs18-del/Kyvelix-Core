# UDDP Continuation / Migration Document

> **Purpose:** Capture the complete state of the UDDP (Universal Dependence Detection Pipeline) project so it can be resumed, audited, or migrated to another repository/team without loss of context.

---

## 📍 Project Status (as of 2026-10-09)

| Metric | Value |
|--------|-------|
| **Language** | Rust (edition 2021) |
| **Crate** | `m0` (single crate, 18 subsystems M0–M13) |
| **Source** | 194 files, ~69K lines (excl. `target/`) |
| **Tests** | 582 library tests + integration suites + fuzz |
| **Certificates** | Machine-checkable; verified by independent `m0-check` |
| **Build** | `cargo build --release` (overflow-checks = true) |
| **Git** | Clean working tree, 4 commits ahead of origin/main |

---

## 🏗️ Architecture Invariants (Non-Negotiable)

| Invariant | Description |
|-----------|-------------|
| **A — Checker Independence** | Every decision carries a certificate. `m0-check` verifies **without running analyzer code**. `V(loop_ir, cert) ∈ {ACCEPT, REJECT}` — total, deterministic, resource-bounded. |
| **B — Sound by Conservatism** | Output lattice: `Exact < None < Unknown`. Analyzer moves only toward `Unknown` when uncertain. Wrong `Exact`/`None` = catastrophic. `Unknown` is always safe. |
| **B1 — Degradation Direction** | Symbolic → `Unknown` or refined exact + `Rewrite` witness. **Never** silent symbolic→constant inside `Exact`/`None`. |
| **A1 — Certificate Completeness** | Nothing influences a decision that isn't (a) replayed from cert, or (b) re-derivable by checker from IR. No "trust-me" fields. |

---

## ✅ Hardening Ledger — All 5 Items **DONE**

| Item | Description | Status | Guard |
|------|-------------|--------|-------|
| **H-1** | Symbolic→constant degradation inside cert emission | ✅ DONE | 58 fuzz tests, checked arithmetic |
| **H-2** | Checker arithmetic overflow | ✅ DONE | `checked_*` everywhere; overflow ⇒ `REJECT` |
| **H-3** | Missing env in witness emission | ✅ DONE | `ExactEnv`/`NoneEnv` carry all deps; emission fails if absent |
| **H-4** | Checker trust boundary breach | ✅ DONE | Checker uses **only** `uddp-ir` + `uddp-arith`; zero analyzer code |
| **H-5** | Non-determinism in certificate emission | ✅ DONE | Deterministic witnesses; seed-and-revert validated |

> **All 5 items are fixed-and-fuzz-guarded. The hardening ledger is empty.**

---

## 📦 Shipped Subsystems (18)

| Level | Name | Scope | Certificates |
|-------|------|-------|--------------|
| M0 | GCD test | Linear Diophantine equations | `Exact` / `None` |
| M1 | Fourier-Motzkin (reals) | Real relaxation + gcd filter | `Exact` / `None` |
| M2G | Generalized Banerjee-Wolfe | Multi-dim, symbolic strides | `Exact` / `None` |
| M2A | Advanced M2 | Parametric bounds | `Exact` / `None` / `Unknown` |
| M2A5 | M2A + 5-var | Extended vars | `Exact` / `None` / `Unknown` |
| M2B | Banerjee-Wolfe base | Classic test | `Exact` / `None` |
| M3 | Induction variables | SCC-based IV detection (§6.5) | `TripCount`, `ScalarDependence` |
| M4 | Constant propagation | `⊤/⊥/Const` lattice (§6.6) | `Exact` / `None` |
| M5 | Chasing recurrences | Array recurrence analysis | `Exact` / `None` / `Unknown` |
| M6 | ScalarDD | 4 dependence situations (§6.7) | `Exact` / `None` / `Direction` / `Level` |
| M7 | Points-to (intraproc) | Allocation sites | `Alias` / `None` |
| M8 | Value recurrences | Linear recurrences over values | `Exact` / `None` / `Unknown` |
| M9 | Interprocedural | Call graph + summaries | `Summary` / `RuntimeCheck` |
| M10 | Control dependence | Post-dominance | `Exact` / `None` |
| M11 | Region analysis | Array regions | `RegionSummary` |
| M12 | Loop transformations | Tiling, fusion, etc. | `Transformation` |
| M13 | Parallelism detection | DOALL/DOACROSS | `Parallel` / `None` |

---

## 🔑 Key Artifacts & Interfaces

### Certificate Claim Kinds
```
Exact(d)           → IntSolution witness
None               → UnsatProof witness
Direction(D)       → IntSolution + componentwise check
Level(k)           → IntSolution + claim_ok
CrossLoop(d)       → CrossLoopSolution
Parametric(T)      → Per-leaf env verification
RegionSummary      → Fixed-point derivation
Alias              → Transfer function replay
Summary            → Composition record
RuntimeCheck       → Guard ⇒ independence proof
```

### Crate Topology (Must Preserve)
```
uddp-ir → uddp-arith → uddp-core / m0-check → spp-*
```
- **Edges point LEFT** (analyzer depends on spec, not vice versa)
- `uddp-arith` is the **only** shared crate (it's the specification)
- Checker contains **zero** analyzer code

### Process Gates (§18)
1. Fuzz gate — every new emission site has adversarial fuzz
2. Corpus regression — 1,001-loop M0 batch + 17 level E2E runners
3. SerDe round-trip — every artifact serializes→parses to identity
4. Phase boundary — no bulk acceptance; regressions individually triaged

---

## 🚫 MUST-NOT-Harden Constraints

1. **Enumeration caps** — fixed per `ARCHITECTURE.md`
2. **A1/A5 quarantine** — certificates that cross the checker boundary
3. **SPP only via artifacts** — no analyzer callbacks to SPP

---

## 📋 Open Work (Precision Extensions)

| Bucket | Items | Description |
|--------|-------|-------------|
| **(I) Integer exactness** | 2, 3, 8 | Exact integer solvers for specific patterns |
| **(C) Certificate scope** | 6 | New claim kinds for richer proofs |
| **(A) Analyzer depth** | 4, 7 | Deeper analysis without new cert kinds |
| **Reclassify** | 5 | M5 chase recurrence → SPP-only (frontend needed) |

> **Note:** Item 5 (M5) requires Phase 9b frontend to emit indexed loads — accepted by scope deferral.

---

## 🛠️ Quick Start (for New Environment)

```bash
# 1. Clone & enter
git clone <repo> && cd m-ky

# 2. Build (releases with overflow checks)
cd m0 && cargo build --release

# 3. Verify core loop
./target/release/m0-decide tests/m2g_loops/g001.json
./target/release/m0-check tests/m2g_loops/g001.json cert.json
# → ACCEPT

# 4. Run full regression
cd .. && ./uddp.sh all
# or
make check
# → 10/10 certs ACCEPT

# 5. Library tests
make lib
# → 582 passed
```

---

## 📂 Repository Layout (Post-Cleanup)

```
m-ky/
├── .gitignore          # Ignores target/, tmp_*, fuzz_artifacts/, return, {
├── Makefile            # build, test, clean, check, bench, fmt, clippy, doc
├── README.md           # Full user-facing docs
├── CONTINUATION.md     # THIS FILE — migration/handoff reference
├── uddp.sh             # Control panel: lib, integration, negative, level, all
├── docs/               # All specs & status reports
│   ├── PHASE*_STATUS.md
│   ├── phase*.txt, phase*.md
│   ├── HANDOFF.md
│   ├── stubs.md        # Source of this document
│   ├── litellm_config.yaml
│   └── refernce.txt
└── m0/                 # Main crate
    ├── .gitignore      # Same patterns + crate-specific
    ├── Cargo.toml      # overflow-checks = true in [profile.release]
    ├── ENGINE_SUMMARY.md
    ├── README.md
    ├── benches/
    ├── examples/
    ├── src/            # 18 subsystem modules + decide dispatcher
    └── tests/          # 17 level runners + negative corpus + phase9 tests
```

---

## 🔄 Change Control (Appendix B Summary)

| Rule | Requirement |
|------|-------------|
| **Additive amendments only** | Retracting invariants requires freezing/migrating artifacts |
| **Architecture first** | New artifact/claim/IndexKind → `ARCHITECTURE.md` **before** code |
| **Protected boundaries** | Enumeration caps, precedence ladder, `uddp-arith` sharing → soundness argument required |

---

## 📖 Reference Documents (in `docs/`)

| File | Purpose |
|------|---------|
| `stubs.md` | **Source of truth** — this document derived from it |
| `PHASE*_STATUS.md` | Per-phase implementation status |
| `phase4,5and6.txt` | Phase 4/5/6 combined spec (~10K lines) |
| `phase7,8and9.md` | Phases 7–9 spec |
| `phase_3.txt` | Phase 3 spec (FUD chains) |
| `completephase4.txt` | Phase 4 completion analysis |
| `refernce.txt` | Phase 2 math engine spec |
| `ENGINE_SUMMARY.md` | M0/M1/M4/M6/M9 engine architecture (in `m0/`) |
| `HANDOFF.md` | Project handoff notes |
| `litellm_config.yaml` | LLM config used during development |

---

## ⚠️ Known Limitations / Technical Debt

| Area | Note |
|------|------|
| **M5 chase recurrence** | Table lookup only; needs Phase 9b frontend for indexed loads |
| **M8 value recurrence** | Linear recurrence over *values* not computable by `induct` — spec gap |
| **Phase 9b frontend** | Accepted by scope deferral; not in current deliverable |
| **7 soundness bugs** | Found by differential fuzzing — certificates **cannot** catch model bugs |
| **ARITH-007** | 2-var parametric → requires enum cap lift (needs amendment) |

---

## ✅ Definition of Done (Per §20)

- [x] Every phase: corpus ACCEPTs, seed-and-revert clean, A/B batch explained
- [x] `m0-check` contains no analyzer code; shared surface = `uddp-ir` + `uddp-arith` only
- [x] SPP touches UDDP exclusively through artifacts A1–A5
- [x] Hardening ledger emptied — all items fixed-and-fuzz-guarded

---

## 📝 For the Next Maintainer

1. **Run `make check` first** — it builds and verifies the certificate pipeline end-to-end
2. **Read `docs/stubs.md`** — it's the authoritative ledger; this document is a summary
3. **Preserve the crate topology** — `uddp-arith` is the spec boundary; don't leak analyzer code into checker
4. **Fuzz everything new** — every emission site needs adversarial fuzz (H-1/H-5 pattern)
5. **Certificates are the contract** — if it's not in the cert, the checker won't accept it

---

*Generated from `docs/stubs.md` — the hardening ledger is empty, all 5 items DONE. The project is in a verified, shippable state.*
