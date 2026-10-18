//! Phase 5 §5.1.4 — pure-function classification (Wolfe §8.4).
//!
//! Interprocedural analysis (Phase 8) wants to treat a *pure* function as a
//! summary: its result is determined entirely by its arguments, so a call can be
//! memoised, reordered, or eliminated. Classifying purity is the first thing
//! Phase 8 needs.
//!
//! ```text
//! Pure function:   uses but does not modify arguments, no globals, no I/O,
//!                  return value determined by the arguments.
//! Pure subroutine: only effect is modify read-write/write-only arguments,
//!                  no I/O or globals, may call only pure procedures.
//! ```
//!
//! ## The IR is a statement list, so purity is a syntactic property here
//!
//! There is no call graph in this IR yet (`CallGraph` is a Phase 8 dependency
//! the spec itself stubs). A function is modelled as a [`Function`]: a scalar
//! parameter list with access modes, the names of array bases that are *local*
//! (its own arguments/locals), the body statements, and the callee names it
//! mentions. [`classify_purity`] reads those and answers with the three-valued
//! [`Purity`] lattice. A call to a callee that is *not* in the supplied pure set
//! makes the caller at most `PureSubroutine` — never `Pure` — because an unknown
//! callee might do anything.
//!
//! ## Why `Impure` is the safe default
//!
//! Every ambiguity resolves downward: an unmodelled statement, an unknown
//! callee, a write or read of a name that is not a parameter/local. A classifier
//! that guessed `Pure` wrongly would let Phase 8 delete a call with side
//! effects; one that answers `Impure` too often only misses an optimization.
//! That is the same direction §5.1.3's I/O rule takes.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::ir::effect::{effect_of, Effect, IoChannels};
use crate::ir::stmt::{ScalarExpr, Stmt, StmtKind};

/// How a scalar parameter is used by a function (Wolfe §8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamMode {
    /// Read only.
    In,
    /// Write only.
    Out,
    /// Read and written.
    InOut,
}

/// A function as this classifier sees it.
///
/// `locals` names the array bases that are the function's own arguments or
/// locals; an access to any *other* base is a global access and blocks purity.
/// This is the array-side analogue of the scalar parameter list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    /// The scalar parameter list, in order; `index` matches the scalar slot a
    /// statement reads or writes.
    pub params: Vec<ParamMode>,
    /// Array base names that are parameters or locals of this function.
    pub locals: Vec<String>,
    /// The body statements, in order.
    pub body: Vec<Stmt>,
    /// Names of functions called from the body.
    pub callees: Vec<String>,
}

/// The purity lattice (Wolfe §8.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Purity {
    /// Uses but does not modify arguments; no globals, no I/O; result is a
    /// function of the arguments.
    Pure,
    /// The only effect is modifying its read-write/write-only arguments; no
    /// I/O or globals; calls only pure procedures. `modified` lists the
    /// modified *scalar* argument indices.
    PureSubroutine { modified: Vec<usize> },
    /// Anything else.
    Impure,
}

impl Purity {
    pub fn is_pure(&self) -> bool {
        matches!(self, Purity::Pure)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Purity::Pure => "pure",
            Purity::PureSubroutine { .. } => "pure-subroutine",
            Purity::Impure => "impure",
        }
    }
}

/// The set of callee names known (or declared) to be pure. A name absent from
/// this set blocks `Pure`: an unknown call might have any effect.
#[derive(Debug, Clone, Default)]
pub struct PureSet {
    names: HashSet<String>,
}

impl PureSet {
    pub fn new() -> PureSet {
        PureSet::default()
    }

