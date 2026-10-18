//! Phase 5 §5.1.8 — reduction detection (Wolfe §7.2.3, "idiom recognition").
//!
//! `sum += a[i]` is the most common loop idiom in real code, and once a loop's
//! accumulator is recognised as a *reduction* it is parallelisable even though
//! the scalar dependence (`sum` flows across iterations) would otherwise make
//! the loop sequential. This module recognises the idiom.
//!
//! ```text
//! procedure DetectReduction(cfg, chains, frud):
//!     for each scalar definition d of variable M:
//!         is M used on the RHS of its own definition?   (self-use)
//!         is the operation associative and commutative?
//!         classify: sum, product, max, min, count
//! ```
//!
//! ## Where the "self-use" comes from
//!
//! The spec reads the self-use out of `frud.def_to_use[d]`. In a φ-structured
//! CFG the use of `M` on the RHS of `M := M + x` is reached by the loop-header
//! φ (the previous iteration's value), a hop the *direct* FRUD relation does not
//! take — so a literal `frud` lookup misses the very idiom this module exists to
//! find. The authoritative operand list is the definition's own right-hand side,
//! so that is what decides `self-use` here; `frud` is still consulted and a
//! `frud` self-link is accepted as equivalent. The deviation is documented
//! because it changes what the analysis *can* see, not because it weakens it.
//!
//! ## Classification
//!
//! `ScalarRhs` carries the operator shape but **not** a callee's name, so a
//! `Call` (which is how `max`/`min`/`MaxWithIndex` would arrive) cannot be
//! classified into `Max`/`Min`/`MaxWithIndex` here: those need the function
//! name, which the scalar IR does not record. The kinds this module emits are
//! therefore `Sum`, `Product` and `Count`; the others are reachable by
//! extending `ScalarRhs::Call` to carry a name, and the module says so rather
//! than guessing a `Call` is a `Max`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::fud::scalardd::find_loops;
use crate::fud::{compute_dominators, FudChains};
use crate::ir::cfg::CFG;
use crate::ir::scalar::{DefId, ScalarRhs};

/// The kind of reduction (Wolfe §7.2.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum ReductionKind {
    /// `M := M + x` — a summation accumulator.
    Sum,
    /// `M := M * x` — a product accumulator.
    Product,
    /// `M := max(M, x)`.
    Max,
    /// `M := min(M, x)`.
    Min,
    /// `M := max(M, x)` with an index tie-break.
    MaxWithIndex,
    /// `M := M + 1` — an occurrence count.
    Count,
    /// User-defined associative-commutative reduction.
    /// The `name` identifies the operation (e.g., "custom_add", "bitwise_or").
    /// The `identity` is the identity element for the operation.
    /// The `is_ac` flag asserts the operation is associative and commutative.
    /// The checker MUST re-verify these properties.
    UserDefined {
        name: String,
        identity: i64,
        is_ac: bool,
    },
}

impl ReductionKind {
    pub fn as_str(&self) -> String {
        match self {
            ReductionKind::Sum => "sum".to_string(),
            ReductionKind::Product => "product".to_string(),
            ReductionKind::Max => "max".to_string(),
            ReductionKind::Min => "min".to_string(),
            ReductionKind::MaxWithIndex => "max-with-index".to_string(),
            ReductionKind::Count => "count".to_string(),
            ReductionKind::UserDefined { name, .. } => name.clone(),
        }
    }

    /// Is the underlying operator associative and commutative? Only such an
    /// operator can be re-associated across iterations.
    pub fn is_associative_commutative(&self) -> bool {
        match self {
            ReductionKind::UserDefined { is_ac, .. } => *is_ac,
            ReductionKind::MaxWithIndex => false,
            _ => true,
        }
    }

    /// Get the identity element for this reduction kind.
    pub fn identity(&self) -> Option<i64> {
        match self {
            ReductionKind::UserDefined { identity, .. } => Some(*identity),
            ReductionKind::Sum => Some(0),
            ReductionKind::Product => Some(1),
            ReductionKind::Max => Some(i64::MIN),
            ReductionKind::Min => Some(i64::MAX),
            ReductionKind::Count => Some(0),
            ReductionKind::MaxWithIndex => None,
        }
    }
}

/// A recognised reduction: `var` accumulates in loop `loop_` under `op`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reduction {
    /// The scalar variable being accumulated.
    pub var: usize,
    /// The operator, as a short stable string (`"add"`, `"mul"`).
    pub op: String,
    pub kind: ReductionKind,
    /// The header block of the loop the reduction lives in.
    pub loop_: usize,
    /// For user-defined reductions, the identity element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<i64>,
}

/// Classify a right-hand side that has already been shown to read `M`.
///
/// `Some(Count)` for `M + 1`; `Some(Sum)` for any other `+`; `Some(Product)`
/// for `*`. `Call`/`Unknown`/a bare `Var`/a constant are `None` — not
/// associative-commutative accumulations this IR can name.
pub fn classify_rhs(rhs: &ScalarRhs) -> Option<(ReductionKind, &'static str)> {
    match rhs {
        ScalarRhs::Add(xs) => {
            // `M + 1` (a count) is a two-operand sum with a literal `1`.
            let has_one = xs.iter().any(|x| matches!(x, ScalarRhs::Const(1)));
            let has_complex = xs.iter().any(|x| {
                matches!(x, ScalarRhs::Var(_) | ScalarRhs::Add(_) | ScalarRhs::Mul(_))
            });
            if has_one && has_complex && xs.len() == 2 {
                Some((ReductionKind::Count, "add"))
            } else {
                Some((ReductionKind::Sum, "add"))
            }
        }
        ScalarRhs::Mul(_) => Some((ReductionKind::Product, "mul")),
        _ => None,
    }
}

