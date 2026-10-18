//! `Algorithm ConstantProp` — sparse constant propagation (Wolfe §6.6).
//!
//! The abstract lattice is the three-point chain `⊤ → Constant(c) → ⊥`:
//!
//! - `⊤` — no information yet (or an uninitialized variable),
//! - `Constant(c)` — every execution reaching this point has value `c`,
//! - `⊥` — over-defined: different executions see different values.
//!
//! `⊤` is the top and `⊥` the bottom, so a φ-term merges its incoming values
//! with `meet`, and the value at a definition is `Evaluate(rhs)` over the
//! merge of the values reaching each operand. The transfer functions are
//! monotone and the lattice has height three, so a round-robin worklist
//! reaches a fixpoint in at most `3 × |ops|` updates.
//!
//! Like every Phase 4 analysis, the checker re-derives this from the carried
//! CFG; the result is never trusted across the certificate boundary.

use std::collections::{HashMap, HashSet};

use crate::ir::scalar::ScalarRhs;
use crate::ir::CFG;

use super::induct::{build_op_graph, OpId, OpGraph};
use super::FudChains;

/// A value in the `⊤ / Constant / ⊥` lattice (Wolfe §6.6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AbsVal {
    /// Top: no information.
    Top,
    /// A single known constant.
    Const(i64),
    /// Bottom: not a constant (over-defined).
    Bottom,
}

impl AbsVal {
    /// `meet`: `⊤` is identity, `⊥` is absorbing, equal constants coincide.
    pub fn meet(&self, other: &AbsVal) -> AbsVal {
        match (self, other) {
            (AbsVal::Top, x) => *x,
            (x, AbsVal::Top) => *x,
            (AbsVal::Bottom, _) | (_, AbsVal::Bottom) => AbsVal::Bottom,
            (AbsVal::Const(a), AbsVal::Const(b)) => {
                if a == b {
                    AbsVal::Const(*a)
                } else {
                    AbsVal::Bottom
                }
            }
        }
    }
}

/// The result of [`constant_prop`].
#[derive(Debug, Clone, Default)]
pub struct ConstPropResult {
    /// The abstract value of every op (def or φ).
    pub value: HashMap<OpId, AbsVal>,
    /// The set of reachable blocks.
    pub reachable: HashSet<usize>,
}

impl ConstPropResult {
    /// The abstract value of an op (`Top` if absent).
    pub fn value_of(&self, op: OpId) -> AbsVal {
        self.value.get(&op).copied().unwrap_or(AbsVal::Top)
    }

    /// The constant of an op, when it is one.
    pub fn constant_of(&self, op: OpId) -> Option<i64> {
        match self.value.get(&op) {
            Some(AbsVal::Const(c)) => Some(*c),
            _ => None,
        }
    }
}

/// `Algorithm ConstantProp`. `cfg` must be the φ-annotated CFG that `chains`
/// was built from.
pub fn constant_prop(cfg: &CFG, chains: &FudChains) -> ConstPropResult {
    let g = build_op_graph(cfg, &chains.chain);
    propagate(cfg, &g)
}

fn reachable_blocks(cfg: &CFG) -> HashSet<usize> {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut work = vec![cfg.entry];
    while let Some(b) = work.pop() {
        if !seen.insert(b) {
            continue;
        }
        for &s in &cfg.blocks[b].succs {
            work.push(s);
        }
    }
    seen
}

fn propagate(cfg: &CFG, g: &OpGraph) -> ConstPropResult {
    let n = g.op_operands.len();
    let reachable = reachable_blocks(cfg);
    let mut value: Vec<AbsVal> = vec![AbsVal::Top; n];

    // Round-robin to a fixpoint. Each φ can only move down the chain
    // `⊤ → Constant → ⊥`, so the iteration terminates. Definitions are pure
    // transfer functions of their operands (a *single* value, not a meet of
    // several paths), so they are recomputed directly each sweep.
    let mut changed = true;
    while changed {
        changed = false;
        for op in 0..n {
            let newv = if g.is_def(op) {
                eval_def(g, op, &value)
            } else {
                value[op].meet(&eval_phi(g, op, &value, &reachable))
            };
            if newv != value[op] {
                value[op] = newv;
                changed = true;
            }
        }
    }

    let mut out = HashMap::new();
    for op in 0..n {
        out.insert(OpId(op), value[op]);
    }
    ConstPropResult { value: out, reachable }
}

fn eval_def(g: &OpGraph, op: usize, value: &[AbsVal]) -> AbsVal {
    eval_rhs(&g.def_rhs[op], &g.def_operand_map[op], value)
}

