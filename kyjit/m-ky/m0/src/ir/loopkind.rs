//! Phase 5 §5.1.11 — loop-kind classification (Wolfe §7.5, Ch. 9).
//!
//! Given the dependences a loop carries, classify it into the kind that says how
//! it may be parallelised (Wolfe Ch. 9; the SPP Layer 2 consumer).
//!
//! ```text
//! Forall:    RHS computed before LHS. Negative distances possible.
//! Dopar:     each iteration copies variables. No flow dependences between iterations.
//! Doall:     dopar with no dependences.
//! Dosingle:  single-assignment rule. No output/anti dependences.
//! Sequential: standard imperative loop.
//! ```
//!
//! The classification is a total order of decreasing freedom: `Doall` (nothing
//! to order) ⊃ `Dopar` (only anti/output, which variable copying handles) ⊃
//! `Forall` (flow, but reversed) ⊃ `Dosingle` (flow, no output/anti) ⊃
//! `Sequential` (everything else). Each step names the exact dependence fact
//! that forces it, so a caller can explain why a loop is not parallel.

use serde::{Deserialize, Serialize};

use crate::ir::dependence::{DepKind, Dependence};

/// How a loop may be parallelised (Wolfe Ch. 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoopKind {
    /// A standard imperative loop: no parallelisation implied.
    Sequential,
    /// Reverse-serial (`forall`): flow dependences run backwards (negative
    /// distance), so the loop is the reverse of a serial loop.
    Forall,
    /// Parallel-do (`dopar`): no flow dependences cross iterations; anti/output
    /// dependences are handled by copying the shared variable per iteration.
    Dopar,
    /// Parallel-do-all (`doall`): no dependences at all.
    Doall,
    /// Single-assignment (`dosingle`): flow only, no output/anti dependences.
    Dosingle,
}

impl LoopKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LoopKind::Sequential => "sequential",
            LoopKind::Forall => "forall",
            LoopKind::Dopar => "dopar",
            LoopKind::Doall => "doall",
            LoopKind::Dosingle => "dosingle",
        }
    }

    /// True for the kinds that carry no ordering constraint at all.
    pub fn is_fully_parallel(self) -> bool {
        matches!(self, LoopKind::Doall)
    }
}

/// §5.1.11: classify a loop by the dependences it carries.
///
/// `loop_` is accepted to match the spec's signature and to keep the caller's
/// intent (a loop-kind claim is about *this* loop); the classification reads the
/// `dependences`, which the caller derives from that same loop.
pub fn classify_loop_kind(_loop_: &crate::ir::Loop, dependences: &[Dependence]) -> LoopKind {
    // Doall: nothing to order.
    if dependences.is_empty() {
        return LoopKind::Doall;
    }

    // Dopar: no flow dependence crosses an iteration.
    let carried_flow: Vec<&Dependence> = dependences
        .iter()
        .filter(|d| d.kind == DepKind::Flow && d.is_loop_carried())
        .collect();
    if carried_flow.is_empty() {
        return LoopKind::Dopar;
    }

    // Forall: every carried flow runs backwards (RHS computed before LHS).
    if carried_flow
        .iter()
        .all(|d| d.distance.iter().any(|&x| x < 0))
    {
        return LoopKind::Forall;
    }

    // Dosingle: flow only — the single-assignment rule excludes output/anti.
    let any_output_anti = dependences
        .iter()
        .any(|d| matches!(d.kind, DepKind::Output | DepKind::Anti));
    if !any_output_anti {
        return LoopKind::Dosingle;
    }

    LoopKind::Sequential
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IndexKind, IV, Loop};

    fn dep(src: usize, dst: usize, kind: DepKind, distance: Vec<i64>) -> Dependence {
        Dependence::new(src, dst, kind, distance)
    }

    fn loop_() -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 8, 1)],
            body: vec![],
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    #[test]
    fn no_dependences_is_doall() {
        assert_eq!(classify_loop_kind(&loop_(), &[]), LoopKind::Doall);
    }

    #[test]
    fn loop_independent_dependences_are_dopar() {
        let deps = vec![dep(0, 1, DepKind::Flow, vec![0])];
        assert_eq!(classify_loop_kind(&loop_(), &deps), LoopKind::Dopar);
    }

    #[test]
    fn anti_and_output_only_is_dopar() {
        let deps = vec![dep(0, 1, DepKind::Anti, vec![1]), dep(1, 0, DepKind::Output, vec![1])];
        assert_eq!(classify_loop_kind(&loop_(), &deps), LoopKind::Dopar);
    }

    #[test]
    fn backward_carried_flow_is_forall() {
        let deps = vec![dep(0, 1, DepKind::Flow, vec![-1])];
        assert_eq!(classify_loop_kind(&loop_(), &deps), LoopKind::Forall);
    }

    #[test]
    fn forward_flow_without_output_anti_is_dosingle() {
        let deps = vec![dep(0, 1, DepKind::Flow, vec![1])];
        assert_eq!(classify_loop_kind(&loop_(), &deps), LoopKind::Dosingle);
    }

    #[test]
    fn forward_flow_with_output_is_sequential() {
        let deps = vec![dep(0, 1, DepKind::Flow, vec![1]), dep(1, 2, DepKind::Output, vec![1])];
        assert_eq!(classify_loop_kind(&loop_(), &deps), LoopKind::Sequential);
    }

    #[test]
    fn loop_kind_round_trips() {
        let _ = IndexKind::Affine;
        for k in [
            LoopKind::Sequential,
            LoopKind::Forall,
            LoopKind::Dopar,
            LoopKind::Doall,
            LoopKind::Dosingle,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: LoopKind = serde_json::from_str(&s).unwrap();
            assert_eq!(back, k);
        }
    }
}