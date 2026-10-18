//! Phase 9a: loop-body scalar statements and expressions.
//!
//! Before Phase 9a, the IR modelled array accesses only — `Access` in
//! `src/ir/mod.rs`. The CFG builder (`src/fud/cfg.rs`) converted those into
//! blocks with no scalar uses or defs, so the FUD chain machinery had nothing
//! to link and [`crate::fud::induct`] had nothing to classify.
//!
//! This module adds three statement kinds — array accesses, scalar
//! assignments and guards — plus a small expression tree for scalar
//! arithmetic. That is enough to model the loop IV (`i = i + step`), the loop
//! guard (`i < upper`) and any scalar computation whose value flows into an
//! array index.
//!
//! ## Relationship to the Phase 3/4 scalar model
//!
//! The scalar CFG ([`crate::ir::cfg::CFG`]) records statements as
//! `{ uses, defs }` over a variable table, and classifications are driven by
//! the right-hand side of a definition ([`crate::ir::scalar::ScalarRhs`]).
//! The statement type here is the *frontend* form: it carries an expression
//! tree with an explicit IV reference ([`ScalarExpr::Iv`]) and lowers into the
//! existing vocabulary via [`ScalarExpr::to_rhs`] and [`Stmt::lower`]. IVs are
//! registered as scalars `0..depth`, so `Iv(k)` and `Scalar(k)` name the same
//! variable space; both count as reads of scalar `k`.

use serde::{Deserialize, Serialize};

use crate::ir::cfg::Stmt as CfgStmt;
use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarUse};

/// A statement in a loop body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum StmtKind {
    /// An array access, read or write. Wraps the existing `Access` type.
    /// It contributes no scalar uses or defs (the array-side dependence is
    /// M2B's path); it is carried so a `Loop`'s statement body can mirror its
    /// `body` accesses.
    ///
    /// The wrapped `Access` is held under the field name `access` rather than
    /// as an untagged newtype payload: `Access` itself carries a field named
    /// `kind`, and `#[serde(tag = "kind")]` would otherwise flatten the two
    /// into a duplicate key on the wire.
    Array { access: crate::ir::Access },

    /// A scalar assignment: `target := expr`.
    Assign {
        /// Index into the loop's scalar table (`Loop::scalars`, with the IVs
        /// registered first).
        target: usize,
        expr: ScalarExpr,
    },

    /// A guard: execution continues only if all constraints hold.
    /// Phase 9a emits at most one guard per IV, at the header.
    /// Each constraint is `iv <op> bound` (e.g., `i < 64`).
    Guard { constraints: Vec<crate::ir::Cmp> },

    /// A function call: `target := callee(args...)`.
    /// Phase 9: enables interprocedural dependence analysis by modeling calls
    /// within the loop body. The checker can use this to verify that calls
    /// do not introduce unexpected dependences.
    Call {
        /// Name of the called function.
        callee: String,
        /// Arguments passed to the call.
        args: Vec<ScalarExpr>,
        /// Optional target scalar to store the return value.
        ret: Option<usize>,
    },
}

/// A scalar expression over IVs, scalars and constants.
///
/// `Iv(k)` reads the k-th IV of the enclosing loop. `Scalar(k)` reads the
/// k-th scalar from the loop's scalar table. The two are the same variable
/// space when the IVs are registered as scalars `0..depth` (see
/// [`crate::ir::Loop::with_iv_scalars`]); `Iv` exists so a frontend can say
/// "the induction variable" without knowing its scalar slot.
///
/// `Load { base, index }` represents an array/memory load: `base[index]`.
/// This enables pointer chasing (M5), symbolic guard bounds (M3), and
/// symbolic pointer chase (Phase 5.3). `base` is a scalar index into the
/// loop's scalar table; `index` is any scalar expression.
///
/// The tree is small by design — Phase 9a only needs what a loop IV requires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScalarExpr {
    Iv(usize),
    Scalar(usize),
    Const(i64),
    Add(Box<ScalarExpr>, Box<ScalarExpr>),
    Mul(Box<ScalarExpr>, Box<ScalarExpr>),
    /// Array/memory load: `base[index]`.
    /// `base` is a scalar variable holding the base address/array reference.
    /// `index` is any scalar expression computing the index.
    Load { base: usize, index: Box<ScalarExpr> },
}

