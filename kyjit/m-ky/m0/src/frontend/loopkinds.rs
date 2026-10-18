//! Phase 9 §9.10 — loop-kind classification, on the frontend side.
//!
//! [`crate::ir::loopkind`] answers *given the dependences a loop carries, how may
//! it be parallelised?* That question is about the IR. §9.10 asks the frontend
//! question that comes before it: **which loop construct is this, and what does
//! the IR do with it?**
//!
//! The two are kept apart on purpose. Recognising `for (const v of A)` is a
//! syntactic fact this module decides alone; calling the loop `Doall` is a claim
//! about dependences that only the deciders can support.
//!
//! ## The one trap, stated loudly
//!
//! [`crate::ir::loopkind::classify_loop_kind`] returns `Doall` for an **empty**
//! dependence slice. That is correct at the IR level — a loop whose dependences
//! were computed and came out empty has nothing to order — and wrong here, where
//! "no dependences" would mean "not computed yet". So [`loop_kind`] is only for a
//! caller that really has the dependences, and [`unknown_kind`] is what to claim
//! otherwise. Passing `&[]` because nothing is known would claim full
//! parallelisability for every loop in the program.

use crate::frontend::ast::{Expr, ForHead, Position, Stmt};
use crate::ir::dependence::Dependence;
use crate::ir::loopkind::{classify_loop_kind, LoopKind};
use crate::ir::Loop;

/// The loop constructs the frontend recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLoop {
    /// `for (init; cond; update)` — §9.4's counted-loop row.
    Counted,
    /// `for (const v of A)` — §9.7's rewriting.
    ForOf,
    /// `for (const k in o)` — no iteration space the IR can state.
    ForIn,
    /// `while (c)`.
    While,
    /// `do { } while (c)`.
    DoWhile,
}

impl SourceLoop {
    /// The construct's name, as §9.14 reports it.
    pub fn name(self) -> &'static str {
        match self {
            SourceLoop::Counted => "for",
            SourceLoop::ForOf => "for-of",
            SourceLoop::ForIn => "for-in",
            SourceLoop::While => "while",
            SourceLoop::DoWhile => "do-while",
        }
    }

    /// Whether lowering can state this construct's iteration space.
    ///
    /// `false` does not mean "never parallelisable" — it means the frontend has
    /// no `IV` chain to hand the deciders, so no kind can be claimed at all.
    pub fn lowers(self) -> bool {
        matches!(self, SourceLoop::Counted | SourceLoop::ForOf)
    }

    /// The kind to claim for a construct the frontend cannot lower.
    pub fn kind_when_unlowered(self) -> LoopKind {
        LoopKind::Sequential
    }
}

/// Which loop construct `s` is, if it is one.
pub fn classify_source(s: &Stmt) -> Option<SourceLoop> {
    match s {
        Stmt::For { head, .. } => match head {
            // Only one head kind exists today; the match keeps this total in the
            // face of another being added.
            ForHead::Classical { .. } => Some(SourceLoop::Counted),
        },
        Stmt::ForOf { .. } => Some(SourceLoop::ForOf),
        Stmt::ForIn { .. } => Some(SourceLoop::ForIn),
        Stmt::While { .. } => Some(SourceLoop::While),
        Stmt::DoWhile { .. } => Some(SourceLoop::DoWhile),
        _ => None,
    }
}

/// Every loop construct in a statement list, in source order, with its position.
///
/// Blocks, function bodies and `if` arms are descended into, because §9.10
/// classifies *loops*, not statements.
pub fn classify_body(stmts: &[Stmt]) -> Vec<(Position, SourceLoop)> {
    let mut out = Vec::new();
    for s in stmts {
        collect(s, &mut out);
    }
    out
}

fn collect(s: &Stmt, out: &mut Vec<(Position, SourceLoop)>) {
    if let Some(k) = classify_source(s) {
        out.push((s.pos(), k));
    }
    match s {
        Stmt::Block(v, _) => {
            for x in v {
                collect(x, out);
            }
        }
        Stmt::FunctionDecl { body, .. } => {
            for x in body {
                collect(x, out);
            }
        }
        Stmt::If { then, els, .. } => {
            collect(then, out);
            if let Some(e) = els {
                collect(e, out);
            }
        }
        // A loop's body is descended into too: nested loops are loops.
        Stmt::For { body, .. }
        | Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForIn { body, .. } => collect(body, out),
        Stmt::Try { body, .. } => {
            for x in body {
                collect(x, out);
            }
        }
        Stmt::Export { decl, .. } => collect(decl, out),
        _ => {}
    }
}

/// The kind of a nest whose dependences **have been computed**.
///
/// A thin re-export, so the frontend and the certificate cannot drift into two
/// different classifications of the same loop.
pub fn loop_kind(nest: &Loop, dependences: &[Dependence]) -> LoopKind {
    classify_loop_kind(nest, dependences)
}

/// The kind to claim for a nest whose dependences have **not** been computed.
///
/// `Sequential`, deliberately: see the module docs. The alternative — passing an
/// empty slice to [`loop_kind`] — reads as "nothing to order" and would report
/// `Doall` for every loop in the program.
pub fn unknown_kind() -> LoopKind {
    LoopKind::Sequential
}

