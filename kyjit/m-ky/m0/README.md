# UDDP — Universal Dependence Detection Pipeline

A Rust dependence analyzer for JavaScript-style loops. Decides whether two
array accesses in a loop can touch the same memory location, and at what
distance. Emits machine-checkable certificates that an independent checker
verifies.

**Status:** 18 subsystems shipped. ~9,500 lines of Rust. 140 lib tests,
3 checker tests, ~60 end-to-end test loops. Zero regressions on the M0
baseline (675 / 326 / 0 / 0 on 1,001 real loops).

**Note on the folder name:** the tree lives at `~/kyjit/m-ky/m0`. The `m0`
is historical — it started as the M0 prototype folder. The project is
UDDP. The analyzer binaries keep the `m0-*` prefix for stability.

---

## Table of contents

- [What UDDP does](#what-uddp-does)
- [Quick start](#quick-start)
- [Architecture in one picture](#architecture-in-one-picture)
- [The certificate discipline](#the-certificate-discipline)
- [Levels shipped](#levels-shipped)
- [Directory layout](#directory-layout)
- [Building](#building)
- [Running the analyzer](#running-the-analyzer)
- [Running the checker](#running-the-checker)
- [Running the test suite](#running-the-test-suite)
- [Control panel script](#control-panel-script)
- [The IR](#the-ir)
- [The certificate format](#the-certificate-format)
- [Adding a new level](#adding-a-new-level)
- [What fits UDDP vs what doesn't](#what-fits-uddp-vs-what-doesnt)
- [Troubleshooting](#troubleshooting)

---

## What UDDP does

Given a loop like

```javascript
for (i = 0; i < N; i++)
    A[i] = A[i + 1];
UDDP decides:

Is there a dependence? Yes — the write to A[i] and the read from
A[i+1] are the same location when the loop carries the dependence.

What is the distance? -1 (the write at iteration i conflicts
with the read at iteration i+1).

Is there no dependence? For A[i] vs A[i + 100] on a short loop,
UDDP proves disjointness and returns None.

Every decision comes with a certificate that an independent checker
verifies. The checker does not re-run the analysis; it replays the
witness against the loop file on disk.

Quick start
bash
# Enter the tree
cd ~/kyjit/m-ky/m0

# Build
cargo build --release

# Analyze a loop
./target/release/m0-decide tests/m2g_loops/g001.json

# Verify the emitted certificate
./target/release/m0-check tests/m2g_loops/g001.json cert.json

# Run one level's test suite
./tests/run_m2g_tests.sh

# Run the full regression
~/uddp.sh all
Architecture in one picture
text
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
    │  M13 (assertions)   ─┐
    │  M10 (fixed point)   │  Pre-M0 routes:
    │  M8  (recurrence)    │  any level that rewrites
    │  M7  (hash)          │  the loop before M0 sees it
    │  M6  (products)      │
    │  M11 (symbolic FP)   │
    │  M9  (mod-p)         │
    │  M2g, M2a, M2a.5    │
    │  M4, M2b, M5, M3    ─┘
    │
    ▼
┌──────────────────────────────────────────────┐
│        M0 — constant-bound affine            │
│  Every lower level delegates here after      │
│  rewriting the loop into affine form         │
└──────────────────────────────────────────────┘
    │
    ▼
Certificate (JSON)
    │
    ▼
┌──────────────────────────────────────────────┐
│       Checker (m0-check binary)              │
│  Verifies the certificate against the loop   │
│  file. No analysis re-run; witness replay.   │
└──────────────────────────────────────────────┘
The certificate discipline
Every level of UDDP emits a certificate. The certificate format is
unchanged since M2a.5. Two Claim variants are used:

Claim::Exact(Vec<i64>) — the dependence distance is exactly the vector.

Claim::None — the two accesses never touch the same location.

Two Witness variants carry the proof:

Witness::IntSolution { i, j } — concrete IV values that make the two
accesses touch the same location.

Witness::UnsatProof { constraints } — a contradictory constraint
proving no dependence.

Two carrier variants wrap the witness for parametric or assertion
contexts:

Witness::WithEnv { env, inner } — carries a runtime-parameter binding
(M9 for ModParam, M12 for dynamic bounds).

Witness::AssertionChain { assertions, inner } — carries a chain of
assertions under which the inner witness is verified (M13).

The rule: every certificate must be replayable by the checker.
No SMT calls at check time. No re-running the analysis. If a level
cannot produce a replayable witness, it returns Unknown.

Levels shipped
Level	What it handles	E2E
M0	Constant-bound affine (A[i+c], A[c*i])	675/326/0
M1	Parametric affine (bounds from parameters)	✅
M2g	Affine-in-disguise ((i<<c), i*c, (i<<a)|b, (i+c)*a, (i/c)*c)	8/8
M2a	Injectivity, affine A[B[i]]	9/9
M2a.5	Injectivity, table A[B[i]]	8/8
M4	Affine guards (if (i < C))	8/8
M2b	Bounded range (i % C, min(i,K), max(i,K))	8/8
M3	Bounded recursion (single self-call)	8/8
M3.5	Mutual recursion (f ↔ g)	3/3
M5	Bounded pointer chase via literal next	8/8
M9	Runtime modulus (i % p)	3/3
M6	Product index (i*j, i*i)	3/3
M6c	Cubic+ (degree ≤ 4)	2/2
M7	Hash index ((i*c) >> k & mask)	3/3
M8	Linear recurrence (Fibonacci-like)	3/3
M10	Fixed-point iteration (while(x != next[x]))	3/3
M11	Symbolic pointer chase (next computed at runtime)	3/3
M12	Dynamic bounds (for(i = f; i < g; i++))	1/1
M13	Assertions (assert(i != j))	3/3
Directory layout
text
m0/                                    (the tree — historical name)
├── Cargo.toml
├── Cargo.lock
├── README.md                          (this file)
├── src/
│   ├── lib.rs
│   ├── main.rs                        m0-decide CLI
│   ├── ir/
│   │   └── mod.rs                     Loop, IV, Access, IndexKind,
│   │                                  IndexExpr, HashExpr, RecurrenceId,
│   │                                  FixedPointTable, RangeExpr, BoundValue
│   ├── certificate/
│   │   └── mod.rs                     Certificate, Claim, Witness, Assertion
│   ├── checker/
│   │   └── main.rs                    m0-check binary
│   ├── decide/
│   │   ├── mod.rs                     Dispatcher
│   │   ├── m0.rs                      M0 — constant affine
│   │   ├── m1.rs                      M1 — parametric affine
│   │   ├── m2g.rs                     M2g — affine-in-disguise
│   │   ├── m2a.rs                     M2a — injectivity (affine)
│   │   ├── m2a5.rs                    M2a.5 — injectivity (table)
│   │   ├── m4.rs                      M4 — guards
│   │   ├── m2b.rs                     M2b + M9 — range + runtime modulus
│   │   ├── m3.rs                      M3 + M3.5 — recursion
│   │   ├── m5.rs                      M5 — pointer chase
│   │   ├── m6.rs                      M6 + M6c — products + cubic
│   │   ├── m7.rs                      M7 — hash
│   │   ├── m8.rs                      M8 — recurrence
│   │   ├── m10.rs                     M10 — fixed point
│   │   ├── m11.rs                     M11 — symbolic fixed point
│   │   ├── m12.rs                     M12 — dynamic bounds
│   │   └── m13.rs                     M13 — assertions
│   ├── param_expr.rs                  ParamExpr for symbolic expressions
│   ├── symbolic.rs                    Feautrier + symbolic constraints
│   ├── solver/
│   │   └── mod.rs                     Fourier-Motzkin elimination
│   ├── arena.rs                       Arena allocator (experimental)
│   ├── arena_parse.rs                 Arena-based parser (experimental)
│   ├── config.rs                      Thread / runtime config
│   └── decide_ref.rs                  Arena-path dispatcher (experimental)
├── src/bin/                           12 additional CLI binaries
│   ├── m0-batch.rs                    Batch analyzer
│   ├── m0-packed.rs                   Packed format
│   ├── m0-parallel.rs                 Multi-threaded
│   ├── m0-stream.rs                   Streaming
│   ├── m0-pack.rs                     Pack a corpus
│   ├── m0-compile.rs                  Compile loop JSON to packed
│   ├── m0-arena.rs                    Arena-path analyzer
│   ├── m0-bench.rs                    In-process benchmark
│   ├── m1-decide.rs                   M1 entry
│   ├── m1-distance.rs                 M1 distance analysis
│   ├── m0-batch-bin.rs                Binary batch
│   └── debug_p004.rs                  Debug helper
├── tests/
│   ├── real_loops/                    1,001-loop corpus
│   ├── m2g_loops/                     + every level's loops:
│   ├── m2a_loops/                     m2a, m2a5, m4, m2b, m3, m35,
│   ├── m2a5_loops/                    m5, m9, m6, m6c, m7, m8, m10,
│   ├── m4_loops/                      m11, m12, m13
│   ├── m2b_loops/
│   ├── m3_loops/
│   ├── m35_loops/
│   ├── m5_loops/
│   ├── m9_loops/
│   ├── m6_loops/
│   ├── m6c_loops/
│   ├── m7_loops/
│   ├── m8_loops/
│   ├── m10_loops/
│   ├── m12_loops/
│   ├── m13_loops/
│   ├── run_m2g_tests.sh              per-level runners
│   ├── run_m2a_tests.sh
│   ├── run_m2a5_tests.sh
│   ├── run_m4_tests.sh
│   ├── run_m2b_tests.sh
│   ├── run_m3_tests.sh
│   ├── run_m35_tests.sh
│   ├── run_m5_tests.sh
│   ├── run_m9_tests.sh
│   ├── run_m6_tests.sh
│   ├── run_m6c_tests.sh
│   ├── run_m7_tests.sh
│   ├── run_m8_tests.sh
│   ├── run_m10_tests.sh
│   ├── run_m11_tests.sh
│   ├── run_m12_tests.sh
│   ├── run_m13_tests.sh
│   ├── run_negative.sh               soundness corpus
│   └── run_bug_tests.sh              edge-case corpus
└── target/                           build artifacts (gitignored)
Building
bash
cd ~/kyjit/m-ky/m0

# Debug build
cargo build

# Release build (recommended — 10-30x faster)
cargo build --release

# Run the full library test suite
cargo test --release --lib

# Run the checker test suite
cargo test --bin m0-check
The release build takes ~30 seconds on a modern laptop. The analyzer on
the 1,001-loop corpus runs in ~10 ms.

Running the analyzer
m0-decide is the analyzer binary. It reads a loop JSON, decides
every ordered pair of accesses, and writes a certificate to cert.json.

Usage
bash
./target/release/m0-decide <loop.json>
Output format
text
pair (0, 1): EXACT distance [-1]
pair (1, 0): EXACT distance [1]
overall: EXACT
wrote cert.json
EXACT distance [d1, d2, ...] — dependence distance is the vector.

NONE — no dependence between the two accesses.

UNKNOWN (reason) — the analyzer could not decide.

overall: — the strongest claim across all pairs.

The certificate for the first pair that emitted one is written to
cert.json.

With a runtime environment (M9, M12)
bash
M0_ENV="p=8,N=32" ./target/release/m0-decide tests/m9_loops/n001.json
M0_ENV is a comma-separated list of name=value pairs.

The environment is passed to m2b::decide_param for M9 (runtime
modulus) and m12::decide_with_env for M12 (dynamic bounds).

If the environment is missing a needed parameter, the affected pairs
return UNKNOWN.

All CLI binaries
Binary	Purpose
m0-decide	Analyze a single loop
m0-batch	Analyze the 1,001-loop corpus
m0-pack / m0-packed	Packed JSON format for batch mode
m0-parallel	Multi-threaded batch mode
m0-stream	Streaming input mode
m1-decide / m1-distance	M1-specific entry points
m0-arena	Arena-based analyzer path (experimental)
m0-compile	Compile a loop JSON to a packed format
m0-bench	In-process benchmark harness
Build all with cargo build --release. They land in target/release/.

Running the checker
m0-check verifies a certificate against a loop file. It is
independent of the analyzer: it does not call decide, it only replays
the witness.

Usage
bash
./target/release/m0-check <loop.json> <cert.json>
Output
ACCEPT — the certificate is valid.

ACCEPT (disjoint ranges) — M2b-style disjointness proof verified.

REJECT: <reason> — the certificate is invalid. Reasons:

loop hash mismatch — the certificate refers to a different loop.

witness arity — witness vectors have the wrong length.

i[k] out of bounds — witness IV value outside the loop's IV range.

locations differ — witness IV values do not produce the same location.

distance mismatch — claimed distance does not match the actual diff.

assertion X does not hold — an M13 assertion fails at the witness.

Exit codes
0 — accepted.

1 — rejected.

Running the test suite
Per-level runners
bash
cd ~/kyjit/m-ky/m0

./tests/run_m2g_tests.sh     # 8/8
./tests/run_m2a_tests.sh     # 9/9
./tests/run_m2a5_tests.sh    # 8/8
./tests/run_m4_tests.sh      # 8/8
./tests/run_m2b_tests.sh     # 8/8
./tests/run_m3_tests.sh      # 8/8
./tests/run_m35_tests.sh     # 3/3
./tests/run_m5_tests.sh      # 8/8
./tests/run_m9_tests.sh      # 3/3
./tests/run_m6_tests.sh      # 3/3
./tests/run_m6c_tests.sh     # 2/2
./tests/run_m7_tests.sh      # 3/3
./tests/run_m8_tests.sh      # 3/3
./tests/run_m10_tests.sh     # 3/3
./tests/run_m11_tests.sh     # 3/3
./tests/run_m12_tests.sh     # 1/1 (unit)
./tests/run_m13_tests.sh     # 3/3
Each runner does three things:

Runs cargo test --lib decide::<level> (unit tests).

Runs the level's E2E loops through m0-decide.

Runs the emitted certificate through m0-check (verify ACCEPT).

Soundness corpus
bash
./tests/run_negative.sh      # 8/8 — every malformed cert must REJECT
./tests/run_bug_tests.sh     # 20/20 — analyzer must not panic
The negative corpus feeds deliberately wrong certificates to the checker
and asserts every one is REJECTed. The bug corpus exercises edge cases
(zero-length loops, mismatched bounds, overflow-prone indices) and
asserts the analyzer never panics.

Full regression
bash
cd ~/kyjit/m-ky/m0

# M0 batch — the handover baseline
./target/release/m0-batch tests/real_loops 2>/dev/null | head -6
# Expected: Loops: 1001, EXACT: 675, NONE: 326, UNKNOWN: 0, Crashes: 0

# Library tests
cargo test --release --lib 2>&1 | grep "test result"
# Expected: 140 passed; 0 failed

# Checker tests
cargo test --bin m0-check 2>&1 | grep "test result"
# Expected: 3 passed; 0 failed

# Every level's E2E
for t in m2g m2a m2a5 m4 m2b m3 m35 m5 m9 m6 m6c m7 m8 m10 m11 m12 m13; do
    printf "%-6s " "$t"
    ./tests/run_${t}_tests.sh 2>&1 | grep -E "PASS:" | head -1
done
Control panel script
Save this as ~/uddp.sh. One command for every common operation.

bash
cat > ~/uddp.sh << 'SCRIPT_EOF'
#!/usr/bin/env bash
# UDDP control panel
set -u
cd ~/kyjit/m-ky/m0 || { echo "tree not found at ~/kyjit/m-ky/m0"; exit 1; }

case "${1:-help}" in
    build)
        echo "=== building ==="
        cargo build --release 2>&1 | tail -3
        ;;

    test)
        echo "=== library tests ==="
        cargo test --release --lib 2>&1 | grep "test result"
        echo ""
        echo "=== checker tests ==="
        cargo test --bin m0-check 2>&1 | grep "test result"
        echo ""
        echo "=== M0 batch ==="
        ./target/release/m0-batch tests/real_loops 2>/dev/null | head -6
        ;;

    all)
        echo "=== library tests ==="
        cargo test --release --lib 2>&1 | grep "test result"
        echo ""
        echo "=== checker tests ==="
        cargo test --bin m0-check 2>&1 | grep "test result"
        echo ""
        echo "=== M0 batch ==="
        ./target/release/m0-batch tests/real_loops 2>/dev/null | head -6
        echo ""
        echo "=== every level E2E ==="
        for t in m2g m2a m2a5 m4 m2b m3 m35 m5 m9 m6 m6c m7 m8 m10 m11 m12 m13; do
            printf "%-6s " "$t"
            ./tests/run_${t}_tests.sh 2>&1 | grep -E "PASS:" | head -1
        done
        echo ""
        echo "=== soundness ==="
        ./tests/run_negative.sh 2>&1 | tail -1
        ./tests/run_bug_tests.sh 2>&1 | tail -1
        ;;

    level)
        # uddp level m13
        local lvl="${2:-}"
        if [ -z "$lvl" ]; then
            echo "usage: $0 level <m2g|m2a|m2a5|m4|m2b|m3|m35|m5|m9|m6|m6c|m7|m8|m10|m11|m12|m13>"
            exit 2
        fi
        ./tests/run_${lvl}_tests.sh
        ;;

    decide)
        # uddp decide tests/m2g_loops/g001.json
        local f="${2:-}"
        if [ -z "$f" ]; then
            echo "usage: $0 decide <loop.json>"
            exit 2
        fi
        echo "=== analyze $f ==="
        ./target/release/m0-decide "$f"
        if [ -f cert.json ]; then
            echo ""
            echo "=== verify cert ==="
            ./target/release/m0-check "$f" cert.json
        fi
        ;;

    decide-env)
        # uddp decide-env p=8 tests/m9_loops/n001.json
        local env="${2:-}"
        local f="${3:-}"
        if [ -z "$env" ] || [ -z "$f" ]; then
            echo "usage: $0 decide-env <name=value,...> <loop.json>"
            exit 2
        fi
        echo "=== analyze $f with env=$env ==="
        M0_ENV="$env" ./target/release/m0-decide "$f"
        if [ -f cert.json ]; then
            echo ""
            echo "=== verify cert ==="
            ./target/release/m0-check "$f" cert.json
        fi
        ;;

    batch)
        echo "=== M0 batch ==="
        ./target/release/m0-batch tests/real_loops 2>/dev/null | head -6
        ;;

    soundness)
        echo "=== negative (must reject all) ==="
        ./tests/run_negative.sh
        echo ""
        echo "=== bug (must not panic) ==="
        ./tests/run_bug_tests.sh
        ;;

    lines)
        echo "=== line counts ==="
        printf "%-40s %6s\n" "FILE" "LINES"
        for f in $(find src -name "*.rs" -not -path "src/bin/*" | sort); do
            printf "%-40s %6d\n" "$f" "$(wc -l < "$f")"
        done
        echo ""
        printf "%-40s %6d\n" "CORE TOTAL" "$(find src -name '*.rs' -not -path 'src/bin/*' -exec cat {} \; | wc -l)"
        printf "%-40s %6d\n" "BINARIES" "$(find src/bin -name '*.rs' -exec cat {} \; | wc -l)"
        printf "%-40s %6d\n" "TOTAL" "$(find src -name '*.rs' -exec cat {} \; | wc -l)"
        ;;

    tree)
        tree src -L 3 --dirsfirst 2>/dev/null || find src -maxdepth 3 -type d | sort
        ;;

    help|*)
        cat << 'HELP'
UDDP control panel
  uddp build               rebuild release
  uddp test                lib + checker tests + M0 batch
  uddp all                 everything: tests + every level + soundness
  uddp level <lvl>         run one level's E2E (e.g. uddp level m13)
  uddp decide <f>          analyze a loop and verify the cert
  uddp decide-env <env> <f> analyze with a runtime env
  uddp batch               M0 batch on the 1,001-loop corpus
  uddp soundness           negative + bug corpora
  uddp lines               per-file line counts
  uddp tree                src tree
  uddp help                this message
HELP
        ;;
esac
SCRIPT_EOF

chmod +x ~/uddp.sh
Usage
bash
~/uddp.sh                  # help
~/uddp.sh build            # rebuild
~/uddp.sh test             # quick smoke test
~/uddp.sh all              # full regression
~/uddp.sh level m13        # one level
~/uddp.sh decide tests/m2g_loops/g001.json
~/uddp.sh decide-env p=8 tests/m9_loops/n001.json
~/uddp.sh batch            # 1,001-loop corpus
~/uddp.sh soundness        # negative + bug
~/uddp.sh lines            # line counts
~/uddp.sh tree             # source tree
Install to $PATH:

bash
sudo cp ~/uddp.sh /usr/local/bin/uddp
sudo chmod +x /usr/local/bin/uddp
uddp help
The IR
The core types in src/ir/mod.rs:

Loop
rust
pub struct Loop {
    pub depth: usize,
    pub ivs: Vec<IV>,
    pub body: Vec<Access>,
    pub recursion: Option<Recursion>,      // M3 / M3.5
    pub chase: Option<PointerChase>,       // M5
    pub assertions: Vec<Assertion>,        // M13
}
IV
rust
pub struct IV {
    pub name: String,
    pub lower: i64,
    pub upper: i64,
    pub step: i64,
}
IV bounds are i64. M12 uses a parallel type DynamicIV with
BoundValue bounds; those are resolved to i64 before dispatch.

Access
rust
pub struct Access {
    pub kind: AccessKind,                  // Read | Write
    pub base: String,                      // array name
    pub index: Vec<i64>,                   // IV coefficients
    pub elem_size: i64,
    pub offset: OffsetValue,               // Const | Param
    pub expr: Option<String>,              // M2g surface expression
    pub indirect: Option<Indirect>,        // M2a / M2a.5
    pub guard: Option<Guard>,              // M4
    pub range: Option<RangeExpr>,          // M2b / M9
    pub index_kind: IndexKind,             // Affine | Product | Hash |
                                           // Recurrence | FixedPoint |
                                           // SymbolicFixedPoint
}
IndexKind
The single extension point for all non-affine index kinds:

rust
pub enum IndexKind {
    Affine,
    Product(IndexExpr),                    // M6 / M6c
    Hash(HashExpr),                        // M7
    Recurrence(RecurrenceId),              // M8
    FixedPoint(FixedPointTable),           // M10
    SymbolicFixedPoint {                   // M11
        next_expr: IndexExpr,
        max_steps: usize,
    },
}
Adding a new level means adding a variant here, a decision module in
src/decide/, a checker branch in src/checker/main.rs, and a
dispatcher route.

The certificate format
rust
pub struct Certificate {
    pub level: String,                     // "M0", "M13", ...
    pub loop_hash: String,                 // SHA256 of canonical loop JSON
    pub pair: (usize, usize),              // access indices
    pub claim: Claim,
    pub witness: Witness,
    pub checker: String,
}

pub enum Claim {
    Exact(Vec<i64>),                       // distance is exactly d
    None,                                  // no dependence
    Parametric(Vec<(String, Vec<ParamExpr>)>),
}

pub enum Witness {
    IntSolution { i: Vec<i64>, j: Vec<i64> },
    UnsatProof { constraints: Vec<CConstraint> },
    WithEnv { env: Vec<(String, i64)>, inner: Box<Witness> },
    AssertionChain {
        assertions: Vec<Assertion>,
        inner: Box<Witness>,
    },
}
Critical: when a level rewrites the loop before delegating to a
lower level (M2g, M2a, M3, M3.5, M4, M5, M6, M7, M8, M10, M11, M12,
M13), it must overwrite cert.loop_hash = loop_.hash() on the
original loop before returning. The checker hashes the file on disk,
not the in-memory transformed loop.

Adding a new level
Established pattern after M0–M13.

1. Extend the IR (if needed)
Add a variant to IndexKind:

rust
pub enum IndexKind {
    Affine,
    // ...
    YourNewKind(YourNewType),
}
Do not change IV. If your level needs parametric bounds, add a
parallel type (see DynamicLoop for M12) rather than mutating IV.

After adding the variant:

bash
cd ~/kyjit/m-ky/m0
cargo build --release 2>&1 | grep "missing field" | head
cargo test --lib 2>&1 | grep "missing field" | head
The first catches struct literals in non-test code. The second catches
test-module literals — cargo build alone will not see them. Every
existing IndexKind match site needs a new arm, usually
YourNewKind(_) => return None. The compiler names each one.

2. Write the decision procedure
Create src/decide/mXX.rs. Copy the shape of m6.rs or m10.rs:

rust
use super::{decide, Decision};
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    // 1. Does this level apply?
    // 2. Enumerate candidate locations (bounded, cap ~4096).
    // 3. Intersect; emit Exact with IntSolution or None with UnsatProof.
    // 4. Unknown if enumeration exceeds cap or level does not apply.
}

#[cfg(test)]
mod tests { /* 5-10 unit tests */ }
3. Wire into the dispatcher
In src/decide/mod.rs:

rust
pub mod mXX;

// Inside `pub fn decide`:
if loop_.body.iter().any(|a| matches!(&a.index_kind, IndexKind::YourNewKind(_))) {
    return mXX::decide(loop_, p, q);
}
Route before M0. Levels that rewrite the loop must run before the
generic affine path.

4. Extend the checker (if needed)
If your level uses existing IntSolution / UnsatProof witnesses, the
checker only needs:

rust
// In location_interval:
if matches!(access.index_kind,
    m0::ir::IndexKind::YourNewKind(_)) {
    return (0, -1);
}

// In witness_location:
if let m0::ir::IndexKind::YourNewKind(x) = &access.index_kind {
    return x.eval(iv_values).saturating_add(offset);
}
If it needs a new Witness variant, add a branch in the
match &cert.witness block.

5. Overwrite loop_hash
If your level rewrites the loop, must run:

rust
cert.loop_hash = loop_.hash();
before returning. Same discipline as M3.5, M12, M13.

6. Write tests
Unit tests in mXX.rs's #[cfg(test)] mod tests: 5-10 tests.

E2E test loops in tests/mXX_loops/: 2-7 JSON files.

Runner tests/run_mXX_tests.sh: same shape as the existing
runners.

7. Regression
bash
cd ~/kyjit/m-ky/m0

cargo test --release --lib 2>&1 | grep "test result"
cargo test --bin m0-check 2>&1 | grep "test result"
./target/release/m0-batch tests/real_loops 2>/dev/null | head -6
for t in m2g m2a m2a5 m4 m2b m3 m35 m5 m9 m6 m6c m7 m8 m10 m11 m12 m13 mXX; do
    printf "%-6s " "$t"
    ./tests/run_${t}_tests.sh 2>&1 | grep "PASS:" | head -1
done
What fits UDDP vs what doesn't
Fits UDDP (shipped)
Everything that reduces to IndexKind extension or Witness
carrier:

Constant / parametric affine (M0, M1)

Affine-in-disguise (M2g)

Injectivity (M2a, M2a.5)

Guards (M4)

Bounded range (M2b)

Recursion (M3, M3.5)

Pointer chase via literal table (M5)

Runtime modulus (M9)

Products / cubic+ (M6, M6c)

Hash (M7)

Linear recurrence (M8)

Fixed point (M10)

Symbolic fixed point (M11)

Dynamic bounds (M12)

Assertions (M13)

Does NOT fit UDDP (different project)
Three subsystems remain that require new architectures, not new
levels:

Subsystem	What it needs	Lines
Interprocedural	Call graph, function summaries, summary composition	~3,000
Points-to / aliasing	Abstract locations, alias sets, context-sensitive analysis	~2,500
Concurrency	Happens-before graphs, interleaving model	~2,000
These are the boundary of UDDP. Every extension of IndexKind, every
witness carrier, every assertion-chain wrapper — all shipped. What
remains requires new architectures (call graphs, abstract domains,
memory models) and is a separate project.

Troubleshooting
loop hash mismatch from the checker
Cause: the certificate was emitted for the transformed loop (a
level rewrote it before delegating to M0) but the checker hashes the
original file.

Fix: in the level's decide or wrap_* function, add
cert.loop_hash = loop_.hash(); before returning. Same discipline as
M3.5, M12, M13.

missing field 'assertions' / missing field 'chase' / etc.
Cause: a new field was added to Loop or Access and not every
struct literal has it.

Fix: the compiler names every site. Run cargo test --lib, not
just cargo build — the test modules have their own struct literals
that cargo build does not compile.

cannot find function 'decide' in module errors after adding a level
Cause: a level's pub fn decide shadows an imported decide from
the dispatcher.

Fix: import only Decision, call the dispatcher as super::decide.

REJECT: assertion does not hold
Cause: an M13 AssertionChain witness's IV values violate one of
the assertions.

Fix: the level's narrowing step must be consistent with the witness.
If you narrowed with i < 16 but emitted a witness at i = 20, the
checker rejects.

M12 E2E shows only unit tests, no E2E
Expected. M12's E2E requires a DynamicLoop input format. The CLI
m0-decide reads standard Loop JSON with i64 IV bounds. Full E2E
requires a --dynamic flag that deserializes DynamicLoop and calls
decide_dynamic. Currently unit tests cover M12.

cargo build --release succeeds but cargo test --lib fails
Cause: test-module struct literals. cargo build does not compile
#[cfg(test)] mod tests, but cargo test does.

Fix: run cargo test --lib 2>&1 | grep 'missing field' and patch
each named site.

See also
docs/PROOF_INFRASTRUCTURE.md — the soundness argument

docs/TRIAGE.md — how to classify Unknown failures

tests/run_negative.sh — the soundness corpus

tests/run_bug_tests.sh — the edge-case corpus

One sentence: UDDP decides dependence for affine and non-affine
loops, emits replayable certificates, and verifies them independently.
18 subsystems shipped, ~9,500 lines of Rust, 140 lib tests, 3 checker
tests, zero regressions on the M0 baseline.

## Fuzzing

Every `decide` function in the codebase has a brute-force
differential fuzzer as a `cargo test --lib` property test. These
compare `decide()` against an independent simulation of the loop's
semantics — they do not trust the analyzer's reasoning, only its
output.

Seven soundness bugs were found by this process in shipped code,
all invisible to the hand-written test corpus (140 tests + ~60 E2E
loops):

1. **`OffsetValue::as_const`** returned `None` for the deserialized
   `Param(Const(n))` form. Every call site that did
   `.unwrap_or(0)` silently substituted offset 0, so both the
   analyzer and the checker ignored every constant offset in every
   loop. Found by affine fuzzing (934/2352 mismatches).

2. **M7** decided hash-indexed dependence by comparing hash values
   instead of locations, ignoring offsets. `A[(i & 1) + 4]` vs
   `A[(j & 1) - 3]` returned `Exact([0])`; the correct answer is
   `None`. Found by 189/500 mismatches.

3. **M5** modeled chase values as IV coefficients
   (`loc(i) = chain[k] * i + offset`) instead of constant array
   indices (`loc = chain[k] + offset`). The analyzer's model of
   `for i: p = next[p]; A[p] = A[p+1]` was wrong. Found by
   185/2000 mismatches.

4. **M13** treated `Ne` assertions as no-ops, emitting witnesses
   that violated their own assertions. Found by 15/3000
   witness-violations.

5. **M3/M35** emitted step-s IV ranges that M0 iterated as step 1.
   `{0, 3, 6, 9, 12}` was treated as `{0, 1, ..., 14}`, producing
   spurious collisions. Found by 274/1217 mismatches.

6. **M4** intersected guards on the loop's shared IV range, missing
   collisions when only one side had a guard. Found by 146/1938
   mismatches.

7. **M1** accepted candidate distances not realizable within the IV
   range. `n=2, diff=4` returned `Const([4])` for a loop where no
   such distance could occur. Found by 54/5578 mismatches.

**The certificate pipeline cannot detect bugs of type 3, 5, 6, or
7.** The checker verifies that a witness is valid for the loop the
analyzer *claims* to have used, not that the analyzer's rewritten
loop faithfully models the original program. Bugs where the
analyzer's *model* is wrong produce certificates that the checker
accepts — the pipeline is internally consistent but semantically
incorrect. Only brute-force differential fuzzing catches this
class.

**Add a fuzzer whenever adding a level.** The pattern:

1. Generate random inputs of the level's shape (Rust `#[test]`
   with an inline xorshift PRNG, no external deps).
2. Compute ground truth by simulating the loop directly —
   enumerate IVs, compute locations, check for collisions.
3. Run `decide()`.
4. Compare verdicts (`Exact` / `None` / `Unknown`). `Unknown` is
   acceptable (conservative), a wrong `Exact` or `None` is not.
5. Assert `unsound == 0`, print failing examples.

All fuzz tests are in the level's `#[cfg(test)] mod tests` block
and run on every `cargo test --lib`.