/// Memory context for evaluating `ScalarExpr::Load`.
/// Maps a base address (scalar value) to an array of i64 values.
#[derive(Debug, Clone, Default)]
pub struct MemoryContext {
    /// Maps base address to array contents.
    pub arrays: std::collections::HashMap<i64, Vec<i64>>,
}

impl MemoryContext {
    /// Create a new empty memory context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an array at the given base address.
    pub fn add_array(&mut self, base: i64, data: Vec<i64>) {
        self.arrays.insert(base, data);
    }

    /// Read a value from memory at `base[index]`.
    pub fn read(&self, base: i64, index: i64) -> Option<i64> {
        let arr = self.arrays.get(&base)?;
        if index < 0 || (index as usize) >= arr.len() {
            return None;
        }
        Some(arr[index as usize])
    }
}

impl ScalarExpr {
    /// Evaluate the expression against concrete IV and scalar values.
    /// Returns `None` on an out-of-range index or arithmetic overflow.
    /// `Load` requires a memory context (base address → array contents).
    pub fn eval(
        &self,
        ivs: &[i64],
        scalars: &[i64],
        memory: Option<&MemoryContext>,
    ) -> Option<i64> {
        match self {
            ScalarExpr::Iv(k) => ivs.get(*k).copied(),
            ScalarExpr::Scalar(k) => scalars.get(*k).copied(),
            ScalarExpr::Const(c) => Some(*c),
            ScalarExpr::Add(a, b) => a
                .eval(ivs, scalars, memory)?
                .checked_add(b.eval(ivs, scalars, memory)?),
            ScalarExpr::Mul(a, b) => a
                .eval(ivs, scalars, memory)?
                .checked_mul(b.eval(ivs, scalars, memory)?),
            ScalarExpr::Load { base, index } => {
                let mem = memory?;
                let base_val = scalars.get(*base).copied()?;
                let idx = index.eval(ivs, scalars, memory)?;
                mem.read(base_val, idx)
            }
        }
    }

    /// Collect the scalar variables this expression reads, in first-seen order
    /// with duplicates removed. `Iv(k)` and `Scalar(k)` both read scalar `k`.
    /// `Load { base, index }` reads `base` and all scalars in `index`.
    pub fn scalars_read(&self, out: &mut Vec<usize>) {
        match self {
            ScalarExpr::Iv(k) | ScalarExpr::Scalar(k) => {
                if !out.contains(k) {
                    out.push(*k);
                }
            }
            ScalarExpr::Const(_) => {}
            ScalarExpr::Add(a, b) | ScalarExpr::Mul(a, b) => {
                a.scalars_read(out);
                b.scalars_read(out);
            }
            ScalarExpr::Load { base, index } => {
                if !out.contains(base) {
                    out.push(*base);
                }
                index.scalars_read(out);
            }
        }
    }

    /// Lower to the Phase 3/4 right-hand-side vocabulary the scalar CFG and
    /// [`crate::fud::induct`] classify. `Iv(k)`/`Scalar(k)` both become
    /// `ScalarRhs::Var(k)`. `Load` becomes `ScalarRhs::Load`.
    pub fn to_rhs(&self) -> ScalarRhs {
        match self {
            ScalarExpr::Iv(k) | ScalarExpr::Scalar(k) => ScalarRhs::Var(*k),
            ScalarExpr::Const(c) => ScalarRhs::Const(*c),
            ScalarExpr::Add(a, b) => ScalarRhs::Add(vec![a.to_rhs(), b.to_rhs()]),
            ScalarExpr::Mul(a, b) => ScalarRhs::Mul(vec![a.to_rhs(), b.to_rhs()]),
            ScalarExpr::Load { base, index } => ScalarRhs::Load {
                base: *base,
                index: Box::new(index.to_rhs()),
            },
        }
    }
}
/// A statement in a loop body, carrying its canonical id within the body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stmt {
    pub id: usize,
    pub kind: StmtKind,
}

