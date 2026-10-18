//! Scalar model for Phase 3 (FUD chains), Wolfe Chapter 6.
//!
//! The array IR (`Access`, `Loop`) has no notion of scalar statements. This
//! module adds the vocabulary Phase 3 needs: variables, uses, (killing)
//! definitions, φ-terms (the forward merge pseudo-definitions) and Υ-terms
//! (the backward merge pseudo-uses that FRUD chains use).
//!
//! IDs (`UseId`, `DefId`, `PhiId`, `UpsilonId`) are *positional*: they are
//! assigned by the canonical traversal of a [`crate::ir::cfg::CFG`] — blocks
//! in id order, statements in order, uses/defs in their recorded order.
//! [`crate::ir::cfg::CFG::index`] recomputes the exact same numbering, so an
//! id means the same thing to the analyzer and to the checker that re-derives
//! the chain from the carried CFG.

use serde::{Deserialize, Serialize};

/// Scalar type. Phase 3 ignores types; they are carried for later phases
/// (Phase 4's constant propagation, Phase 8's interprocedural summaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScalarTy {
    I64,
    F64,
    Bool,
    Unknown,
}

/// A scalar variable in a loop body. Code refers to variables by `usize`
/// index into a CFG's `vars` table; the name is for diagnostics only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScalarVar {
    pub name: String,
    pub ty: ScalarTy,
}

/// A use of a scalar: reads the variable's current value.
/// `stmt`/`order` locate it within its block for diagnostics; the
/// authoritative position is the id assigned by `CFG::index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScalarUse {
    pub var: usize,
    pub stmt: usize,
    pub order: usize,
}

/// A definition of a scalar. Phase 3 models only *killing* definitions
/// (a scalar assignment overwrites the whole variable); the nonkilling
/// case (array stores) is Phase 6, and the `Chain`/`FollowChain` machinery
/// keeps a slot for it.
///
/// Phase 4 adds [`ScalarDef::rhs`]: the right-hand-side expression of the
/// assignment. It is `#[serde(default)]` ([`ScalarRhs::Unknown`]) so every
/// Phase 3 CFG — which models only the *positions* of defs, not their
/// operands — deserializes unchanged and is classified as `Complex` by
/// [`crate::fud::induct`], exactly as before Phase 4.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScalarDef {
    pub var: usize,
    pub stmt: usize,
    pub order: usize,
    /// Phase 4: the expression assigned to `var`. `Unknown` when the
    /// frontend did not model the RHS.
    #[serde(default)]
    pub rhs: ScalarRhs,
}

/// Phase 4: the right-hand-side expression of a scalar definition.
///
/// This is the "operation" vocabulary that Wolfe's `Algorithm Induct`
/// (§6.5) classifies. A definition `m := rhs` is classified from the
/// classifications of the operands `rhs` mentions:
///
/// - [`ScalarRhs::Const`] — a literal (Rule 1).
/// - [`ScalarRhs::Var`] — the value of another scalar; its classification is
///   that of the definition reaching it (Rule 3, "fetch").
/// - [`ScalarRhs::Add`] / [`ScalarRhs::Mul`] — n-ary arithmetic, classified
///   via the IV algebra (Rules 5–7). An empty `Add` is `0`, an empty `Mul`
///   is `1`.
/// - [`ScalarRhs::Call`] — an opaque function of a scalar (`Complex`).
/// - [`ScalarRhs::Unknown`] — not modelled (`Complex`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScalarRhs {
    /// A literal constant.
    Const(i64),
    /// The value of variable `usize` at this point in the program.
    Var(usize),
    /// Sum of the operand values.
    Add(Vec<ScalarRhs>),
    /// Product of the operand values.
    Mul(Vec<ScalarRhs>),
    /// Array/memory load: `base[index]`. Used for pointer chasing and
    /// symbolic guard bounds. `base` is a scalar variable; `index` is any
    /// scalar expression.
    Load { base: usize, index: Box<ScalarRhs> },
    /// An arbitrary function applied to a variable (`Complex`).
    Call(usize),
    /// An unmodelled expression (`Complex`).
    Unknown,
}

