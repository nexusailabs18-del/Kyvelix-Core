
# PHASE 7 — Points-to / Aliasing (Ch. 8.2)

## 7.0 What Phase 7 is

**Input:** CFG (Phase 3), FUD chains (Phase 3), region summaries (Phase 6), storage association (Phase 5.8), and the existing M0–M12 levels reasoning about named arrays `A[expr]`.

**Output:** a points-to graph that lets `A[p]` and `A[q]` be classified as **MustAlias**, **MayAlias**, or **NoAlias**, plus pointer-arithmetic reconstruction that turns `*(p + i*k + c)` back into `A[i*k + c]` for the inner loop.

**One-line goal:** make `A[p]` where `p` is a pointer variable analyze as if it were `A[expr]` with `expr` recoverable — so M0–M12 apply unchanged.

**What Phase 7 does NOT do:** interprocedural (Phase 8), frontend (Phase 9).

**Reference:** Wolfe §8.2. All content below maps to §8.2.1 (points-to relations), §8.2.2 (array pointers), §8.2.3 (pointer arithmetic), §8.2.4 (allocation dependence).

---

## 7.1 Module structure

```
src/points/
├── mod.rs              (~200)  — PointsToGraph type, public API
├── relation.rs         (~700)  — (p, q, d) triples, definiteness lattice
├── transfer.rs         (~500)  — assignment effects (4 cases from §8.2.1)
├── fixpoint.rs         (~300)  — iterate over CFG to fixpoint
├── dopvec.rs           (~500)  — Fortran 90 dope vectors, array pointers
├── ptrarith.rs         (~600)  — IV-based pointer reconstruction
├── allocdep.rs         (~400)  — allocation dependence (Fortran 90 vs C)
├── alias.rs            (~400)  — MustAlias / MayAlias / NoAlias query
└── certificate.rs      (~500)  — AliasCertificate + replay

tests/phase7/
├── features.rs         (~500)
├── fuzz.rs             (~500)   — brute-force alias sets
└── e2e.rs              (~250)

Total: ~5,350 lines
```

---

## 7.2 Points-to relation type (`relation.rs`)

**Wolfe §8.2.1:**

> "A triple (p, q, d): p = pointer, q = target, d = definiteness (D definite, P possible)."

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Definiteness { Definite, Possible }

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct PtrRelation {
    pub pointer: VarId,
    pub target:  VarId,   // may be a synthetic "unknown" target
    pub def:     Definiteness,
}

pub struct PointsToGraph {
    /// Sparse adjacency: pointer → set of (target, def)
    pub by_pointer: HashMap<VarId, BTreeSet<(VarId, Definiteness)>>,
    /// Reverse index: target → set of pointers pointing to it
    pub by_target:  HashMap<VarId, BTreeSet<(VarId, Definiteness)>>,
    /// The synthetic "unknown" target for unresolved pointers
    pub unknown:    VarId,
    /// Iteration count for debugging
    pub iters:      usize,
}

/// Query: does p definitely point only to q?
pub fn must_point_to(g: &PointsToGraph, p: VarId, q: VarId) -> bool;

/// Query: can p point to q?
pub fn may_point_to(g: &PointsToGraph, p: VarId, q: VarId) -> bool;

/// Query: can p and q point to the same variable?
pub fn may_alias(g: &PointsToGraph, p: VarId, q: VarId) -> Alias;
```

Definiteness lattice:

```
  Possible  ≤  Definite
      \         /
       \       /
        bottom (no relation)
```

`meet(Definite, Possible) = Possible`. `join(Definite, Definite) = Definite`. `join(Possible, *) = Possible`.

---

## 7.3 Transfer functions (`transfer.rs`)

**Wolfe §8.2.1** — four cases for assignment:

> 1. LHS definite target: kills previous relations for that target.
> 2. LHS possible targets: change definite to possible for all targets.
> 3. Both RHS and LHS definite: add definite relation.
> 4. Otherwise: add possible relations from all LHS targets to all RHS targets.

```rust
pub enum Effect {
    /// `*p = *q` with p definite
    DefiniteLhs { lhs: VarId, rhs: VarId },
    /// `*p = *q` with p possible or unknown
    PossibleLhs { lhs_targets: Vec<VarId>, rhs_targets: Vec<VarId> },
    /// `p = q` (pointer assignment, not deref)
    PointerAssign { lhs: VarId, rhs: VarId },
    /// `p = &x`
    AddressOf { lhs: VarId, target: VarId },
    /// `p = null` or `p = unknown`
    Kill { lhs: VarId },
    /// call / unknown effect
    Clobber,
}

pub fn apply_effect(g: &mut PointsToGraph, e: &Effect);
```

The four cases in code:

```rust
pub fn apply_effect(g: &mut PointsToGraph, e: &Effect) {
    match e {
        Effect::DefiniteLhs { lhs, rhs } => {
            // Case 1: kill all previous relations to *lhs, then add definite
            // Wolfe §8.2.1 case 1: "LHS definite target: kills previous relations for that target."
            g.kill_targets_of(lhs);
            g.add_relation(*lhs, *rhs, Definiteness::Definite);
        }
        Effect::PossibleLhs { lhs_targets, rhs_targets } => {
            // Case 2: change every definite to possible for all lhs targets
            for &t in lhs_targets {
                g.weaken_to_possible(t);
            }
            // Case 4: add possible relations from all lhs to all rhs
            for &lt in lhs_targets {
                for &rt in rhs_targets {
                    g.add_relation(lt, rt, Definiteness::Possible);
                }
            }
        }
        Effect::PointerAssign { lhs, rhs } => {
            // Case 3 if both are definite, else possible
            let r_rels = g.relations_of(*rhs).clone();
            g.kill_targets_of(lhs);
            for (target, d) in r_rels {
                g.add_relation(*lhs, target, d);
            }
        }
        Effect::AddressOf { lhs, target } => {
            g.kill_targets_of(lhs);
            g.add_relation(*lhs, *target, Definiteness::Definite);
        }
        Effect::Kill { lhs } => { g.kill_targets_of(lhs); }
        Effect::Clobber => { g.clobber_all(); }
    }
}
```

---

## 7.4 Fixpoint iteration (`fixpoint.rs`)

**Wolfe §8.2.1:**

> "Merge at CFG merge points: change definite to possible on conflicts. Loops: iterate until fixpoint."

```rust
pub fn analyze(cfg: &Cfg, fns: &FunctionTable) -> PointsToGraph {
    let mut g = PointsToGraph::empty(cfg);

    // Initial: all params and locals point to unknown (Possible).
    for v in cfg.all_vars() {
        g.add_relation(v, g.unknown, Definiteness::Possible);
    }

    let mut changed = true;
    let mut iters = 0;
    while changed && iters < MAX_ITERS {
        changed = false;
        iters += 1;

        // Traverse blocks in RPO
        for block in cfg.rpo_blocks() {
            // Meet predecessors' graphs into this block
            for pred in block.preds() {
                changed |= meet_into(&mut g, &pred.out_graph);
            }
            // Apply transfer function for each statement in block
            for stmt in block.stmts() {
                let e = extract_effect(stmt, &g, fns);
                apply_effect(&mut g, &e);
                // Note: we don't need to detect change from apply_effect
                // because monotonicity means the graph only grows.
            }
            block.out_graph = g.clone();
        }
    }
    g.iters = iters;
    g
}