impl Stmt {
    /// The scalar variables this statement reads.
    pub fn uses(&self) -> Vec<usize> {
        let mut out = Vec::new();
        match &self.kind {
            StmtKind::Array { .. } => {}
            StmtKind::Assign { expr, .. } => expr.scalars_read(&mut out),
            StmtKind::Guard { constraints } => {
                for c in constraints {
                    if !out.contains(&c.iv) {
                        out.push(c.iv);
                    }
                }
            }
            StmtKind::Call { args, .. } => {
                for arg in args {
                    arg.scalars_read(&mut out);
                }
            }
        }
        out
    }

    /// The scalar variables this statement writes.
    pub fn defs(&self) -> Vec<usize> {
        match &self.kind {
            StmtKind::Array { .. } => Vec::new(),
            StmtKind::Assign { target, .. } => vec![*target],
            StmtKind::Guard { .. } => Vec::new(),
            StmtKind::Call { ret, .. } => ret.map_or(Vec::new(), |r| vec![r]),
        }
    }

    /// Lower to the scalar-CFG statement the chains and analyses consume.
    /// `stmt` is the statement's index within its block; use/def positions are
    /// set canonically (`order` 0..). Array accesses lower to a statement with
    /// no uses or defs.
    pub fn lower(&self, stmt: usize) -> CfgStmt {
        let uses = self
            .uses()
            .into_iter()
            .enumerate()
            .map(|(order, var)| ScalarUse { var, stmt, order })
            .collect();
        let defs = match &self.kind {
            StmtKind::Assign { target, expr } => vec![ScalarDef {
                var: *target,
                stmt,
                order: 0,
                rhs: expr.to_rhs(),
            }],
            _ => Vec::new(),
        };
        CfgStmt { id: stmt, uses, defs }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_const() {
        assert_eq!(ScalarExpr::Const(7).eval(&[], &[], None), Some(7));
    }

    #[test]
    fn eval_iv() {
        assert_eq!(ScalarExpr::Iv(0).eval(&[42], &[], None), Some(42));
        assert_eq!(ScalarExpr::Iv(1).eval(&[42], &[], None), None);
    }

    #[test]
    fn eval_scalar() {
        assert_eq!(ScalarExpr::Scalar(0).eval(&[], &[13], None), Some(13));
        assert_eq!(ScalarExpr::Scalar(1).eval(&[], &[13], None), None);
    }

    #[test]
    fn eval_add_mul() {
        // (2 * i) + 3 with i = 5 -> 13
        let e = ScalarExpr::Add(
            Box::new(ScalarExpr::Mul(
                Box::new(ScalarExpr::Const(2)),
                Box::new(ScalarExpr::Iv(0)),
            )),
            Box::new(ScalarExpr::Const(3)),
        );
        assert_eq!(e.eval(&[5], &[], None), Some(13));
    }

    #[test]
    fn eval_overflow_returns_none() {
        let e = ScalarExpr::Mul(
            Box::new(ScalarExpr::Const(i64::MAX)),
            Box::new(ScalarExpr::Const(2)),
        );
        assert_eq!(e.eval(&[], &[], None), None);
    }

    #[test]
    fn eval_load() {
        let mut mem = MemoryContext::new();
        mem.add_array(1000, vec![10, 20, 30, 40]);
        // Load base=1000, index=2 -> 30
        let e = ScalarExpr::Load {
            base: 0,
            index: Box::new(ScalarExpr::Const(2)),
        };
        assert_eq!(e.eval(&[], &[1000], Some(&mem)), Some(30));
        // Out of bounds
        let e2 = ScalarExpr::Load {
            base: 0,
            index: Box::new(ScalarExpr::Const(10)),
        };
        assert_eq!(e2.eval(&[], &[1000], Some(&mem)), None);
        // Unknown base
        assert_eq!(e.eval(&[], &[999], Some(&mem)), None);
        // No memory context
        assert_eq!(e.eval(&[], &[1000], None), None);
    }

    #[test]
    fn scalars_read_dedupes() {
        // (s0 + s1) * s0 -> [0, 1]
        let e = ScalarExpr::Mul(
            Box::new(ScalarExpr::Add(
                Box::new(ScalarExpr::Scalar(0)),
                Box::new(ScalarExpr::Scalar(1)),
            )),
            Box::new(ScalarExpr::Scalar(0)),
        );
        let mut out = Vec::new();
        e.scalars_read(&mut out);
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn iv_and_scalar_name_the_same_slot() {
        let mut out = Vec::new();
        ScalarExpr::Iv(2).scalars_read(&mut out);
        assert_eq!(out, vec![2]);
        assert_eq!(ScalarExpr::Iv(2).to_rhs(), ScalarRhs::Var(2));
    }

    #[test]
    fn stmt_uses_defs() {
        let assign = Stmt {
            id: 0,
            kind: StmtKind::Assign {
                target: 2,
                expr: ScalarExpr::Add(
                    Box::new(ScalarExpr::Scalar(0)),
                    Box::new(ScalarExpr::Scalar(1)),
                ),
            },
        };
        assert_eq!(assign.uses(), vec![0, 1]);
        assert_eq!(assign.defs(), vec![2]);

        let guard = Stmt {
            id: 1,
            kind: StmtKind::Guard { constraints: vec![crate::ir::Cmp { iv: 0, op: crate::ir::CmpOp::Lt, bound: 64 }] },
        };
        assert_eq!(guard.uses(), vec![0]);
        assert!(guard.defs().is_empty());
    }

    #[test]
    fn stmt_lowers_to_cfg() {
        let s = Stmt {
            id: 4,
            kind: StmtKind::Assign {
                target: 1,
                expr: ScalarExpr::Add(
                    Box::new(ScalarExpr::Iv(0)),
                    Box::new(ScalarExpr::Const(1)),
                ),
            },
        };
        let cfg_stmt = s.lower(3);
        assert_eq!(cfg_stmt.id, 3);
        assert_eq!(cfg_stmt.uses, vec![ScalarUse { var: 0, stmt: 3, order: 0 }]);
        assert_eq!(cfg_stmt.defs.len(), 1);
        assert_eq!(cfg_stmt.defs[0].var, 1);
        assert_eq!(cfg_stmt.defs[0].order, 0);
        assert_eq!(
            cfg_stmt.defs[0].rhs,
            ScalarRhs::Add(vec![ScalarRhs::Var(0), ScalarRhs::Const(1)])
        );
    }

    #[test]
    fn stmt_serde_roundtrip_assign() {
        let s = Stmt {
            id: 5,
            kind: StmtKind::Assign {
                target: 1,
                expr: ScalarExpr::Add(
                    Box::new(ScalarExpr::Scalar(1)),
                    Box::new(ScalarExpr::Const(1)),
                ),
            },
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Stmt = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn stmt_serde_roundtrip_guard() {
        let s = Stmt {
            id: 6,
            kind: StmtKind::Guard { constraints: vec![crate::ir::Cmp { iv: 0, op: crate::ir::CmpOp::Lt, bound: 64 }] },
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Stmt = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn stmt_serde_roundtrip_array() {
        let access = crate::ir::Access {
            kind: crate::ir::AccessKind::Write,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: crate::ir::OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: crate::ir::IndexKind::Affine,
        };
        let s = Stmt {
            id: 7,
            kind: StmtKind::Array { access },
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Stmt = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert!(back.uses().is_empty());
        assert!(back.defs().is_empty());
    }
}