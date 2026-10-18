//! Phase 7 §7.7 — allocation dependence (Wolfe §8.2.4).
//!
//! > "Two dynamic allocation statements: order matters for memory addresses.
//! >  Fortran 90: no syntax to get the address of a pointer target; location
//! >  independence assured. C: can access/modify addresses; if the order is
//! >  important, the compiler must preserve it; insert dependence relations
//! >  between allocation statements."
//!
//! The difference is not in the allocator — it is in whether a program can
//! *observe* where an allocation landed. F90 has no address-of on a pointer
//! target, so two allocations commute. C does, so if either result escapes (its
//! address is taken, or it reaches a call the analysis cannot see through), the
//! two allocations do not commute and the compiler must keep their order.
//!
//! "Escapes" is exactly "may be `unknown` in the points-to graph": an address
//! that reaches an untracked place is one whose numeric value could be compared.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::points::relation::{PointsToGraph, VarId};

/// Which language's allocation semantics apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// No address escape for a pointer target: allocation order is independent.
    Fortran90,
    /// Address escape allowed: observable order must be preserved.
    C,
}

/// A dynamic allocation, named by the variable that receives its result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocStmt {
    /// The variable holding the allocated object.
    pub result: VarId,
    /// A stable index for diagnostics and for `AliasBasis::AllocationOrder`.
    pub site: usize,
}

impl AllocStmt {
    pub fn new(result: VarId, site: usize) -> AllocStmt {
        AllocStmt { result, site }
    }
}

/// The dependence between two allocations: none, or a loop-independent order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocDependence {
    /// The allocations commute.
    None,
    /// The order is observable and must be preserved.
    LoopIndependent,
}

/// The dependence between `a1` and `a2` under `lang`'s allocation semantics,
/// given the set of escaping variables.
///
/// In F90 the answer is always [`AllocDependence::None`]. In C it is
/// [`AllocDependence::LoopIndependent`] iff either result escapes, and `None`
/// otherwise.
pub fn allocation_dependence(
    lang: Language,
    a1: &AllocStmt,
    a2: &AllocStmt,
    escaped: &BTreeSet<VarId>,
) -> AllocDependence {
    match lang {
        Language::Fortran90 => AllocDependence::None,
        Language::C => {
            if escaped.contains(&a1.result) || escaped.contains(&a2.result) {
                AllocDependence::LoopIndependent
            } else {
                AllocDependence::None
            }
        }
    }
}

/// True when `v` escapes: its address can be observed because it may point at
/// something the analysis does not track.
pub fn escapes(g: &PointsToGraph, v: VarId) -> bool {
    g.may_be_unknown(v)
}

/// Every variable that escapes, as the set [`allocation_dependence`] consumes.
pub fn escaping_vars(g: &PointsToGraph) -> BTreeSet<VarId> {
    g.pointers().filter(|p| g.may_be_unknown(*p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::points::relation::Definiteness;

    fn v(n: u32) -> VarId {
        VarId(n)
    }

    #[test]
    fn fortran_allocation_order_is_always_independent() {
        let mut esc = BTreeSet::new();
        esc.insert(v(1));
        assert_eq!(
            allocation_dependence(Language::Fortran90, &AllocStmt::new(v(1), 0), &AllocStmt::new(v(2), 1), &esc),
            AllocDependence::None
        );
    }

    #[test]
    fn c_order_matters_when_a_result_escapes() {
        let mut esc = BTreeSet::new();
        esc.insert(v(2));
        assert_eq!(
            allocation_dependence(Language::C, &AllocStmt::new(v(1), 0), &AllocStmt::new(v(2), 1), &esc),
            AllocDependence::LoopIndependent
        );
    }

    #[test]
    fn c_order_is_free_when_nothing_escapes() {
        let esc = BTreeSet::new();
        assert_eq!(
            allocation_dependence(Language::C, &AllocStmt::new(v(1), 0), &AllocStmt::new(v(2), 1), &esc),
            AllocDependence::None
        );
    }

    #[test]
    fn escapes_follows_the_unknown_target() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(10), Definiteness::Definite);
        g.add_relation(v(2), g.unknown, Definiteness::Possible);
        assert!(!escapes(&g, v(1)));
        assert!(escapes(&g, v(2)));
        let e = escaping_vars(&g);
        assert!(e.contains(&v(2)));
        assert!(!e.contains(&v(1)));
    }
}