fn eval_rhs(rhs: &ScalarRhs, map: &HashMap<usize, Option<usize>>, value: &[AbsVal]) -> AbsVal {
    match rhs {
        ScalarRhs::Const(c) => AbsVal::Const(*c),
        ScalarRhs::Var(v) => match map.get(v).copied().flatten() {
            Some(op) => value[op],
            None => AbsVal::Top,
        },
        // An opaque function is not a constant.
        ScalarRhs::Call(_) | ScalarRhs::Unknown => AbsVal::Bottom,
        // A load from memory cannot be proven constant without memory context.
        ScalarRhs::Load { .. } => AbsVal::Bottom,
        ScalarRhs::Add(xs) => fold_arith(xs, map, value, 0, |a, b| a.checked_add(b)),
        ScalarRhs::Mul(xs) => fold_arith(xs, map, value, 1, |a, b| a.checked_mul(b)),
    }
}

fn fold_arith(
    xs: &[ScalarRhs],
    map: &HashMap<usize, Option<usize>>,
    value: &[AbsVal],
    unit: i64,
    f: fn(i64, i64) -> Option<i64>,
) -> AbsVal {
    let mut acc = AbsVal::Const(unit);
    for x in xs {
        let c = eval_rhs(x, map, value);
        acc = match (acc, c) {
            (AbsVal::Const(a), AbsVal::Const(b)) => match f(a, b) {
                Some(r) => AbsVal::Const(r),
                None => AbsVal::Bottom,
            },
            // Any bottom operand (or a combining bottom) makes the result
            // non-constant; a top operand leaves it under-determined.
            (AbsVal::Bottom, _) | (_, AbsVal::Bottom) => AbsVal::Bottom,
            (AbsVal::Top, _) | (_, AbsVal::Top) => AbsVal::Top,
        };
    }
    acc
}

