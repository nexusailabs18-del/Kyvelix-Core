//! `Algorithm Induct` — SCC-based induction-variable detection (Wolfe §6.5).
//!
//! Phase 4 opens by walking the Phase 3 graph. The *chain graph* has one
//! vertex — an [`OpId`] — per scalar **definition** and per **φ-term**;
//! its edges are the operator links δ: a definition points at the
//! definition (or φ-term) that reaches each variable read by its
//! right-hand side ([`crate::ir::ScalarRhs`]), and a φ-term points at the
//! definitions arriving on each incoming edge.
//!
//! A non-trivial strongly-connected component of this graph is an
//! induction variable: the header φ and the update that feeds its back
//! edge form a cycle `φ → update → φ`. Every such SCC whose cycle is a
//! pure "add a loop-invariant value" is classified `LinearIv { a, b }`
//! — the closed form `value(i) = a + b·i` — and recorded in
//! [`InductResult::ivs`].
//!
//! ## Classification rules (Wolfe §6.5.5)
//!
//! | Rule | Condition | Classification |
//! |---|---|---|
//! | 1 | literal constant | `Constant(c)` |
//! | 3 | `Var(v)` — value of reaching def | class of that def |
//! | 5 | `Add`/`Mul` of constants | `Constant` |
//! | 6 | `Add` of two linear IVs | `LinearIv` |
//! | 8 | loop-header φ (not an IV) | `Complex` |
//! | 9 | φ with identical incoming classes | that class, else `Complex` |
//! | 10 | non-trivial SCC of additions | `LinearIv` |
//!
//! ## The discipline
//!
//! Nothing here is carried in a certificate verbatim: a Phase 4 certificate
//! carries the CFG, and the checker re-runs `induct` from scratch (see the
//! Phase 4 checker arms). This module therefore has no side channel a forgery
//! could exploit.

use std::collections::HashMap;

use crate::ir::scalar::{DefId, PhiId, ScalarRhs, UseId};
use crate::ir::CFG;

use super::chain::{ChainLink, FudChain};
use super::dominators::{compute_dominators, DominatorTree};
use super::FudChains;

/// A vertex of the chain graph: the `n_defs` definitions occupy ids
/// `0..n_defs`, the φ-terms occupy `n_defs..n_defs+n_phis`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct OpId(pub usize);

impl OpId {
    pub fn index(self) -> usize {
        self.0
    }
}

/// The classification of an operation (Wolfe §6.5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Class {
    /// A literal, or an expression that folds to one.
    Constant(i64),
    /// `value(i) = a + b·i` where `i` is the enclosing induction variable.
    LinearIv { a: i64, b: i64 },
    /// A wrapping induction variable at `level` (Wolfe §6.5.6). Detected
    /// but not otherwise used in Phase 4 v1.
    WrapAround(usize),
    /// Anything not classified above.
    Complex,
}

impl Class {
    /// IV algebra: `(a + b·i) + c`, `const + const`, `iv + iv` (Wolfe §6.5.2).
    pub fn add(&self, other: &Class) -> Class {
        match (self, other) {
            (Class::Constant(a), Class::Constant(b)) => Class::Constant(a.wrapping_add(*b)),
            (Class::Constant(a), Class::LinearIv { a: b, b: c })
            | (Class::LinearIv { a: b, b: c }, Class::Constant(a)) => Class::LinearIv {
                a: a.wrapping_add(*b),
                b: *c,
            },
            (Class::LinearIv { a: a1, b: b1 }, Class::LinearIv { a: a2, b: b2 }) => {
                Class::LinearIv {
                    a: a1.wrapping_add(*a2),
                    b: b1.wrapping_add(*b2),
                }
            }
            _ => Class::Complex,
        }
    }

    /// IV algebra: `(a + b·i)·c` and `const·const` (Wolfe §6.5.2).
    pub fn mul(&self, other: &Class) -> Class {
        match (self, other) {
            (Class::Constant(a), Class::Constant(b)) => Class::Constant(a.wrapping_mul(*b)),
            (Class::Constant(c), Class::LinearIv { a, b })
            | (Class::LinearIv { a, b }, Class::Constant(c)) => Class::LinearIv {
                a: a.wrapping_mul(*c),
                b: b.wrapping_mul(*c),
            },
            _ => Class::Complex,
        }
    }
}

