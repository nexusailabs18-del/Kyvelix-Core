//! Phase 5 — a dependency-agnostic dependence record (Wolfe Ch. 7.5–7.6, 9).
//!
//! Several Phase 5 subsystems (`ir::loopkind`, `fud::alignment`,
//! `fud::crossloop`) reason about a loop given *the set of dependences* it
//! carries. The array-side analysis never materialises that set as one type —
//! `fud::scalardd` produces `ScalarDependence` and the deciding levels produce
//! `Claim`s — so this module is the small, shared vocabulary those subsystems
//! consume.
//!
//! Nothing here is trusted: a `Dependence` is an input a caller derives from the
//! loop (or from the scalar CFG) and hands to a classifier. The certificate
//! vocabulary carries the loop and lets `m0-check` re-derive the same
//! classification, exactly as the other phases do.

use serde::{Deserialize, Serialize};

/// The direction of a dependence (Wolfe §6.7.1 / §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    /// Write → read.
    Flow,
    /// Read → write.
    Anti,
    /// Write → write.
    Output,
    /// Read → read.
    Input,
}

impl DepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DepKind::Flow => "flow",
            DepKind::Anti => "anti",
            DepKind::Output => "output",
            DepKind::Input => "input",
        }
    }

    /// True for dependences that order two *writes* (output) or a write before a
    /// read of the same value (flow). Loop-kind classification treats these as
    /// the "real" ordering constraints.
    pub fn is_write_involving(self) -> bool {
        matches!(self, DepKind::Flow | DepKind::Anti | DepKind::Output)
    }
}

/// A dependence between two statements of one loop nest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependence {
    /// Index of the statement that must execute first.
    pub src: usize,
    /// Index of the statement that must execute second.
    pub dst: usize,
    pub kind: DepKind,
    /// The dependence distance, one component per loop level (outermost first).
    pub distance: Vec<i64>,
    /// True when the pair *might* be dependent but the analysis could not
    /// prove it (a `Decision::Unknown`). Such an edge is conservative: it must
    /// be treated as a carried dependence, because a classifier that assumed
    /// independence would be unsound. `new` sets this to `false`.
    #[serde(default)]
    pub unknown: bool,
}

impl Dependence {
    pub fn new(src: usize, dst: usize, kind: DepKind, distance: Vec<i64>) -> Dependence {
        Dependence { src, dst, kind, distance, unknown: false }
    }

    /// A conservative edge for a pair the analysis could not decide: it may be
    /// any dependence, so it forces the ordering to be preserved.
    pub fn unknown(src: usize, dst: usize, kind: DepKind) -> Dependence {
        Dependence { src, dst, kind, distance: vec![], unknown: true }
    }

    /// True when every distance component is zero: the dependence is
    /// loop-independent (Wolfe §5.1). An `unknown` edge is never reported
    /// independent.
    pub fn is_loop_independent(&self) -> bool {
        !self.unknown && self.distance.iter().all(|&d| d == 0)
    }

    /// True when some component is non-zero: the dependence crosses iterations.
    pub fn is_loop_carried(&self) -> bool {
        !self.is_loop_independent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_independence_is_all_zero() {
        assert!(Dependence::new(0, 1, DepKind::Flow, vec![0, 0]).is_loop_independent());
        assert!(Dependence::new(0, 1, DepKind::Flow, vec![]).is_loop_independent());
        assert!(!Dependence::new(0, 1, DepKind::Flow, vec![1]).is_loop_independent());
        assert!(Dependence::new(0, 1, DepKind::Flow, vec![1]).is_loop_carried());
    }

    #[test]
    fn kind_round_trips_through_json() {
        for k in [DepKind::Flow, DepKind::Anti, DepKind::Output, DepKind::Input] {
            let d = Dependence::new(1, 2, k, vec![3]);
            let s = serde_json::to_string(&d).unwrap();
            let back: Dependence = serde_json::from_str(&s).unwrap();
            assert_eq!(back, d);
        }
    }
}