/// Merge the values arriving at a φ from its reachable incoming edges.
fn eval_phi(g: &OpGraph, op: usize, value: &[AbsVal], reachable: &HashSet<usize>) -> AbsVal {
    let mut acc = AbsVal::Top;
    for &o in &g.op_operands[op] {
        // Ignore operands coming from unreachable blocks.
        if !reachable.contains(&g.op_block[o]) {
            continue;
        }
        acc = acc.meet(&value[o]);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::chain::build_chain_with_phi;
    use crate::fud::induct::build_op_graph;
    use crate::ir::cfg::Block;
    use crate::ir::{ScalarDef, ScalarTy, ScalarVar, Stmt};

    #[test]
    fn meet_is_a_lattice_min() {
        assert_eq!(AbsVal::Top.meet(&AbsVal::Const(3)), AbsVal::Const(3));
        assert_eq!(AbsVal::Const(3).meet(&AbsVal::Top), AbsVal::Const(3));
        assert_eq!(AbsVal::Const(3).meet(&AbsVal::Const(3)), AbsVal::Const(3));
        assert_eq!(AbsVal::Const(3).meet(&AbsVal::Const(4)), AbsVal::Bottom);
        assert_eq!(AbsVal::Bottom.meet(&AbsVal::Const(3)), AbsVal::Bottom);
    }

    #[test]
    fn fold_adds_constants_and_poisons_on_bottom() {
        let map = HashMap::new();
        let value: Vec<AbsVal> = vec![];
        let rhs = ScalarRhs::Add(vec![ScalarRhs::Const(2), ScalarRhs::Const(3)]);
        assert_eq!(eval_rhs(&rhs, &map, &value), AbsVal::Const(5));
        let rhs_bad = ScalarRhs::Add(vec![ScalarRhs::Const(2), ScalarRhs::Call(0)]);
        assert_eq!(eval_rhs(&rhs_bad, &map, &value), AbsVal::Bottom);
    }

    /// A block reachable from entry but that cannot reach exit is a shape
    /// `CFG::validate` rejects, so the reachable-operand filter in `eval_phi`
    /// guards a defensive path rather than a representable one. Pin it anyway:
    /// this is *why* the filter is there, and it is what makes the filter load-
    /// bearing rather than dead code if a future caller relaxes `validate`.
    ///
    /// `x = 5` on the reachable edge, `x = <opaque>` on the unreachable one.
    /// Meeting both gives `⊥` — the analysis would report "not a constant" for a
    /// φ whose every *executed* incoming value is 5. Skipping the dead edge gives
    /// the truth.
    #[test]
    fn phi_meet_skips_operands_from_unreachable_blocks() {
        let rhs_of = |v: usize, rhs: ScalarRhs| ScalarDef {
            var: v,
            stmt: 0,
            order: 0,
            rhs,
        };
        let block = |id: usize, preds: Vec<usize>, succs: Vec<usize>, defs: Vec<ScalarDef>| Block {
            id,
            preds,
            succs,
            stmts: defs
                .into_iter()
                .enumerate()
                .map(|(s, d)| Stmt {
                    id: s,
                    uses: vec![],
                    defs: vec![ScalarDef {
                        stmt: s,
                        order: 0,
                        ..d
                    }],
                })
                .collect(),
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        };
        // 0 entry → 1 (def x = 5) → 3; 2 (def x = opaque) → 3; 3 → 4 exit.
        // Block 2 has no predecessor, so nothing can execute it.
        let cfg = CFG {
            vars: vec![ScalarVar { name: "x".into(), ty: ScalarTy::I64 }],
            blocks: vec![
                block(0, vec![], vec![1], vec![]),
                block(1, vec![0], vec![3], vec![rhs_of(0, ScalarRhs::Const(5))]),
                block(2, vec![], vec![3], vec![rhs_of(0, ScalarRhs::Unknown)]),
                block(3, vec![1, 2], vec![4], vec![]),
                block(4, vec![3], vec![], vec![]),
            ],
            entry: 0,
            exit: 4,
        };
        assert!(
            cfg.validate().is_err(),
            "this shape must stay outside what `validate` accepts"
        );
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let g = build_op_graph(&annotated, &chain);
        let res = propagate(&annotated, &g);

        // Block 3 is where `x`'s two definitions merge, so it carries the φ.
        let ix = annotated.index();
        let phi_op = ix
            .phis
            .iter()
            .position(|s| s.block == 3 && s.var == 0)
            .expect("x must have a φ at the merge block");
        let op = OpId(ix.defs.len() + phi_op);
        assert_eq!(
            res.value_of(op),
            AbsVal::Const(5),
            "the φ must be the constant 5, its only reachable incoming value"
        );

        // The answer is right, and the end-to-end path is what a caller sees.
        // Note *where* the pruning happened: `construct_chains` never feeds the
        // φ an operand from a predecessor it cannot reach, so this CFG alone does
        // not exercise `eval_phi`'s filter. The filter is pinned directly below.
        let mut seen: Vec<AbsVal> = Vec::new();
        for o in 0..ix.defs.len() + ix.phis.len() {
            seen.push(res.value_of(OpId(o)));
        }
        assert!(
            seen.iter().any(|v| *v == AbsVal::Const(5)),
            "the analysis produced no constant at all: {seen:?}"
        );
    }

    /// `eval_phi`'s own contract, independent of what `construct_chains` hands it:
    /// operands living in a block outside `reachable` do not take part in the
    /// meet. This is the load-bearing half of the pruning — a φ whose dead edge
    /// carries a non-constant is still a constant.
    #[test]
    fn eval_phi_meets_only_reachable_operands() {
        // Two definitions of `x`, ops 0 and 1, then a φ merging them, op 2.
        // Op 1 sits in block 9, which the caller declares unreachable.
        let g = OpGraph {
            n_defs: 2,
            n_phis: 1,
            op_var: vec![0, 0, 0],
            op_block: vec![1, 9, 3],
            op_stmt: vec![0, 0, usize::MAX],
            def_rhs: vec![ScalarRhs::Const(5), ScalarRhs::Const(5)],
            def_operand_map: vec![HashMap::new(), HashMap::new()],
            op_operands: vec![vec![], vec![], vec![0, 1]],
            phi_slots: vec![vec![0, 1]],
            use_reaching: vec![],
            use_lookup: HashMap::new(),
        };
        let value = [AbsVal::Const(5), AbsVal::Const(5), AbsVal::Top];

        // Every block live: the meet is the constant both edges agree on.
        let all: HashSet<usize> = [1, 3, 9].into_iter().collect();
        assert_eq!(eval_phi(&g, 2, &value, &all), AbsVal::Const(5));

        // A *disagreeing* dead edge must not poison the answer. This is the case
        // the filter exists for: block 9 computes something non-constant, but it
        // never executes, so the φ is still the constant 5.
        let value2 = [AbsVal::Const(5), AbsVal::Bottom, AbsVal::Top];
        assert_eq!(
            eval_phi(&g, 2, &value2, &all),
            AbsVal::Bottom,
            "with block 9 reachable, its non-constant must drag the φ down"
        );
        let live: HashSet<usize> = [1, 3].into_iter().collect();
        assert_eq!(
            eval_phi(&g, 2, &value2, &live),
            AbsVal::Const(5),
            "with block 9 unreachable, the φ is the constant its live edge carries"
        );

        // Sanity: an empty operand list meets to `⊤` (no incoming information).
        let g_empty = OpGraph {
            op_operands: vec![vec![], vec![], vec![]],
            ..g
        };
        assert_eq!(eval_phi(&g_empty, 2, &value, &all), AbsVal::Top);
    }
}