/// A detected induction variable: the header φ, its initial value, its step,
/// the scalar variable and the header block.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InductionVariable {
    /// The header φ-term op (a vertex of the chain graph).
    pub op: OpId,
    /// Initial value (`a` in `a + b·i`).
    pub a: i64,
    /// Step (`b` in `a + b·i`).
    pub b: i64,
    /// Scalar variable index.
    pub var: usize,
    /// The header block of the loop this IV drives.
    pub loop_: usize,
}

/// The result of [`induct`].
#[derive(Debug, Clone, Default)]
pub struct InductResult {
    /// Classification per operation.
    pub class: HashMap<OpId, Class>,
    /// SCC id per operation (Wolfe's `Lowlink`/component numbering).
    pub scc: HashMap<OpId, usize>,
    /// Detected induction variables, in header-φ order.
    pub ivs: Vec<InductionVariable>,
}

impl InductResult {
    /// The classification of a definition.
    pub fn class_of_def(&self, d: DefId, _n_defs: usize) -> Class {
        self.class
            .get(&OpId(d.index()))
            .cloned()
            .unwrap_or(Class::Complex)
    }

    /// The classification of a φ-term.
    pub fn class_of_phi(&self, p: PhiId, n_defs: usize) -> Class {
        self.class
            .get(&OpId(n_defs + p.index()))
            .cloned()
            .unwrap_or(Class::Complex)
    }

    /// The effective classification seen by a use: that of the operation its
    /// chain link points at (a def or a φ).
    pub fn class_of_use(&self, u: UseId, chain: &FudChain, n_defs: usize) -> Class {
        match chain.use_to_def.get(&u) {
            Some(ChainLink::Def(d)) => self.class_of_def(*d, n_defs),
            Some(ChainLink::Phi(p)) => self.class_of_phi(*p, n_defs),
            None => Class::Complex,
        }
    }
}

/// `Algorithm Induct`. `cfg` must be the φ-annotated CFG that `chains` was
/// built from (see [`crate::fud::build_fud_chains`]).
pub fn induct(cfg: &CFG, chains: &FudChains) -> InductResult {
    Builder::new(cfg, chains).run()
}

/// Tarjan's strongly-connected-components, iterative (no recursion depth
/// limit). `succ[v]` are the successor vertices of `v`. Returns the SCC id of
/// each vertex and the number of components.
fn tarjan(succ: &[Vec<usize>]) -> (Vec<usize>, usize) {
    let n = succ.len();
    let mut idx = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut scc = vec![usize::MAX; n];
    let mut counter = 0usize;
    let mut scc_count = 0usize;
    for s in 0..n {
        if idx[s] != usize::MAX {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(s, 0)];
        while let Some(&(v, ci)) = work.last() {
            if ci == 0 {
                idx[v] = counter;
                low[v] = counter;
                counter += 1;
                stack.push(v);
                on[v] = true;
            }
            if ci < succ[v].len() {
                let w = succ[v][ci];
                work.last_mut().unwrap().1 += 1;
                if idx[w] == usize::MAX {
                    work.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(idx[w]);
                }
            } else {
                if low[v] == idx[v] {
                    loop {
                        let w = stack.pop().unwrap();
                        on[w] = false;
                        scc[w] = scc_count;
                        if w == v {
                            break;
                        }
                    }
                    scc_count += 1;
                }
                work.pop();
                if let Some(&(p, _)) = work.last() {
                    low[p] = low[p].min(low[v]);
                }
            }
        }
    }
    (scc, scc_count)
}