/// A construct the frontend cannot lower: the kind to record, and why.
pub fn unlowered_kind(k: SourceLoop) -> (LoopKind, &'static str) {
    (
        k.kind_when_unlowered(),
        match k {
            SourceLoop::ForOf => {
                "a for-of lowers only over a named array whose length is a parameter (§9.7)"
            }
            SourceLoop::ForIn => "for-in iterates keys, which no `IV` chain states",
            SourceLoop::While => "a while's trip count is not a function of a counter",
            SourceLoop::DoWhile => "a do-while's trip count is not a function of a counter",
            SourceLoop::Counted => {
                "a counted loop lowers unless its head is outside the affine subset"
            }
        },
    )
}

/// The `Expr` a loop construct tests, when it has one.
pub fn condition_of(s: &Stmt) -> Option<&Expr> {
    match s {
        Stmt::For { head, .. } => match head {
            ForHead::Classical { cond, .. } => cond.as_ref(),
        },
        Stmt::While { cond, .. } => Some(cond),
        Stmt::DoWhile { cond, .. } => Some(cond),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lower::{lower_source, Env};
    use crate::frontend::parse;

    /// The body of the first function in `src`.
    fn fn_body(src: &str) -> Vec<Stmt> {
        parse(src)
            .expect("parses")
            .body
            .into_iter()
            .find_map(|s| match s {
                Stmt::FunctionDecl { body, .. } => Some(body),
                _ => None,
            })
            .expect("a function declaration")
    }

    fn kinds(src: &str) -> Vec<SourceLoop> {
        classify_body(&fn_body(src)).into_iter().map(|(_, k)| k).collect()
    }

    #[test]
    fn every_construct_is_recognised() {
        let src = "
            function f(A) {
                for (let i = 0; i < 8; i++) { A[i] = 0; }
                for (const v of A) { A[0] = 0; }
                for (const k in A) { }
                while (a) { }
                do { } while (a);
            }";
        assert_eq!(
            kinds(src),
            vec![
                SourceLoop::Counted,
                SourceLoop::ForOf,
                SourceLoop::ForIn,
                SourceLoop::While,
                SourceLoop::DoWhile,
            ]
        );
    }

    #[test]
    fn only_the_constructs_lowering_can_state_report_lowers() {
        assert!(SourceLoop::Counted.lowers());
        assert!(SourceLoop::ForOf.lowers());
        for k in [SourceLoop::ForIn, SourceLoop::While, SourceLoop::DoWhile] {
            assert!(!k.lowers(), "{k:?} has no IV chain to hand the deciders");
            assert_eq!(k.kind_when_unlowered(), LoopKind::Sequential);
            let (kind, why) = unlowered_kind(k);
            assert_eq!(kind, LoopKind::Sequential);
            assert!(why.contains(k.name()), "`{why}` should name `{}`", k.name());
        }
    }

    #[test]
    fn nested_loops_are_all_reported_in_source_order() {
        let src = "
            function f(A) {
                for (let i = 0; i < 8; i++) {
                    for (const v of A) { }
                    if (a) { while (b) { } }
                }
            }";
        // The middle `if` is not a loop; a loop inside an `if` still is.
        assert_eq!(kinds(src), vec![SourceLoop::Counted, SourceLoop::ForOf, SourceLoop::While]);
    }

    #[test]
    fn an_uncomputed_dependence_set_must_not_read_as_doall() {
        // The trap the module docs name, pinned: the IR-level answer for an empty
        // slice is `Doall`, so the frontend must not pass one when nothing was
        // computed. This test fails the day the two are conflated.
        let k = lower_source(
            "function f(A) { for (let i = 0; i < 8; i++) { A[i] = 0; } }",
            &Env::empty(),
        )
        .expect("parses");
        assert_eq!(k.loops.len(), 1);
        assert_eq!(
            loop_kind(&k.loops[0], &[]),
            LoopKind::Doall,
            "the IR-level reading of a computed-and-empty set"
        );
        assert_eq!(unknown_kind(), LoopKind::Sequential, "the frontend's answer");
        assert_ne!(unknown_kind(), loop_kind(&k.loops[0], &[]), "never conflate the two");
    }

    #[test]
    fn condition_of_names_the_test_of_each_construct_but_not_a_for_of_iterable() {
        let body = fn_body(
            "function f(A) {
                for (let i = 0; i < 8; i++) { }
                for (const v of A) { }
                while (a) { }
                do { } while (a);
            }",
        );
        let mut tested = 0;
        let mut iterable_only = 0;
        for s in &body {
            match classify_source(s) {
                Some(SourceLoop::ForOf) => {
                    assert!(condition_of(s).is_none(), "a for-of tests no condition");
                    iterable_only += 1;
                }
                Some(_) => {
                    assert!(condition_of(s).is_some(), "{:?} has a test", classify_source(s));
                    tested += 1;
                }
                None => panic!("not a loop: {}", s.pos()),
            }
        }
        assert_eq!((tested, iterable_only), (3, 1));
    }
}