impl Default for ScalarRhs {
    fn default() -> Self {
        ScalarRhs::Unknown
    }
}

impl ScalarRhs {
    /// Append every variable this expression reads (in first-seen order,
    /// with duplicates removed) to `out`. `Call(v)` contributes `v`; the
    /// arithmetic nodes recurse; `Const`/`Unknown`/`Load` contribute nothing
    /// beyond their sub-expressions.
    pub fn collect_vars(&self, out: &mut Vec<usize>) {
        match self {
            ScalarRhs::Var(v) | ScalarRhs::Call(v) => {
                if !out.contains(v) {
                    out.push(*v);
                }
            }
            ScalarRhs::Add(xs) | ScalarRhs::Mul(xs) => {
                for x in xs {
                    x.collect_vars(out);
                }
            }
            ScalarRhs::Load { base, index } => {
                if !out.contains(base) {
                    out.push(*base);
                }
                index.collect_vars(out);
            }
            ScalarRhs::Const(_) | ScalarRhs::Unknown => {}
        }
    }
}

/// A φ-term: the pseudo-definition at a control-flow merge (Wolfe §6.1).
/// At most one per (block, variable). Its incoming links are filled by
/// Algorithm Chain and live in `fud::chain::FudChain::phi_incoming`, one
/// slot per predecessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhiTerm {
    pub var: usize,
    pub block: usize,
}

/// A Υ-term ("upsilon"): the pseudo-use at a merge in the *reverse*
/// direction, used by FRUD chains (Wolfe §6.7.5). At most one per
/// (block, variable); incoming links live in
/// `fud::frud::FrudChain::upsilon_incoming`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UpsilonTerm {
    pub var: usize,
    pub block: usize,
}

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub usize);

        impl $name {
            /// The zero-based position of this item in its CFG's canonical order.
            pub fn index(self) -> usize {
                self.0
            }
        }
    };
}

id_type!(UseId, "Positional id of a `ScalarUse` in a CFG.");
id_type!(DefId, "Positional id of a `ScalarDef` in a CFG.");
id_type!(PhiId, "Positional id of a `PhiTerm` in a CFG.");
id_type!(UpsilonId, "Positional id of an `UpsilonTerm` in a CFG.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_ordered_by_position() {
        let mut v = vec![UseId(3), UseId(0), UseId(2), UseId(1)];
        v.sort();
        assert_eq!(v, vec![UseId(0), UseId(1), UseId(2), UseId(3)]);
        assert_eq!(UseId(7).index(), 7);
        assert_eq!(DefId(4).index(), 4);
        assert_eq!(PhiId(0).index(), 0);
        assert_eq!(UpsilonId(9).index(), 9);
    }

    #[test]
    fn scalar_types_round_trip_through_json() {
        for ty in [ScalarTy::I64, ScalarTy::F64, ScalarTy::Bool, ScalarTy::Unknown] {
            let var = ScalarVar { name: "x".into(), ty };
            let s = serde_json::to_string(&var).unwrap();
            let back: ScalarVar = serde_json::from_str(&s).unwrap();
            assert_eq!(back, var);
        }
    }

    #[test]
    fn use_def_and_terms_round_trip_through_json() {
        let u = ScalarUse { var: 1, stmt: 2, order: 3 };
        let d = ScalarDef { var: 4, stmt: 5, order: 6, rhs: ScalarRhs::Unknown };
        let p = PhiTerm { var: 7, block: 8 };
        let y = UpsilonTerm { var: 9, block: 10 };
        for s in [
            serde_json::to_string(&u).unwrap(),
            serde_json::to_string(&d).unwrap(),
            serde_json::to_string(&p).unwrap(),
            serde_json::to_string(&y).unwrap(),
        ] {
            assert!(s.contains('7') || s.contains('1') || s.contains('4') || s.contains('9'));
        }
        let u2: ScalarUse = serde_json::from_str(&serde_json::to_string(&u).unwrap()).unwrap();
        let d2: ScalarDef = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        let p2: PhiTerm = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        let y2: UpsilonTerm = serde_json::from_str(&serde_json::to_string(&y).unwrap()).unwrap();
        assert_eq!((u, d, p, y), (u2, d2, p2, y2));
    }
}