/// The raw operand graph of a CFG + chain, shared by every Phase 4 algorithm
/// (Induct, ConstantProp, ScalarDD). Vertex numbering: defs `0..n_defs`, then
/// φ-terms `n_defs..n_defs+n_phis`.
pub(crate) struct OpGraph {
    /// Number of definition vertices.
    pub n_defs: usize,
    /// Number of φ vertices.
    pub n_phis: usize,
    /// Variable of each op.
    pub op_var: Vec<usize>,
    /// Block of each op.
    pub op_block: Vec<usize>,
    /// Statement index within the block (defs only; `usize::MAX` for φ).
    pub op_stmt: Vec<usize>,
    /// RHS of each definition.
    pub def_rhs: Vec<ScalarRhs>,
    /// Per definition: variable → the op reaching it at that statement.
    pub def_operand_map: Vec<HashMap<usize, Option<usize>>>,
    /// Successor (δ) links per op, deduplicated in operand order.
    pub op_operands: Vec<Vec<usize>>,
    /// Per φ-term (indexed `0..n_phis`), one entry per **predecessor block** of
    /// that φ's block, in `Block::preds` order: the op feeding that edge, or
    /// [`usize::MAX`] when the edge carries no definition (uninitialised).
    ///
    /// This is the pred-aligned companion of [`Self::op_operands`]. Deduplication
    /// is right for the *chain graph* — two edges fed by the same definition are
    /// the same vertex — but it throws away exactly the information
    /// `analyze_iv_scc` needs: which operands of a header φ arrive on the loop's
    /// **back edges** (and so contribute the step) and which arrive from outside
    /// the loop (and so contribute the initial value).
    pub phi_slots: Vec<Vec<usize>>,
    /// The op reaching each use index (`None` = uninitialized).
    pub use_reaching: Vec<Option<usize>>,
    /// (block, stmt, var) → first use index.
    pub use_lookup: HashMap<(usize, usize, usize), usize>,
}

impl OpGraph {
    /// The op (def or φ vertex) that reaches use `u`.
    pub fn reaching_op_of_use(&self, u: UseId) -> Option<usize> {
        self.use_reaching.get(u.index()).copied().flatten()
    }

    /// Is `op` a definition vertex?
    pub fn is_def(&self, op: usize) -> bool {
        op < self.n_defs
    }

    /// Is `op` a φ vertex?
    pub fn is_phi(&self, op: usize) -> bool {
        op >= self.n_defs
    }
}

/// Build the shared operand graph from a φ-annotated `cfg` and its chain.
/// The CFG **must** be the one the chain was built from (see
/// [`crate::fud::build_fud_chains`]).
pub(crate) fn build_op_graph(cfg: &CFG, chain: &FudChain) -> OpGraph {
    let ix = cfg.index();
    let n_defs = ix.defs.len();
    let n_phis = ix.phis.len();
    let n = n_defs + n_phis;

    let mut op_var = vec![0usize; n];
    let mut op_block = vec![0usize; n];
    let mut op_stmt = vec![0usize; n];
    let mut def_at: HashMap<(usize, usize, usize), usize> = HashMap::new();
    for (di, s) in ix.defs.iter().enumerate() {
        op_var[di] = s.var;
        op_block[di] = s.block;
        op_stmt[di] = s.stmt;
        def_at.insert((s.block, s.stmt, s.order), di);
    }
    for (pi, s) in ix.phis.iter().enumerate() {
        op_var[n_defs + pi] = s.var;
        op_block[n_defs + pi] = s.block;
        op_stmt[n_defs + pi] = usize::MAX;
    }

    let mut use_lookup: HashMap<(usize, usize, usize), usize> = HashMap::new();
    for (ui, s) in ix.uses.iter().enumerate() {
        use_lookup.entry((s.block, s.stmt, s.var)).or_insert(ui);
    }
    let mut use_reaching: Vec<Option<usize>> = vec![None; ix.uses.len()];
    for (ui, slot) in use_reaching.iter_mut().enumerate() {
        *slot = match chain.use_to_def.get(&UseId(ui)) {
            Some(ChainLink::Def(d)) => Some(d.index()),
            Some(ChainLink::Phi(p)) => Some(n_defs + p.index()),
            None => None,
        };
    }

    let mut def_rhs: Vec<ScalarRhs> = vec![ScalarRhs::Unknown; n_defs];
    let mut def_operand_map: Vec<HashMap<usize, Option<usize>>> = vec![HashMap::new(); n_defs];
    let mut op_operands: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, blk) in cfg.blocks.iter().enumerate() {
        for (sidx, st) in blk.stmts.iter().enumerate() {
            for d in st.defs.iter() {
                let di = match def_at.get(&(b, sidx, d.order)) {
                    Some(&i) => i,
                    None => continue,
                };
                def_rhs[di] = d.rhs.clone();
                let mut vars = Vec::new();
                d.rhs.collect_vars(&mut vars);
                let mut ops = Vec::new();
                let mut map: HashMap<usize, Option<usize>> = HashMap::new();
                for v in vars {
                    let op = use_lookup
                        .get(&(b, sidx, v))
                        .map(|&ui| use_reaching[ui])
                        .unwrap_or(None);
                    map.insert(v, op);
                    if let Some(op) = op {
                        if !ops.contains(&op) {
                            ops.push(op);
                        }
                    }
                }
                def_operand_map[di] = map;
                op_operands[di] = ops;
            }
        }
    }
    let mut phi_slots: Vec<Vec<usize>> = vec![Vec::new(); n_phis];
    for (pi, site) in ix.phis.iter().enumerate() {
        let op = n_defs + pi;
        if let Some(slots) = chain.phi_incoming.get(&PhiId(pi)) {
            let mut ops = Vec::new();
            let mut aligned = vec![usize::MAX; slots.len()];
            for (i, slot) in slots.iter().enumerate() {
                let o = match slot {
                    Some(ChainLink::Def(d)) => Some(d.index()),
                    Some(ChainLink::Phi(p)) => Some(n_defs + p.index()),
                    None => None,
                };
                aligned[i] = o.unwrap_or(usize::MAX);
                if let Some(o) = o {
                    if !ops.contains(&o) {
                        ops.push(o);
                    }
                }
            }
            debug_assert_eq!(aligned.len(), cfg.blocks[site.block].preds.len());
            op_operands[op] = ops;
            phi_slots[pi] = aligned;
        }
    }

    OpGraph {
        n_defs,
        n_phis,
        op_var,
        op_block,
        op_stmt,
        def_rhs,
        def_operand_map,
        op_operands,
        phi_slots,
        use_reaching,
        use_lookup,
    }
}