    pub fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

impl FromIterator<String> for PureSet {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> PureSet {
        PureSet { names: iter.into_iter().collect() }
    }
}

/// Does the body touch a device under `channels`?
fn has_io(body: &[Stmt], channels: &IoChannels) -> bool {
    body.iter().any(|s| match &s.kind {
        StmtKind::Array { access } => effect_of(access, channels) == Effect::Io,
        _ => false,
    })
}

/// The private scalar slots the body writes, and whether it writes any slot
/// that is not a parameter.
fn scalar_writes(body: &[Stmt], nparams: usize) -> (Vec<usize>, bool) {
    let mut writes = Vec::new();
    let mut global = false;
    for s in body {
        if let StmtKind::Assign { target, .. } = &s.kind {
            if *target < nparams {
                writes.push(*target);
            } else {
                global = true;
            }
        }
    }
    (writes, global)
}

/// Does the body read a scalar slot that is not a parameter?
fn reads_global(body: &[Stmt], nparams: usize) -> bool {
    let over = |e: &ScalarExpr| {
        let mut v = Vec::new();
        e.scalars_read(&mut v);
        v.into_iter().any(|k| k >= nparams)
    };
    body.iter().any(|s| match &s.kind {
        StmtKind::Assign { expr, .. } => over(expr),
        StmtKind::Guard { constraints } => constraints.iter().any(|c| c.iv >= nparams),
        StmtKind::Array { .. } => false,
        StmtKind::Call { .. } => true, // Calls are impure by default
    })
}

/// Does the body touch an array base outside the function's own locals?
fn touches_global_array(body: &[Stmt], locals: &[String]) -> bool {
    body.iter().any(|s| match &s.kind {
        StmtKind::Array { access } => !locals.iter().any(|l| l == &access.base),
        _ => false,
    })
}

/// §5.1.4: classify `func`, given the callees known to be pure.
///
/// The decision, in order:
///
/// 1. Any I/O, any global scalar or array access → [`Purity::Impure`].
/// 2. A call to a callee outside `pure` → at most [`Purity::PureSubroutine`]
///    (the callee might have effects); with no other modification it is
///    `PureSubroutine { modified: [] }`, otherwise `Impure`, because an
///    unknown callee's effects cannot be summarized here.
/// 3. No writes, all callees pure → [`Purity::Pure`].
/// 4. Writes only to scalar parameters, all callees pure → `PureSubroutine`
///    listing the modified parameter indices (deduplicated, sorted).
pub fn classify_purity(func: &Function, pure: &PureSet) -> Purity {
    let channels = IoChannels::standard();
    if has_io(&func.body, &channels) {
        return Purity::Impure;
    }
    if touches_global_array(&func.body, &func.locals) {
        return Purity::Impure;
    }
    let nparams = func.params.len();
    if reads_global(&func.body, nparams) {
        return Purity::Impure;
    }

    let (mut modified, scalar_global) = scalar_writes(&func.body, nparams);
    if scalar_global {
        return Purity::Impure;
    }
    modified.sort_unstable();
    modified.dedup();

    let unknown_callee = func.callees.iter().any(|c| !pure.contains(c));
    if unknown_callee {
        if modified.is_empty() {
            return Purity::PureSubroutine { modified: vec![] };
        }
        return Purity::Impure;
    }

    if modified.is_empty() {
        Purity::Pure
    } else {
        Purity::PureSubroutine { modified }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::stmt::{ScalarExpr, Stmt, StmtKind};
    use crate::ir::{Access, AccessKind, IndexKind, OffsetValue};

    fn arr(kind: AccessKind, base: &str) -> Access {
        Access {
            kind,
            base: base.into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn astmt(id: usize, access: Access) -> Stmt {
        Stmt { id, kind: StmtKind::Array { access } }
    }

    fn assign(id: usize, target: usize, expr: ScalarExpr) -> Stmt {
        Stmt { id, kind: StmtKind::Assign { target, expr } }
    }

    fn func(params: usize, locals: Vec<&str>, body: Vec<Stmt>, callees: Vec<String>) -> Function {
        Function {
            name: "f".into(),
            params: vec![ParamMode::In; params],
            locals: locals.into_iter().map(String::from).collect(),
            body,
            callees,
        }
    }

    #[test]
    fn a_reader_with_no_calls_is_pure() {
        let f = func(2, vec!["A"], vec![astmt(0, arr(AccessKind::Read, "A"))], vec![]);
        assert_eq!(classify_purity(&f, &PureSet::new()), Purity::Pure);
    }

    #[test]
    fn a_parameter_write_is_a_pure_subroutine() {
        let f = func(3, vec!["A"], vec![assign(0, 2, ScalarExpr::Const(1))], vec![]);
        assert_eq!(
            classify_purity(&f, &PureSet::new()),
            Purity::PureSubroutine { modified: vec![2] }
        );
    }

    #[test]
    fn a_global_write_is_impure() {
        let f = func(1, vec!["A"], vec![astmt(0, arr(AccessKind::Write, "G"))], vec![]);
        assert_eq!(classify_purity(&f, &PureSet::new()), Purity::Impure);
        // A scalar assignment above the parameter count is a global write too.
        let g = func(1, vec!["A"], vec![assign(0, 5, ScalarExpr::Const(1))], vec![]);
        assert_eq!(classify_purity(&g, &PureSet::new()), Purity::Impure);
    }

    #[test]
    fn reading_a_non_parameter_slot_is_impure() {
        let f = func(1, vec!["A"], vec![assign(0, 0, ScalarExpr::Scalar(7))], vec![]);
        assert_eq!(classify_purity(&f, &PureSet::new()), Purity::Impure);
    }

    #[test]
    fn io_makes_a_function_impure() {
        let f = func(1, vec!["A"], vec![astmt(0, arr(AccessKind::Write, "stdout"))], vec![]);
        assert_eq!(classify_purity(&f, &PureSet::new()), Purity::Impure);
    }

    #[test]
    fn unknown_callee_blocks_pure_but_allows_pure_subroutine() {
        let mut pure = PureSet::new();
        let reader =
            func(1, vec!["A"], vec![astmt(0, arr(AccessKind::Read, "A"))], vec!["g".into()]);
        assert_eq!(classify_purity(&reader, &pure), Purity::PureSubroutine { modified: vec![] });
        pure.insert("g");
        assert_eq!(classify_purity(&reader, &pure), Purity::Pure);
    }

    #[test]
    fn purity_round_trips_through_json() {
        for p in [Purity::Pure, Purity::PureSubroutine { modified: vec![1, 2] }, Purity::Impure] {
            let s = serde_json::to_string(&p).unwrap();
            let back: Purity = serde_json::from_str(&s).unwrap();
            assert_eq!(back, p);
        }
    }
}