/// Meet two points-to graphs. Definite + Possible on same target → Possible.
fn meet_into(dst: &mut PointsToGraph, src: &PointsToGraph) -> bool {
    let mut changed = false;
    for (p, rels) in &src.by_pointer {
        for (t, d) in rels {
            if dst.add_relation_meet(*p, *t, *d) { changed = true; }
        }
    }
    changed
}
```

**Termination:** Definiteness lattice has height 2. Every relation can only weaken (Definite → Possible) or be added. Finite lattice × finite relations = finite iterations. Cap at `MAX_ITERS = 100` for safety.

---

## 7.5 Array pointers / dope vectors (`dopvec.rs`)

**Wolfe §8.2.2:**

> "Fortran 90 array pointers: dope vectors. Dope vector fields: address, lower/upper bounds, strides for each dimension. Simple points-to keeps target array name; more precise keeps dope vector info."

```rust
pub struct DopeVector {
    /// Base address (as a symbolic expression or a VarId if known)
    pub address:   VarId,
    /// For each dimension: (lower, upper, stride). Symbols allowed.
    pub bounds:    Vec<DimBound>,
    /// Total element size
    pub elem_size: usize,
    /// Rank
    pub rank:      usize,
}

pub struct DimBound {
    pub lower:  SymExpr,
    pub upper:  SymExpr,
    pub stride: i64,
}

/// Extend PointsToGraph so that array pointers point to (base, dope vector)
/// rather than just a named array.
pub struct ArrayPointsTo {
    /// For each array-pointer variable, the target array and (optionally)
    /// the dope vector.
    pub map: HashMap<VarId, ArrayTarget>,
}

pub enum ArrayTarget {
    /// Coarse: "p points to array A" (no offset info)
    Named { array: VarId },
    /// Fine: "p points to A with dope vector dv"
    WithDope { array: VarId, dv: DopeVector },
}
```

**When `A[p]` is analyzed:**
1. Look up `p` in `ArrayPointsTo`. If `Named { array: A }` → treat as `A[0]` (worst case). If `WithDope { array: A, dv }` → substitute the dope vector's stride and offset.
2. For M0–M12 to apply, `A[p]` must be rewritten as `A[expr]` where `expr` is affine. The dope vector gives `expr = addr_expr + Σ dim_k · stride_k`.

**Certificate:** the dope vector itself, with a check that `addr_expr` and each `stride_k` are constant or parametric.

---

## 7.6 Pointer arithmetic (`ptrarith.rs`)

**Wolfe §8.2.3:**

> "C arrays defined in terms of address arithmetic. Extend induction variable analysis to discover starting point, increment, range of pointer (induction variable of loop). Can rebuild array references from pointer references for inner loops."

**This is the big win.** In C, `for (p = A; p < A + n; p++) sum += *p;` compiles to pointer arithmetic — but if we recover the IV, it becomes `for (i = 0; i < n; i++) sum += A[i];`, and M0 applies.

**Algorithm `ReconstructArrayRef`:**

```
input: loop L, pointer p incremented in L
output: array A, affine index expression `a*i + b` such that *p ≡ A[a*i + b]