/// The chain-graph builder and classifier.
struct Builder {
    dom: DominatorTree,
    /// Predecessor blocks (for loop-header detection).
    block_preds: Vec<Vec<usize>>,
    n_defs: usize,
    /// Successor (δ) links per op, in operand order (deduplicated).
    op_operands: Vec<Vec<usize>>,
    /// variable / block / statement-index-within-block per op.
    op_var: Vec<usize>,
    op_block: Vec<usize>,
    op_stmt: Vec<usize>,
    /// RHS per definition.
    def_rhs: Vec<ScalarRhs>,
    /// Per definition: variable → the op reaching it at that statement.
    def_operand_map: Vec<HashMap<usize, Option<usize>>>,
    /// Per φ-term, the pred-aligned incoming ops (see [`OpGraph::phi_slots`]).
    phi_slots: Vec<Vec<usize>>,
    /// The op reaching each use (`None` = uninitialized).
    use_reaching: Vec<Option<usize>>,
    /// (block, stmt-index, var) → the first use of `var` at that statement.
    use_lookup: HashMap<(usize, usize, usize), usize>,
    // Tarjan result.
    scc_of: Vec<usize>,
    scc_count: usize,
    // Classification (filled SCC by SCC, sinks first).
    class: Vec<Option<Class>>,
    /// Cycle guard for on-demand classification.
    visiting: Vec<bool>,
    ivs: Vec<InductionVariable>,
}

impl Builder {
    fn new(cfg: &CFG, chains: &FudChains) -> Builder {
        let g = build_op_graph(cfg, &chains.chain);
        let n = g.op_operands.len();
        let (scc_of, scc_count) = tarjan(&g.op_operands);
        Builder {
            dom: compute_dominators(cfg),
            block_preds: cfg.blocks.iter().map(|b| b.preds.clone()).collect(),
            n_defs: g.n_defs,
            op_operands: g.op_operands,
            op_var: g.op_var,
            op_block: g.op_block,
            op_stmt: g.op_stmt,
            def_rhs: g.def_rhs,
            def_operand_map: g.def_operand_map,
            phi_slots: g.phi_slots,
            use_reaching: g.use_reaching,
            use_lookup: g.use_lookup,
            scc_of,
            scc_count,
            class: vec![None; n],
            visiting: vec![false; n],
            ivs: Vec::new(),
        }
    }
}

impl Builder {
    fn run(mut self) -> InductResult {
        // Classify SCCs in reverse-Tarjan order: Tarjan numbers components so
        // that an edge always runs from a higher-numbered component to a
        // lower-numbered one, so processing `scc_count-1 .. 0` visits sinks
        // first and every operand is classified before its consumer.
        for scc in (0..self.scc_count).rev() {
            self.classify_scc(scc);
        }
        let n = self.op_operands.len();
        let mut class = HashMap::new();
        for op in 0..n {
            if let Some(c) = &self.class[op] {
                class.insert(OpId(op), c.clone());
            }
        }
        let mut scc = HashMap::new();
        for op in 0..n {
            scc.insert(OpId(op), self.scc_of[op]);
        }
        InductResult { class, scc, ivs: self.ivs }
    }