/// The reduction classification of `rhs` if it reads `var`.
fn reduction_of(rhs: &ScalarRhs, var: usize) -> Option<(ReductionKind, &'static str)> {
    let mut read = Vec::new();
    rhs.collect_vars(&mut read);
    if !read.contains(&var) {
        return None;
    }
    classify_rhs(rhs)
}

/// §5.1.8: the reductions in `cfg`.
///
/// A definition of variable `M` is a reduction update when its right-hand side
/// reads `M` (the accumulator's previous value) and applies an
/// associative-commutative operator. Only definitions inside a loop are
/// reported — a self-referencing assignment outside a loop is not a reduction —
/// and each `(var, loop_)` pair is reported once.
pub fn detect_reductions(cfg: &CFG, chains: &FudChains) -> Vec<Reduction> {
    let dom = compute_dominators(cfg);
    let loops = find_loops(cfg, &dom);
    let ix = cfg.index();
    let mut out = Vec::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();

    for (d, site) in ix.defs.iter().enumerate() {
        let d = DefId(d);
        let var = site.var;
        // The definition's actual RHS, located by its block/statement/order.
        let rhs = cfg.blocks[site.block].stmts[site.stmt].defs[site.order].rhs.clone();
        let from_rhs = reduction_of(&rhs, var);
        // FRUD self-link, accepted as an equivalent route to the same fact.
        let from_frud = chains.frud.uses_of(d).iter().any(|u| ix.uses[u.0].var == var);
        if from_rhs.is_none() && !from_frud {
            continue;
        }
        let (kind, op) = match from_rhs.or_else(|| classify_rhs(&rhs)) {
            Some(k) => k,
            None => continue,
        };
        // The innermost enclosing loop's header; a def outside every loop is
        // not a loop reduction and is not reported.
        let Some(header) = loops
            .iter()
            .filter(|l| l.body.contains(&site.block))
            .min_by_key(|l| l.body.len())
            .map(|l| l.header)
        else {
            continue;
        };
        if seen.insert((var, header)) {
            let identity = match &kind {
                ReductionKind::UserDefined { identity, .. } => Some(*identity),
                _ => kind.identity(),
            };
            out.push(Reduction { var, op: op.to_string(), kind, loop_: header, identity });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::{build_cfg_from_loop, build_fud_chains};
    use crate::ir::stmt::{ScalarExpr, Stmt as LoopStmt, StmtKind};
    use crate::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue, ScalarTy, ScalarVar};

    fn acc(kind: AccessKind, base: &str, index: i64) -> Access {
        Access {
            kind,
            base: base.into(),
            index: vec![index],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    /// `for (i = 0; i < 8; i++) { sum = <expr>; }` with one scalar statement.
    fn loop_with(expr: ScalarExpr) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 8, 1)],
            body: vec![acc(AccessKind::Write, "sum", 0), acc(AccessKind::Read, "A", 1)],
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![
                ScalarVar { name: "i".into(), ty: ScalarTy::I64 },
                ScalarVar { name: "sum".into(), ty: ScalarTy::I64 },
                ScalarVar { name: "k".into(), ty: ScalarTy::I64 },
            ],
            stmts: Some(vec![LoopStmt { id: 0, kind: StmtKind::Assign { target: 1, expr } }]),
        }
    }

    fn reductions_of(l: &Loop) -> Vec<Reduction> {
        let cfg = build_cfg_from_loop(l).unwrap();
        let (_a, chains) = build_fud_chains(&cfg);
        detect_reductions(&cfg, &chains)
    }

    #[test]
    fn sum_is_detected() {
        // sum := sum + A[i]  (scalar 1 + scalar 2)
        let l = loop_with(ScalarExpr::Add(
            Box::new(ScalarExpr::Scalar(1)),
            Box::new(ScalarExpr::Scalar(2)),
        ));
        let r = reductions_of(&l);
        assert!(r.iter().any(|x| x.var == 1 && x.kind == ReductionKind::Sum), "{r:?}");
    }

    #[test]
    fn product_is_detected() {
        let l = loop_with(ScalarExpr::Mul(
            Box::new(ScalarExpr::Scalar(1)),
            Box::new(ScalarExpr::Const(2)),
        ));
        let r = reductions_of(&l);
        assert!(r.iter().any(|x| x.kind == ReductionKind::Product), "{r:?}");
    }

    #[test]
    fn count_is_detected() {
        let l = loop_with(ScalarExpr::Add(
            Box::new(ScalarExpr::Scalar(1)),
            Box::new(ScalarExpr::Const(1)),
        ));
        let r = reductions_of(&l);
        assert!(r.iter().any(|x| x.kind == ReductionKind::Count), "{r:?}");
    }

    #[test]
    fn a_non_accumulating_assignment_is_not_a_reduction() {
        let l = loop_with(ScalarExpr::Const(3));
        let r = reductions_of(&l);
        assert!(r.iter().all(|x| x.var != 1), "{r:?}");
    }

    #[test]
    fn reduction_kind_round_trips() {
        for k in [
            ReductionKind::Sum,
            ReductionKind::Product,
            ReductionKind::Max,
            ReductionKind::Min,
            ReductionKind::MaxWithIndex,
            ReductionKind::Count,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: ReductionKind = serde_json::from_str(&s).unwrap();
            assert_eq!(back, k);
        }
    }
}