1. Run IV analysis (Phase 4's Algorithm Induct) on the pointer p.
   Result: p has form p₀ + s·i where i is the loop IV, s is the stride.

2. Find the base array:
   - If p₀ = &A[0], then p = A + s·i (using pointer arithmetic).
   - If p₀ = A + c, then p = A + (c + s·i).
   - Otherwise: fail (pointer derived from unknown expression).

3. Rewrite *p as A[c + s·i].

4. Verify: element size matches the array's element type.
```

```rust
pub struct ReconstructedRef {
    pub array:  VarId,
    pub coeff:  i64,        // s
    pub offset: i64,        // c
    pub elem_size: usize,
}

pub fn reconstruct(
    loop_: &Loop,
    ptr: VarId,
    ivs: &[IvInfo],
    pts: &ArrayPointsTo,
) -> Option<ReconstructedRef>;
```

**When it fires:** inner loops where the pointer's IV has constant stride and the base points to a named array. When it doesn't fire (pointer derived from a function call, cast, or aliased base), fall back to `MayAlias`.

**Certificate:** an `AliasCertificate` with `ReconstructedRef { array, coeff, offset, elem_size }`. The checker verifies that the loop IV increment matches the pointer increment (replays the pointer's IV computation).

---

## 7.7 Allocation dependence (`allocdep.rs`)

**Wolfe §8.2.4:**

> "Two dynamic allocation statements: order matters for memory addresses. Fortran 90: no syntax to get address of pointer target; location independence assured. C: can access/modify addresses; if order important, compiler must preserve; insert dependence relations between allocation statements."

```rust
pub enum Language {
    Fortran90,   // no address escape → allocation order independent
    C,           // address escape allowed → must preserve order if observable
}

pub fn allocation_dependence(
    lang: Language,
    alloc1: &AllocStmt,
    alloc2: &AllocStmt,
    pts: &PointsToGraph,
) -> Dependence {
    match lang {
        Language::Fortran90 => Dependence::None,
        Language::C => {
            // If any pointer to alloc1's result escapes (is used in
            // address arithmetic or passed to a non-pure function),
            // the order matters.
            if pts.escapes(alloc1.result) || pts.escapes(alloc2.result) {
                Dependence::Exact(vec![])   // loop-independent dependence
            } else {
                Dependence::None
            }
        }
    }
}
```

---

## 7.8 Alias certificate (`certificate.rs`)

Extend `Claim` with:

```rust
pub enum Claim {
    // ... existing ...
    Alias(AliasClaim),
}

pub enum AliasClaim {
    /// p definitely points to (array A, index expr e)
    MustAlias { pointer: VarId, array: VarId, index: SymExpr },
    /// p and q may point to the same location
    MayAlias { p: VarId, q: VarId },
    /// p and q point to disjoint locations
    NoAlias { p: VarId, q: VarId },
    /// Two array accesses A[e1] and A[e2] are disjoint
    DisjointArrays { a: VarId, e1: SymExpr, a2: VarId, e2: SymExpr },
}

pub enum Witness {
    // ... existing ...
    AliasWitness {
        /// The resolved points-to relation
        relation: PtrRelation,
        /// The dope vector or reconstructed ref used to derive it
        basis: AliasBasis,
    },
}

pub enum AliasBasis {
    DirectPointsTo(PtrRelation),
    ReconstructedRef(ReconstructedRef),
    AllocationOrder { first: StmtId, second: StmtId },
}
```

**Checker (`m0-check`) new arm:**

```rust
fn check_alias_witness(claim: &AliasClaim, w: &Witness, env: &Env) -> Result<()> {
    match (claim, w) {
        (AliasClaim::MustAlias { pointer, array, index }, 
         Witness::AliasWitness { relation, basis: AliasBasis::DirectPointsTo(r) }) => {
            if r.pointer != *pointer { return Err("pointer mismatch"); }
            if r.target  != *array   { return Err("target mismatch"); }
            if r.def != Definiteness::Definite { return Err("not definite"); }
            // Verify the index expression matches the array's dope vector
            verify_index(index, array, env)?;
            Ok(())
        }
        (AliasClaim::NoAlias { p, q },
         Witness::AliasWitness { basis: AliasBasis::ReconstructedRef(r), .. }) => {
            // Both p and q must reconstruct to distinct (array, coeff, offset)
            verify_reconstruction(*p, r)?;
            verify_reconstruction(*q, r)?;
            Ok(())
        }
        // ... other arms
        _ => Err("claim/witness mismatch"),
    }
}
```

---

## 7.9 Fuzz (`tests/phase7/fuzz.rs`)

**Strategy:** brute-force alias sets on synthetic programs.

1. Generate a synthetic C program: `int A[N]; int *p, *q; p = &A[0]; q = &A[k]; ...`
2. Run Phase 7 → get points-to graph → get alias set.
3. Run a concrete interpreter on the same program with a symbolic address model → get exact alias set.
4. Compare.

Also seed-and-revert: seed a bug (e.g. skip case 2 in `apply_effect`) → confirm fuzzer catches it → revert → 0 violations.

---

## 7.10 Phase 7 summary

| Item | Lines |
|---|---|
| `relation.rs` | 700 |
| `transfer.rs` | 500 |
| `fixpoint.rs` | 300 |
| `dopvec.rs` | 500 |
| `ptrarith.rs` | 600 |
| `allocdep.rs` | 400 |
| `alias.rs` | 400 |
| `certificate.rs` | 500 |
| Tests (features + fuzz + e2e) | 1,250 |
| `mod.rs` + glue | 200 |
| **Total** | **~5,350** |

**Deliverable:** `A[p]` classified as Must/May/NoAlias. Pointer arithmetic reconstructed to affine array references. Runtime checks where reconstruction fails.

---

# PHASE 8 — Interprocedural (Ch. 8.5)

## 8.0 What Phase 8 is

**Input:** single-procedure summaries from Phases 3–7 (FUD chains, region summaries, points-to, effect summaries).

**Output:** whole-program call graph, bottom-up interprocedural summaries, and top-down context-sensitive refinement.

**One-line goal:** a loop calling `helper()` no longer collapses to `MAY_DEPEND`. The callee's effect is composed with the caller's access patterns.

**Reference:** Wolfe §8.5, plus §8.3 (I/O) and §8.4 (procedure call classification), which are prerequisites.

**What Phase 8 does NOT do:** frontend (Phase 9). It assumes the CFG, FUD chains, and summaries already exist.

---

## 8.1 Module structure

```
src/ipa/
├── mod.rs              (~200)  — public API
├── callgraph.rs        (~800)  — triplet ⟨q, k, p⟩ construction
├── pure.rs             (~200)  — pure-function classification (Ch. 8.4)
├── io_effects.rs       (~150)  — I/O dependence summary (Ch. 8.3)
├── mod_analysis.rs     (~700)  — interprocedural MOD (binding graph, SCC)
├── constprop.rs        (~600)  — interprocedural constant propagation (jump functions)
├── summaries.rs        (~800)  — bottom-up summary computation
├── context.rs          (~600)  — top-down context refinement
├── compose.rs          (~500)  — compose summaries at call sites
├── certificate.rs      (~600)  — InterproceduralCertificate + replay
└── mod.rs              (~200)

tests/phase8/
├── features.rs         (~700)
├── fuzz.rs             (~900)
└── e2e.rs              (~400)

Total: ~5,600 lines
```

---

## 8.2 Call graph construction (`callgraph.rs`)

**Wolfe §8.5.1:**

> "A triplet ⟨q, k, p⟩: q passed at parameter position k to procedure p."

Three cases when processing:

> 1. Call site doesn't invoke f_k, f_k not argument: skip.
> 2. Call site doesn't invoke f_k, f_k appears as argument position m:
>    add ⟨q, m, a⟩ for actual procedure a, or for a ∈ BoundTo(p, j)
>    if invoking formal f_j.
> 3. Call site invokes f_k:
>    add edge p → q;
>    add ⟨a, m, q⟩ for actual procedures/formal arguments.

```rust
pub struct CallGraph {
    /// Edge p → q for each call from p to q
    pub edges: HashMap<ProcId, BTreeSet<ProcId>>,
    /// Triple ⟨q, k, p⟩: q is passed at position k to procedure p
    pub arg_flow: BTreeSet<(ProcId, usize, ProcId)>,
    /// Reverse edges
    pub callers: HashMap<ProcId, BTreeSet<ProcId>>,
}

pub fn build(cfg: &Cfg, funcs: &FunctionTable) -> CallGraph {
    let mut cg = CallGraph::default();
    let mut worklist: Vec<(ProcId, usize, ProcId)> = 
        collect_direct_triples(funcs);
    
    while let Some((q, k, p)) = worklist.pop() {
        if !cg.arg_flow.insert((q, k, p)) { continue; }
        // Case 3: p invokes q at position k
        cg.edges.entry(p).or_default().insert(q);
        cg.callers.entry(q).or_default().insert(p);
        
        // Case 2: q appears as argument in p's body at position m
        for (m, called_proc) in called_with_arg(funcs, p, q) {
            // If called_proc is a formal bound to some actual proc a
            for a in bound_to(funcs, p, called_proc) {
                worklist.push((a, m, called_proc));
            }
        }
    }
    
    cg
}
```

**Termination:** `arg_flow` is a monotonic set of triples; each triple added once. Finite.

**Indirect calls:** if `p()` is called through a function pointer, the case 3 triples are added for every possible target from points-to (Phase 7).

---

## 8.3 Pure-function classification (`pure.rs`)

**Wolfe §8.4:**

> "Pure function: uses but doesn't modify arguments, no globals, no I/O; return value determined by arguments. Pure subroutine: only effect is modify read-write/write-only arguments; no I/O or globals; can call only pure procedures."

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcPurity {
    Pure,           // no side effects at all
    PureSubroutine, // only modifies its read-write args
    Impure,         // has I/O, globals, or calls impure
}

pub fn classify(proc: ProcId, cg: &CallGraph, funcs: &FunctionTable) -> ProcPurity {
    if funcs[proc].has_io()         { return ProcPurity::Impure; }
    if funcs[proc].writes_globals() { return ProcPurity::Impure; }
    for callee in cg.edges[&proc] {
        match classify(*callee, cg, funcs) {
            ProcPurity::Impure => return ProcPurity::Impure,
            ProcPurity::PureSubroutine if modifies_caller_state(proc, *callee, funcs) =>
                return ProcPurity::Impure,
            _ => {}
        }
    }
    if funcs[proc].modifies_only_own_args() {
        ProcPurity::PureSubroutine
    } else {
        ProcPurity::Pure
    }
}
```

**Fixpoint:** SCCs of the call graph. If a cycle exists, classify the whole SCC together (mutual recursion).

---

## 8.4 I/O dependence summary (`io_effects.rs`)

**Wolfe §8.3:**

> "I/O dependence: any statement that reads/writes external device dependent on any preceding statement that may do likewise. Preserve order of open, close, rewind, status inquiry, read, write."

```rust
pub struct IoEffect {
    /// Which units are opened/closed/read/written
    pub ops: Vec<IoOp>,
}

pub enum IoOp {
    Open(UnitId), Close(UnitId), Rewind(UnitId),
    Read(UnitId), Write(UnitId), Inquire(UnitId),
}

/// Two statements are I/O-dependent if they touch the same unit and
/// either could be a read or write, in the original order.
pub fn io_dependence(a: &IoEffect, b: &IoEffect) -> Dependence;
```

**Loop interaction:** if a loop contains I/O, the loop cannot be parallelized. This is a hard rule — the effect summary records it.

---

## 8.5 Interprocedural MOD (`mod_analysis.rs`)

**Wolfe §8.5.2:**

> "Binding graph: vertices = formal parameters; edge p_x.f_i → p_y.f_j if p_x calls p_y passing f_i as reference parameter to f_j. Find SCCs. If any argument in SCC locally modified or bound to modified argument, all in SCC may be modified."

```rust
pub struct BindingGraph {
    /// For each procedure, map formal parameter name → vertex
    pub formal_to_vertex: HashMap<(ProcId, VarId), VertexId>,
    /// Edges between formal parameters across procedures
    pub edges: BTreeSet<(VertexId, VertexId)>,
    /// Procedure-level MOD set
    pub mod_sets: HashMap<ProcId, BTreeSet<VarId>>,
}

pub fn interprocedural_mod(cg: &CallGraph, funcs: &FunctionTable) -> BindingGraph {
    let mut bg = BindingGraph::default();
    
    // Build binding graph
    for (caller, callers_set) in &cg.callers {
        for callee in callers_set {
            for (arg_pos, formal_pos) in pass_through(caller, callee, funcs) {
                let v1 = bg.formal_to_vertex[&(*caller, arg_pos)];
                let v2 = bg.formal_to_vertex[&(*callee, formal_pos)];
                bg.edges.insert((v1, v2));
            }
        }
    }
    
    // Find SCCs
    let sccs = tarjan_scc(&bg.edges);
    
    // Compute local MOD for each procedure
    let mut local_mod: HashMap<ProcId, BTreeSet<VarId>> = HashMap::new();
    for proc in funcs.procs() {
        local_mod.insert(proc, local_modified_vars(proc, funcs));
    }
    
    // Propagate: SCC with any locally-modified formal → whole SCC modified
    let mut changed = true;
    while changed {
        changed = false;
        for scc in &sccs {
            let any_modified = scc.iter().any(|v| {
                let (p, x) = vertex_to_var(*v, &bg);
                local_mod[&p].contains(&x)
            });
            if any_modified {
                for v in scc {
                    let (p, x) = vertex_to_var(*v, &bg);
                    if local_mod.entry(p).or_default().insert(x) { changed = true; }
                }
            }
        }
    }
    
    bg.mod_sets = local_mod;
    bg
}
```

**Result:** for each procedure, the set of variables (globals + formals that may be modified through the call graph) reachable as `MOD`.

---

## 8.6 Interprocedural constant propagation (`constprop.rs`)

**Wolfe §8.5.3:**

> "Jump functions: J(q, j) = c (Const), J(q, j) = p.i + c (Function), J(q, j) = ⊥ (NonConst)."

```rust
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JumpFn {
    Const(i64),
    Function { formal: VarId, coeff: i64, offset: i64 },
    NonConst,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AbstractValue {
    Top,           // no info yet
    Const(i64),
    NonConst,      // ⊥ in Wolfe's notation
}

pub fn meet(a: AbstractValue, b: AbstractValue) -> AbstractValue {
    match (a, b) {
        (AbstractValue::Top, x) => x,
        (x, AbstractValue::Top) => x,
        (AbstractValue::Const(c1), AbstractValue::Const(c2)) if c1 == c2 => 
            AbstractValue::Const(c1),
        (AbstractValue::NonConst, _) | (_, AbstractValue::NonConst) => 
            AbstractValue::NonConst,
        _ => AbstractValue::NonConst,
    }
}

/// For each procedure and each formal parameter, compute the value lattice.
pub fn analyze(cg: &CallGraph, funcs: &FunctionTable, entry_args: &[(ProcId, Vec<AbstractValue>)]) -> HashMap<(ProcId, usize), AbstractValue> {
    let mut values: HashMap<(ProcId, usize), AbstractValue> = HashMap::new();
    let mut worklist: Vec<(ProcId, usize, AbstractValue)> = entry_args
        .iter()
        .flat_map(|(p, args)| args.iter().enumerate().map(|(i, v)| (*p, i, v.clone())))
        .collect();
    
    while let Some((p, j, v)) = worklist.pop() {
        let old = values.entry((p, j)).or_insert(AbstractValue::Top).clone();
        let new = meet(old, v);
        if new != values[&(p, j)] {
            values.insert((p, j), new);
            // Propagate to callees
            for callee in cg.edges.get(&p).into_iter().flatten() {
                for (jump_target, jump_fn) in funcs.calls_with_jumps(&p, callee) {
                    let out = apply_jump(&jump_fn, &values);
                    worklist.push((callee, jump_target, out));
                }
            }
        }
    }
    
    values
}
```

**Result:** for each procedure, each formal parameter's value lattice. Used to specialize `x = pure_fn(3)` at the call site when `pure_fn`'s parameter is a formal.

---

## 8.7 Bottom-up summaries (`summaries.rs`)

**Wolfe §8.5 implicitly.** A summary is:

```rust
pub struct ProcSummary {
    pub proc:          ProcId,
    pub purity:        ProcPurity,
    /// Which formal parameters and globals are read
    pub read_set:      BTreeSet<Symbol>,
    /// Which formal parameters and globals are written
    pub write_set:     BTreeSet<Symbol>,
    /// Array regions (from Phase 6) for each parameter/global
    pub regions:       HashMap<Symbol, RegionSummary>,
    /// Points-to effects (from Phase 7)
    pub pts_effects:   Vec<PointsToEffect>,
    /// Constant propagation lattice (from 8.6)
    pub const_lattice: HashMap<(ProcId, usize), AbstractValue>,
    /// I/O effects (from 8.4)
    pub io_effects:    Vec<IoOp>,
    /// Which callees (transitively) are called
    pub callees:       BTreeSet<ProcId>,
    /// Depth in the call graph (for scheduling)
    pub depth:         usize,
}
```

**Bottom-up computation:**

```
1. Topologically sort procedures by call graph (SCC condensation).
2. Process in reverse topological order (callees before callers).
3. For each procedure, compute:
   - Local read/write sets
   - Local region summaries
   - Local points-to effects
   - Meet with callees' summaries, composed at call sites
```

```rust
pub fn compute_summaries(cg: &CallGraph, funcs: &FunctionTable, regions: &RegionAnalysis, pts: &PointsToGraph) -> HashMap<ProcId, ProcSummary> {
    let order = topo_sort_condensation(cg);
    let mut summaries: HashMap<ProcId, ProcSummary> = HashMap::new();
    
    for scc in order.iter().rev() {
        for proc in scc {
            let mut summary = local_summary(*proc, funcs, regions, pts);
            
            // Compose callees' summaries at each call site
            for callee in cg.edges.get(proc).into_iter().flatten() {
                let callee_summary = &summaries[callee];
                let composed = compose_at_call_site(&summary, callee_summary, proc, *callee, funcs);
                summary = merge(summary, composed);
            }
            
            // For SCCs, iterate to fixpoint
            summaries.insert(*proc, summary);
        }
        if scc.len() > 1 {
            // Mutual recursion: iterate this SCC to fixpoint
            iterate_scc_fixpoint(scc, &mut summaries, cg, funcs, regions, pts);
        }
    }
    summaries
}
```

---

## 8.8 Top-down context refinement (`context.rs`)

**Why:** bottom-up summaries are context-insensitive. If `helper()` is called from 50 sites with different argument shapes, the bottom-up summary is the union. Top-down cloning specializes each call site.

**Approach:** clone a procedure's summary for each distinct context (up to a cap, e.g. 8 contexts per procedure).

```rust
pub struct Context {
    pub caller:  ProcId,
    pub arg_shapes: Vec<ArgShape>,
}

pub enum ArgShape {
    ScalarConst(i64),
    ArrayRegion(RegionSummary),
    PointsTo(PointsToGraph),
    Unknown,
}

pub fn refine(
    cg: &CallGraph,
    summaries: &HashMap<ProcId, ProcSummary>,
    entry: ProcId,
) -> HashMap<(ProcId, Context), ProcSummary> {
    let mut refined: HashMap<(ProcId, Context), ProcSummary> = HashMap::new();
    let mut worklist = vec![(entry, Context::root())];
    
    while let Some((proc, ctx)) = worklist.pop() {
        if refined.contains_key(&(proc, ctx.clone())) { continue; }
        if count_contexts(&refined, proc) > MAX_CONTEXTS { continue; }
        
        let base = &summaries[&proc];
        let specialized = specialize(base, &ctx);
        refined.insert((proc, ctx.clone()), specialized);
        
        for callee in cg.edges.get(&proc).into_iter().flatten() {
            let callee_ctx = derive_context(&ctx, proc, *callee, &specialized);
            worklist.push((*callee, callee_ctx));
        }
    }
    
    refined
}
```

**Cap:** `MAX_CONTEXTS = 8`. Once exceeded, fall back to bottom-up summary.

---

## 8.9 Compose summaries at call sites (`compose.rs`)

```rust
pub fn compose_at_call_site(
    caller_summary: &ProcSummary,
    callee_summary: &ProcSummary,
    caller: ProcId,
    callee: ProcId,
    funcs: &FunctionTable,
) -> ProcSummary {
    let mut composed = caller_summary.clone();
    
    // For each call site in caller that invokes callee:
    for call_site in funcs.call_sites(caller, callee) {
        // Map callee's formal read/write set to caller's actual args
        let arg_map = build_arg_map(call_site, callee, funcs);
        
        for sym in &callee_summary.read_set {
            let mapped = arg_map.map(sym);
            composed.read_set.insert(mapped);
        }
        for sym in &callee_summary.write_set {
            let mapped = arg_map.map(sym);
            composed.write_set.insert(mapped);
        }
        for (sym, region) in &callee_summary.regions {
            let mapped = arg_map.map(sym);
            composed.regions.entry(mapped).or_default().union(region);
        }
        for op in &callee_summary.io_effects {
            composed.io_effects.push(op.clone());
        }
        // Purity: impure callee makes caller impure
        if callee_summary.purity == ProcPurity::Impure {
            composed.purity = ProcPurity::Impure;
        }
    }
    
    composed
}
```

---

## 8.10 Interprocedural certificate (`certificate.rs`)

```rust
pub enum Claim {
    // ... existing ...
    Interprocedural(InterprocClaim),
}

pub enum InterprocClaim {
    /// Procedure p has summary S
    Summary { proc: ProcId, summary: ProcSummary },
    /// Call graph has edge p → q
    CallEdge { p: ProcId, q: ProcId },
    /// Procedure p is pure / pure subroutine / impure
    Purity { proc: ProcId, purity: ProcPurity },
    /// Formal parameter i of p is modified
    ModArg { proc: ProcId, position: usize },
}

pub enum Witness {
    // ... existing ...
    InterprocWitness {
        /// The binding graph and jump functions used to derive the claim
        binding_graph: BindingGraph,
        jump_functions: Vec<JumpFn>,
        /// The SCC decomposition used
        scc: Vec<ProcId>,
    },
}
```

**Checker arm:**

```rust
fn check_interproc_witness(claim: &InterprocClaim, w: &Witness, env: &Env) -> Result<()> {
    match (claim, w) {
        (InterprocClaim::Purity { proc, purity },
         Witness::InterprocWitness { binding_graph, .. }) => {
            // Rebuild local purity of proc's body
            let local = local_purity(*proc, env)?;
            if *purity == ProcPurity::Impure && local != ProcPurity::Impure {
                return Err("impurity claimed but body is pure");
            }
            // Check that every call in proc's body goes to a proc whose
            // purity was claimed and is compatible
            for callee in env.call_edges_of(*proc) {
                let callee_purity = env.claimed_purity(callee)?;
                if callee_purity == ProcPurity::Impure && *purity != ProcPurity::Impure {
                    return Err("call to impure callee but caller claims pure");
                }
            }
            Ok(())
        }
        (InterprocClaim::ModArg { proc, position },
         Witness::InterprocWitness { binding_graph, .. }) => {
            // Verify that the formal at position is in an SCC with a locally-modified formal
            verify_scc_touches_local_mod(binding_graph, *proc, *position, env)?;
            Ok(())
        }
        // ...
    }
}
```

---

## 8.11 Fuzz (`tests/phase8/fuzz.rs`)

**Strategy:** brute-force interprocedural summary computation on synthetic programs.

1. Generate a synthetic program: `f(x) { x = 1; } g(y) { f(y); } main() { a = 0; g(a); }`.
2. Run Phase 8 → get `mod_sets`, `purity`, `read_set`, `write_set`.
3. Run a concrete interpreter on the same program → get actual modified variables.
4. Compare.

Seed-and-revert: seed a bug (e.g. skip SCC propagation in `interprocedural_mod`) → confirm fuzzer catches it → revert.

---

## 8.12 Phase 8 summary

| Item | Lines |
|---|---|
| `callgraph.rs` | 800 |
| `pure.rs` | 200 |
| `io_effects.rs` | 150 |
| `mod_analysis.rs` | 700 |
| `constprop.rs` | 600 |
| `summaries.rs` | 800 |
| `context.rs` | 600 |
| `compose.rs` | 500 |
| `certificate.rs` | 600 |
| Tests | 2,000 |
| `mod.rs` | 200 |
| **Total** | **~7,150** |

(The reference book plan estimated 5,500; the honest estimate is higher because the certificate infrastructure, context refinement, and fuzzing are all larger than sketched.)

**Deliverable:** a loop calling `helper()` gets `helper`'s effect summary, not `MAY_DEPEND`.

---

# PHASE 9 — Frontend Extensions

## 9.0 What Phase 9 is

**Input:** a JavaScript source file (or `.ky` file), plus the existing lexer/parser in `kyjit_frontend.c`.

**Output:** the full IR that M0–M12 and the SPP transformations consume — including closures, destructuring, `for-of`, object property accesses, array-of-structs, and the loop constructs the SPP engine needs.

**One-line goal:** the frontend handles the subset of JS the SPP engine can analyze. Anything outside falls back to conservative `MAY_DEPEND`.

**What Phase 9 does NOT do:** the entire JS language. Phase 9 is the **analyzable subset** — the parts UDDP + SPP can reason about.

---

## 9.1 Module structure

```
src/frontend/
├── mod.rs              (~150)
├── lexer.rs            (~400)  — extends existing lexer
├── parser.rs           (~1,200) — recursive-descent parser
├── ast.rs              (~500)  — AST types
├── lower.rs            (~800)  — AST → IR
├── closures.rs         (~500)  — closure conversion, capture analysis
├── destructuring.rs    (~400)  — destructuring patterns
├── for_of.rs           (~500)  — for-of → indexed loop
├── objects.rs          (~600)  — property accesses, shape inference
├── aostruct.rs         (~300)  — array-of-structs recognition
├── loopkinds.rs        (~300)  — loop kind classification
├── ctrl_edges.rs       (~200)  — control edges for PDG
└── fuzz.rs             (~500)  — frontend fuzzing

Total: ~6,350 lines
```

---

## 9.2 Lexer (`lexer.rs`)

Extend the existing lexer with:

- **Template literals:** `` `hello ${name}` ``
- **Regex literals:** `/pattern/flags`
- **Numeric separators:** `1_000_000`
- **Exponent notation:** `1e10`, `1.5e-3`
- **BigInt suffix:** `42n`
- **Keyword coverage:** `let`, `const`, `class`, `extends`, `super`, `import`, `export`, `yield`, `await`, `async`, `of`, `in`, `instanceof`, `typeof`, `void`, `delete`

Token types extend the existing enum. The lexer remains a single-pass character scanner.

---

## 9.3 Parser (`parser.rs`)

Recursive-descent, one function per grammar rule. Extends the existing `parse_expr` / `parse_stmt` / `parse_block`.

**New in Phase 9:**

| Construct | Parser function |
|---|---|
| `for (let i = 0; i < n; i++)` | `parse_for` |
| `for (const x of arr)` | `parse_for_of` |
| `for (const k in obj)` | `parse_for_in` |
| `while (cond)` | `parse_while` |
| `do { } while (cond)` | `parse_do_while` |
| `function name(a, b) { }` | `parse_function_decl` |
| `const f = (x) => x * 2` | `parse_arrow` |
| `{ a, b: c, ...rest }` | `parse_object_pattern` |
| `[x, y, ...rest]` | `parse_array_pattern` |
| `class C { }` | `parse_class` |
| `try { } catch (e) { } finally { }` | `parse_try` |
| `throw x` | `parse_throw` |
| `import { a } from 'm'` | `parse_import` |
| `export default x` | `parse_export` |
| `` `text ${x}` `` | `parse_template` |

**AST shape:**

```rust
pub enum Stmt {
    Expr(Expr),
    Let { name: String, init: Option<Expr> },
    Const { name: String, init: Expr },
    Var { name: String, init: Option<Expr> },
    If { cond: Expr, then: Box<Stmt>, else_: Option<Box<Stmt>> },
    While { cond: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, cond: Expr },
    For { init: Option<Box<Stmt>>, cond: Option<Expr>, step: Option<Expr>, body: Box<Stmt> },
    ForOf { binding: Pattern, iter: Expr, body: Box<Stmt> },
    ForIn { binding: Pattern, obj: Expr, body: Box<Stmt> },
    Return(Option<Expr>),
    Break(Option<String>),
    Continue(Option<String>),
    Block(Vec<Stmt>),
    FunctionDecl { name: String, params: Vec<Pattern>, body: Vec<Stmt>, is_async: bool },
    ClassDecl { name: String, extends: Option<Expr>, body: Vec<ClassMember> },
    Try { body: Vec<Stmt>, catch: Option<(Pattern, Vec<Stmt>)>, finally: Option<Vec<Stmt>> },
    Throw(Expr),
    Import { .. },
    Export { .. },
}

pub enum Expr {
    Literal(Literal),
    Ident(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Function { params: Vec<Pattern>, body: Vec<Stmt>, is_async: bool },
    Arrow { params: Vec<Pattern>, body: ArrowBody },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    New { callee: Box<Expr>, args: Vec<Expr> },
    Member { obj: Box<Expr>, prop: Box<Expr>, computed: bool },
    Index { obj: Box<Expr>, index: Box<Expr> },
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnOp, arg: Box<Expr> },
    Assign { op: AssignOp, target: Box<Expr>, value: Box<Expr> },
    Conditional { cond: Box<Expr>, then: Box<Expr>, else_: Box<Expr> },
    Template { parts: Vec<TemplatePart> },
    Spread(Box<Expr>),
    Yield(Option<Box<Expr>>),
    Await(Box<Expr>),
    This,
    Super,
}
```

---

## 9.4 AST → IR lowering (`lower.rs`)

The critical transformation. Every source construct becomes IR nodes M0–M12 can reason about.

**Key lowerings:**

| Source | IR |
|---|---|
| `A[i]` (array literal read) | `LoadElement(A, i)` with `index_kind = Affine` |
| `A[i] = x` | `StoreElement(A, i, x)` |
| `obj.field` | `LoadField(obj, offset)` with shape check (Phase 4's IC) |
| `obj.field = x` | `StoreField(obj, offset, x)` |
| `obj[expr]` (computed) | dynamic property access — falls back to `MayAlias` unless shape-typed |
| `f(x, y)` | `Call(f, [x, y])` |
| `new C()` | `New(C)` + `Call(C, [])` |
| `a + b` | `Add(a, b)` with dynamic type dispatch |
| `a === b` | `CmpEq(a, b)` |
| `x && y` | short-circuit: `If(x, y, x)` |
| `x \|\| y` | short-circuit: `If(x, x, y)` |
| `x ?? y` | `If(x != null, x, y)` |
| `cond ? a : b` | `If(cond, a, b)` |
| `for (init; cond; step) body` | `init; header: If(cond, body, exit); body; step; Jump(header)` |
| `for (x of arr)` | expand to indexed loop (see §9.6) |

**For the SPP engine, the important thing is:** every loop becomes a canonical `KyLoop` with `header`, `latch`, `body`, `preheader`, and known IV structure. Every array access becomes `A[affine_expr]` or `A[non_affine]` (tagged for M6+).

---

## 9.5 Closures (`closures.rs`)

**Closure conversion** is the standard lambda-lifting transformation:

```
function outer(x) {
    return function inner(y) { return x + y; };
}
```
becomes:
```
function outer(x) {
    let __env = { x: x };   // captured environment
    return __make_inner(__env);
}
function __make_inner(__env) {
    return function inner(y) { return __env.x + y; };
}
```

**Capture analysis:**

1. For each closure, find free variables (variables used but not defined locally).
2. Classify each free variable as:
   - **Immutable** → capture by value (safe to analyze)
   - **Mutable but not escaped** → capture by value, track updates
   - **Mutable and escaped** → capture by reference (conservative; blocks analysis)

```rust
pub struct ClosureInfo {
    pub params:      Vec<Pattern>,
    pub free_vars:   Vec<(String, CaptureKind)>,
    pub body:        Vec<Stmt>,
}

pub enum CaptureKind {
    ByValue,           // safe for SPP
    ByValueMutable,    // value tracked; updates propagated
    ByReference,       // conservative; disables loop analysis
}
```

**Loop interaction:** if a closure captures a loop IV by reference and mutates it, the loop is `Impure` and cannot be parallelized. The effect summary records this.

---

## 9.6 Destructuring (`destructuring.rs`)

`const { a, b: c, ...rest } = obj;` → sequential property loads:

```
a = obj.a;
c = obj.b;
rest = { ...obj minus a, b };
```

`const [x, y, ...rest] = arr;` → sequential indexed loads:

```
x = arr[0];
y = arr[1];
rest = arr[2..];
```

**For SPP:** destructuring is unrolled. Each property/index load becomes a separate IR node. Aliasing is tracked per property. This preserves the M0–M12 analysis.

---

## 9.7 `for-of` (`for_of.rs`)

`for (const x of arr) body;` → indexed loop:

```
let __iter = __get_iterator(arr);
let __done = false;
while (!__done) {
    let __step = __iter.next();
    __done = __step.done;
    if (!__done) {
        const x = __step.value;
        body;
    }
}
```

**Optimization:** if `arr` is a statically-known array (via points-to, Phase 7), skip the iterator and use:

```
for (let __i = 0; __i < arr.length; __i++) {
    const x = arr[__i];
    body;
}
```

This is the SPP-analyzable form. Falls back to iterator form if not provably an array.

---

## 9.8 Object property accesses (`objects.rs`)

Property accesses become `LoadField` / `StoreField` with static offsets from shape inference:

```
obj.x   →   LoadField(obj, offset_of_x_in_shape(obj))
obj.x = v   →   StoreField(obj, offset_of_x_in_shape(obj), v)
```

**Shape inference at IR level:**

1. Run static shape analysis (Decision 2 from the earlier design) on all object literals.
2. Assign each literal a shape.
3. Monomorphize: for each access `obj.x`, look up `obj`'s shape and find the offset.
4. Emit a shape guard if the shape cannot be proven unique.

**Loop interaction:** if `obj` is a loop-carried variable whose shape changes, the loop is `Impure`. Otherwise, `LoadField` is a pure read.

---

## 9.9 Array-of-structs (`aostruct.rs`)

Recognize `struct Point { x, y, z }` and `Point[]`:

```
for (i = 0; i < n; i++) {
    sum += points[i].x + points[i].y + points[i].z;
}
```

**Recognition:**

1. Detect that `points` is an array whose elements all have the same shape.
2. Detect that `.x`, `.y`, `.z` are accessed in the loop.
3. Compute the element stride (from the shape's size).
4. **Rewrite** as:
   ```
   for (i = 0; i < n; i++) {
       base = points[i];
       sum += load(base + offset_x) + load(base + offset_y) + load(base + offset_z);
   }
   ```
5. For SPP, this becomes **three independent strided accesses** with stride = element size. M0 can analyze each.

---

## 9.10 Loop kind classification (`loopkinds.rs`)

Classify each loop as:

```rust
pub enum LoopKind {
    /// doall — no loop-carried dependences at all
    DoAll,
    /// dopar — only anti-dependences
    DoPar,
    /// forall — all dependences lexically forward
    ForAll,
    /// dosingle — flow dependences but no cycles
    DoSingle,
    /// sequential — has dependence cycles
    Sequential,
    /// reduction — only reduction dependences
    Reduction(ReductionOp),
    /// unknown — cannot classify
    Unknown,
}
```

This is what SPP Layer 2 consumes to decide parallelization strategy.

---

## 9.11 Control edges for PDG (`ctrl_edges.rs`)

Add control dependence edges to the IR for the full PDG:

- **Post-dominator tree** construction.
- For each branch, add control edges from the branch node to nodes in the "then" and "else" regions.
- The PDG is used by SPP Layer 2 for statement reordering (`§9.1.1`).

---

## 9.12 Frontend fuzzing (`fuzz.rs`)

**Strategy:** differential testing against Node.js.

1. Generate a random JS program in the analyzable subset.
2. Run it under Node.js → get output.
3. Run it under the KYJIT AOT compiler → get output.
4. Compare.

If they differ → bug. Also seed-and-revert for specific constructs.

---

## 9.13 Phase 9 summary

| Item | Lines |
|---|---|
| `lexer.rs` | 400 |
| `parser.rs` | 1,200 |
| `ast.rs` | 500 |
| `lower.rs` | 800 |
| `closures.rs` | 500 |
| `destructuring.rs` | 400 |
| `for_of.rs` | 500 |
| `objects.rs` | 600 |
| `aostruct.rs` | 300 |
| `loopkinds.rs` | 300 |
| `ctrl_edges.rs` | 200 |
| `fuzz.rs` | 500 |
| Tests | 1,000 |
| `mod.rs` | 150 |
| **Total** | **~7,350** |

---

## 9.14 The analyzable subset — the honest boundary

**In scope for Phase 9:**

- Numeric arithmetic on numbers
- Array indexing with affine expressions
- Object literals with fixed shape
- Property accesses with statically-known shape
- `for`, `while`, `do-while`, `for-of` (when the iterable is provably an array)
- Function declarations and calls
- Arrow functions with `ByValue` captures
- Destructuring of literals
- Basic built-ins: `Math.*`, `Array.prototype.{push,pop,length}`, `String.length`, `Number(...)`, `parseInt`, etc.

**Out of scope (falls back to `MayAlias`):**

- `eval`
- `Proxy` / `Reflect`
- Getters/setters on objects
- `with` statements
- Dynamic property names that don't statically resolve
- `Symbol`-keyed properties
- `for-in` over non-array objects
- `for-of` over non-array iterables
- Generators and `yield`
- `async` / `await`
- `try` / `catch` / `finally` (conservative — disables loop analysis inside)
- Closures with `ByReference` captures that escape

**The rule:** anything outside the analyzable subset doesn't get analyzed. It gets compiled correctly but conservatively. This is what makes the analysis sound.

---

## 9.15 Phase 9 summary table

| Phase 9 component | Lines | Unlocks |
|---|---|---|
| Lexer extensions | 400 | Template literals, regex, BigInt |
| Parser | 1,200 | All statement forms |
| AST | 500 | Type-safe tree |
| Lowering | 800 | IR from AST |
| Closures | 500 | Lambda-lifting |
| Destructuring | 400 | Pattern matching |
| For-of | 500 | Iteration protocol |
| Objects | 600 | Shape-based property access |
| Array-of-structs | 300 | Strided access recognition |
| Loop kinds | 300 | SPP parallelization decision |
| Control edges | 200 | Full PDG |
| Fuzz | 500 | Differential testing vs Node |
| Tests | 1,000 | Feature + e2e |
| mod.rs | 150 | Public API |
| **Total** | **~7,350** | |

---

# THE THREE PHASES TOGETHER

| Phase | Reference | Lines | Weeks |
|---|---|---|---|
| **7** — Points-to / aliasing | Ch. 8.2 | ~5,350 | 7 |
| **8** — Interprocedural | Ch. 8.5 (+ 8.3, 8.4) | ~7,150 | 10 |
| **9** — Frontend extensions | Ch. 9 / implicit | ~7,350 | 11.5 |
| **Total** | | **~19,850** | **~28.5 weeks** |

Multiply by 1.5–2× for reality: **~40–55 weeks** for Phases 7–9 alone.