    /// Is `op` a loop-header term? A φ whose block dominates one of its own
    /// predecessors (i.e. an incoming edge is a back edge).
    fn is_loop_header(&self, op: usize) -> bool {
        if op < self.n_defs {
            return false;
        }
        let block = self.op_block[op];
        self.block_preds[block]
            .iter()
            .any(|&p| self.dom.dominates(block, p))
    }

    /// The classification of an operation, classified on demand (fallback for
    /// operands whose SCC was already processed).
    fn classify_op(&mut self, op: usize) -> Class {
        if let Some(c) = &self.class[op] {
            return c.clone();
        }
        if self.visiting[op] {
            // A cycle reached outside the SCC order: bail out conservatively.
            return Class::Complex;
        }
        self.visiting[op] = true;
        let c = self.classify_structural(op);
        self.visiting[op] = false;
        self.class[op] = Some(c.clone());
        c
    }

    /// Classify a whole SCC, then record it if it is a linear IV.
    fn classify_scc(&mut self, scc: usize) {
        let members: Vec<usize> = (0..self.op_operands.len())
            .filter(|&op| self.scc_of[op] == scc)
            .collect();
        if members.is_empty() {
            return;
        }
        let nontrivial = members.len() > 1;
        let classes = if nontrivial {
            let classes = self.analyze_iv_scc(&members);
            classes
        } else {
            let op = members[0];
            // A self-loop `φ ← φ` is a trivial SCC; classify it structurally.
            let mut m = HashMap::new();
            m.insert(op, self.classify_structural(op));
            m
        };
        let mut iv: Option<(usize, i64, i64)> = None;
        for &op in &members {
            let c = classes.get(&op).cloned().unwrap_or(Class::Complex);
            self.class[op] = Some(c.clone());
            if let Class::LinearIv { a, b } = c {
                if self.is_loop_header(op) && iv.is_none() {
                    iv = Some((op, a, b));
                }
            }
        }
        if let Some((op, a, b)) = iv {
            self.ivs.push(InductionVariable {
                op: OpId(op),
                a,
                b,
                var: self.op_var[op],
                loop_: self.op_block[op],
            });
        }
    }

    /// Structural classification (Rules 1–9).
    fn classify_structural(&mut self, op: usize) -> Class {
        if op < self.n_defs {
            let rhs = self.def_rhs[op].clone();
            let map = self.def_operand_map[op].clone();
            self.eval_rhs(&rhs, &map)
        } else {
            self.classify_phi(op)
        }
    }

    /// Rule 1/3/5/6/7: evaluate a RHS from the classifications of the
    /// operations reaching each variable it reads.
    fn eval_rhs(&mut self, rhs: &ScalarRhs, map: &HashMap<usize, Option<usize>>) -> Class {
        match rhs {
            ScalarRhs::Const(c) => Class::Constant(*c),
            ScalarRhs::Var(v) => match map.get(v).copied().flatten() {
                Some(op) => self.classify_op(op),
                None => Class::Complex,
            },
            // An opaque function of a scalar is Complex (Rule 4/7).
            ScalarRhs::Call(_) => Class::Complex,
            ScalarRhs::Add(xs) => {
                let mut acc = Class::Constant(0);
                for x in xs {
                    let c = self.eval_rhs(x, map);
                    acc = acc.add(&c);
                }
                acc
            }
            ScalarRhs::Mul(xs) => {
                let mut acc = Class::Constant(1);
                for x in xs {
                    let c = self.eval_rhs(x, map);
                    acc = acc.mul(&c);
                }
                acc
            }
            ScalarRhs::Unknown => Class::Complex,
            // A load from memory (base[index]) cannot be classified as a simple
            // scalar recurrence — it depends on memory state.
            ScalarRhs::Load { .. } => Class::Complex,
        }
    }

    /// Rule 9: a φ is `c` when all incoming operands have class `c`, else
    /// `Complex`. A header φ reached through this path (rather than through
    /// its SCC) is Rule 8 — `Complex`.
    fn classify_phi(&mut self, op: usize) -> Class {
        let ops = self.op_operands[op].clone();
        if ops.is_empty() {
            return Class::Complex;
        }
        let mut it = ops.iter();
        let first = self.classify_op(*it.next().unwrap());
        for &o in it {
            if self.classify_op(o) != first {
                return Class::Complex;
            }
        }
        first
    }

    /// Rules 6/9/10 in one pass over a non-trivial SCC: classify the defs with
    /// the in-SCC header φ treated as the symbolic identity `i = 0 + 1·i`, then
    /// read the header φ's closed form off its **initialising** predecessors and
    /// its **back-edge** predecessors separately. Every SCC member is reported
    /// as that IV (or all `Complex` when the SCC is not a linear IV).
    fn analyze_iv_scc(&mut self, members: &[usize]) -> HashMap<usize, Class> {
        // Step 1: classify the definitions, dependency-free within the SCC.
        let mut def_cls: HashMap<usize, Class> = HashMap::new();
        for &op in members {
            if op < self.n_defs {
                let c = self.eval_def_in_scc(op, &def_cls);
                def_cls.insert(op, c);
            }
        }
        // Step 2: the SCC must contain exactly one header φ.
        let headers: Vec<usize> = members
            .iter()
            .copied()
            .filter(|&op| op >= self.n_defs && self.is_loop_header(op))
            .collect();
        let complex_all = |members: &[usize]| -> HashMap<usize, Class> {
            members.iter().map(|&m| (m, Class::Complex)).collect()
        };
        if headers.len() != 1 {
            return complex_all(members);
        }
        let h = headers[0];
        // Step 3: walk the header's incoming edges **by predecessor**, keeping
        // the back edges (the update, hence the step) apart from the edges that
        // enter the loop (the initialisation, hence the base value).
        //
        // Reading the two off the flattened, deduplicated operand list — which
        // is what an earlier revision did — conflated them: `init` and `step`
        // became whichever operand happened to come last. A header with two
        // latches stepping by different amounts therefore reported *one* of the
        // two steps, and a latch that merely re-assigns the variable could be
        // read as the base value, so the closed form claimed a `value(0)` the
        // loop never starts from.
        let slots: Vec<usize> = self.phi_slots[h - self.n_defs].clone();
        let mut init: Option<i64> = None;
        let mut step: Option<i64> = None;
        for (i, &o) in slots.iter().enumerate() {
            if o == usize::MAX {
                // An incoming edge that carries no definition at all: the merge
                // reads uninitialised memory on that path.
                return complex_all(members);
            }
            if self.is_back_edge_of(h, i) {
                // `def_cls` is expressed in the symbolic-identity convention
                // (`i = 0 + 1·i`), so a linear update `i := i + s` is exactly
                // `1·i + s`: its `a` is the step and its coefficient must be
                // `1`. Anything else — another φ, a coefficient other than `1`
                // (the geometric `i := i + i`), a definition outside the SCC
                // (a latch that *resets* `i`) — is not a linear IV.
                if !self.same_scc(o, h) || o >= self.n_defs {
                    return complex_all(members);
                }
                let s = match def_cls.get(&o) {
                    Some(Class::LinearIv { a, b: 1 }) => *a,
                    _ => return complex_all(members),
                };
                match step {
                    Some(prev) if prev != s => return complex_all(members),
                    _ => step = Some(s),
                }
            } else {
                // An entry edge. Every one must agree on the same literal, or
                // the loop starts at more than one value and no single `a`
                // describes it.
                let v = match self.classify_op(o) {
                    Class::Constant(v) => v,
                    _ => return complex_all(members),
                };
                match init {
                    Some(prev) if prev != v => return complex_all(members),
                    _ => init = Some(v),
                }
            }
        }
        match (init, step) {
            (Some(a), Some(b)) => {
                let cls = Class::LinearIv { a, b };
                members.iter().map(|&m| (m, cls.clone())).collect()
            }
            _ => complex_all(members),
        }
    }

    /// Is incoming predecessor slot `i` of header φ `h` one of the loop's back
    /// edges — i.e. does the edge `preds[i] → h.block` leave from a block that
    /// the header dominates?
    fn is_back_edge_of(&self, h: usize, i: usize) -> bool {
        let block = self.op_block[h];
        match self.block_preds[block].get(i) {
            Some(&p) => self.dom.dominates(block, p),
            None => false,
        }
    }

    /// The class of variable `v` read by def `op` *within* its SCC analysis:
    /// an in-SCC φ is the symbolic identity `0 + 1·i`; an in-SCC def uses the
    /// already computed `def_cls`; an out-of-SCC op uses its stored class.
    fn in_scc_operand_class(
        &mut self,
        op: usize,
        v: usize,
        def_cls: &HashMap<usize, Class>,
    ) -> Class {
        let o = match self.def_operand_map[op].get(&v).copied().flatten() {
            Some(o) => o,
            None => return Class::Complex,
        };
        if self.same_scc(o, op) {
            if o >= self.n_defs {
                Class::LinearIv { a: 0, b: 1 }
            } else {
                def_cls.get(&o).cloned().unwrap_or(Class::Complex)
            }
        } else {
            self.classify_op(o)
        }
    }

    /// Classify one definition of an SCC as `m := expr` with the IV algebra.
    fn eval_def_in_scc(&mut self, op: usize, def_cls: &HashMap<usize, Class>) -> Class {
        let rhs = self.def_rhs[op].clone();
        match &rhs {
            ScalarRhs::Var(v) => self.in_scc_operand_class(op, *v, def_cls),
            ScalarRhs::Add(xs) => {
                let xs = xs.clone();
                let mut acc = Class::Constant(0);
                for x in &xs {
                    let c = match x {
                        ScalarRhs::Const(c) => Class::Constant(*c),
                        ScalarRhs::Var(v) => self.in_scc_operand_class(op, *v, def_cls),
                        _ => Class::Complex,
                    };
                    acc = acc.add(&c);
                }
                acc
            }
            _ => Class::Complex,
        }
    }

    /// Do two ops share an SCC?
    fn same_scc(&self, a: usize, b: usize) -> bool {
        self.scc_of[a] == self.scc_of[b]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::build_fud_chains;
    use crate::fud::cfg::build_cfg_from_loop;
    use crate::ir::{Access, AccessKind, IndexKind, OffsetValue, IV, Loop};

    /// The Phase 9a loop CFG for a single-IV loop, body otherwise empty.
    fn iv_loop(lo: i64, hi: i64, step: i64) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", lo, hi, step)],
            body: vec![Access {
                kind: AccessKind::Write,
                base: "A".into(),
                index: vec![1],
                elem_size: 8,
                offset: OffsetValue::Const(0),
                expr: None,
                indirect: None,
                guard: None,
                range: None,
                index_kind: IndexKind::Affine,
            }],
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    /// The header φ of a loop CFG is an induction variable, and `induct` recovers
    /// the closed form `a + b·i` from the initialisation and the latch increment.
    ///
    /// This is the in-file proof that `induct` reads the CFG the Phase 9a builder
    /// emits — the one shape the frontend actually produces — rather than only
    /// the hand-built CFGs the integration tests use.
    #[test]
    fn the_loop_iv_is_recovered_as_a_linear_closed_form() {
        let l = iv_loop(3, 20, 2);
        let cfg = build_cfg_from_loop(&l).expect("an IV loop has a CFG");
        let (annot, chains) = build_fud_chains(&cfg);
        let r = induct(&annot, &chains);

        let iv = r
            .ivs
            .iter()
            .find(|iv| iv.var == 0)
            .expect("`i` is scalar 0 and is proven an IV");
        // 3, 5, 7, ...  ->  value(i) = 3 + 2·i.
        assert_eq!((iv.a, iv.b), (3, 2), "the initialisation and step are the closed form");
        assert_eq!(iv.loop_, 1, "the IV lives at the header block");
        // The same fact is recorded in the per-op classification.
        assert_eq!(
            r.class.get(&iv.op).cloned(),
            Some(Class::LinearIv { a: 3, b: 2 }),
            "the header φ's class is the same closed form"
        );
    }

    /// A loop whose IV is trivial (`step = 1`, `lower = 0`) is still an IV, so
    /// the classification is not an artifact of an unusual initial value.
    #[test]
    fn a_canonical_counting_loop_is_a_linear_iv() {
        let l = iv_loop(0, 8, 1);
        let cfg = build_cfg_from_loop(&l).expect("CFG");
        let (annot, chains) = build_fud_chains(&cfg);
        let r = induct(&annot, &chains);
        let iv = r.ivs.iter().find(|iv| iv.var == 0).expect("IV");
        assert_eq!((iv.a, iv.b), (0, 1));
    }
}