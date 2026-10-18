//! Phase 7 §7.3 — the four assignment cases of Wolfe §8.2.1.
//!
//! Wolfe enumerates four cases for an assignment. They become the six effects
//! below, because two of the four ("LHS definite" and "LHS possible") split into
//! a deref store and a plain pointer copy:
//!
//! 1. **LHS definite target** — `*p = *q` with `p` pointing at exactly one
//!    object: kill every relation of `p`, then add `p → q` definite.
//! 2. **LHS possible targets** — `*p = *q` with `p` pointing at several: the
//!    written objects may be any of `p`'s targets, so weaken each to `Possible`
//!    and add `Possible` relations from every LHS target to every RHS target.
//! 3. **Both RHS and LHS definite** — `p = q` copies `q`'s relations onto `p`.
//! 4. **Otherwise** — same as case 2.
//!
//! `AddressOf` (`p = &x`) is Wolfe's definite-target case with a known target,
//! `Kill` (`p = null`) is its degenerate form, and `Clobber` is "some call we
//! cannot see through".
//!
//! Every effect is *monotone* in the direction the fixpoint needs: it only
//! weakens definiteness or adds targets, never strengthens. That is what makes
//! [`crate::points::fixpoint::analyze`] terminate without a widening operator.

use serde::{Deserialize, Serialize};

use crate::points::relation::{Definiteness, PointsToGraph, VarId};

/// An assignment's effect on the points-to graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// `*p = *q` with `p` definite: `lhs` is `p`, `rhs` is the *target* of `q`.
    DefiniteLhs { lhs: VarId, rhs: VarId },
    /// `*p = *q` with `p` possible: `lhs_targets` are `p`'s targets,
    /// `rhs_targets` are the targets of `q`.
    PossibleLhs {
        lhs_targets: Vec<VarId>,
        rhs_targets: Vec<VarId>,
    },
    /// `p = q` (a pointer copy, not a deref).
    PointerAssign { lhs: VarId, rhs: VarId },
    /// `p = &x`, so `p` definitely points at `x`.
    AddressOf { lhs: VarId, target: VarId },
    /// `p = null` (or any assignment that gives `p` no target).
    Kill { lhs: VarId },
    /// A call or unknown statement: every pointer's relations weaken and reach
    /// the unknown target.
    Clobber,
}

/// The `Effect` a native `p = q` assignment has, given the graph *before* the
/// assignment decides which case applies.
///
/// This is the one place the four cases are chosen, so the fixpoint and the
/// tests agree on the selection: `q` definite-singleton → `PointerAssign`
/// (case 3), otherwise case 4's conservative `PossibleLhs`.
pub fn effect_of_pointer_copy(g: &PointsToGraph, lhs: VarId, rhs: VarId) -> Effect {
    match g.relations_of(rhs) {
        Some(rels) if rels.iter().all(|(_, d)| d.is_definite()) => {
            Effect::PointerAssign { lhs, rhs }
        }
        _ => {
            let rhs_targets: Vec<VarId> = g.targets_of(rhs).into_iter().collect();
            let lhs_targets: Vec<VarId> = g.targets_of(lhs).into_iter().collect();
            Effect::PossibleLhs {
                lhs_targets,
                rhs_targets,
            }
        }
    }
}

/// Apply one effect to the graph. Monotone: definiteness only decreases.
pub fn apply_effect(g: &mut PointsToGraph, e: &Effect) -> bool {
    match e {
        Effect::DefiniteLhs { lhs, rhs } => {
            // Case 1: kill all previous relations of `lhs`, then add definite.
            let mut changed = g.kill_targets_of(*lhs);
            changed |= g.add_relation(*lhs, *rhs, Definiteness::Definite);
            changed
        }
        Effect::PossibleLhs {
            lhs_targets,
            rhs_targets,
        } => {
            let mut changed = false;
            // Case 2: the objects `lhs` may point at are all written, so any
            // pointer stored in them may now hold anything → weaken.
            for t in lhs_targets {
                changed |= g.weaken_to_possible(*t);
            }
            // Case 4: add possible relations from every LHS to every RHS target.
            for lt in lhs_targets {
                for rt in rhs_targets {
                    changed |= g.add_relation(*lt, *rt, Definiteness::Possible);
                }
            }
            changed
        }
        Effect::PointerAssign { lhs, rhs } => {
            // Case 3: copy the RHS's relations (with their definiteness) onto
            // the LHS, killing the LHS's old ones first.
            let r_rels = g.relations_of(*rhs).cloned().unwrap_or_default();
            let mut changed = g.kill_targets_of(*lhs);
            for (target, d) in r_rels {
                changed |= g.add_relation(*lhs, target, d);
            }
            changed
        }
        Effect::AddressOf { lhs, target } => {
            let mut changed = g.kill_targets_of(*lhs);
            changed |= g.add_relation(*lhs, *target, Definiteness::Definite);
            changed
        }
        Effect::Kill { lhs } => g.kill_targets_of(*lhs),
        Effect::Clobber => g.clobber_all(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: u32) -> VarId {
        VarId(n)
    }

    #[test]
    fn definite_lhs_kills_then_adds_definite() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(9), Definiteness::Definite);
        apply_effect(&mut g, &Effect::DefiniteLhs { lhs: v(0), rhs: v(1) });
        assert!(g.must_point_to(v(0), v(1)));
        assert!(!g.may_point_to(v(0), v(9)));
    }

    #[test]
    fn possible_lhs_weakens_and_adds_possible() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(7), Definiteness::Definite);
        apply_effect(
            &mut g,
            &Effect::PossibleLhs {
                lhs_targets: vec![v(1)],
                rhs_targets: vec![v(2), v(3)],
            },
        );
        // v(1)'s own relation weakened to Possible …
        assert!(!g.must_point_to(v(1), v(7)));
        // … and both RHS targets were added Possibly.
        assert!(g.may_point_to(v(1), v(2)));
        assert!(g.may_point_to(v(1), v(3)));
        assert!(!g.must_point_to(v(1), v(2)));
    }

    #[test]
    fn pointer_assign_copies_definiteness() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(5), Definiteness::Definite);
        g.add_relation(v(0), v(8), Definiteness::Definite);
        apply_effect(&mut g, &Effect::PointerAssign { lhs: v(0), rhs: v(1) });
        assert!(g.must_point_to(v(0), v(5)));
        assert!(!g.may_point_to(v(0), v(8)));
    }

    #[test]
    fn address_of_is_definite() {
        let mut g = PointsToGraph::empty();
        apply_effect(&mut g, &Effect::AddressOf { lhs: v(0), target: v(3) });
        assert!(g.must_point_to(v(0), v(3)));
    }

    #[test]
    fn kill_removes_all_relations() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(1), Definiteness::Definite);
        g.add_relation(v(0), v(2), Definiteness::Possible);
        apply_effect(&mut g, &Effect::Kill { lhs: v(0) });
        assert_eq!(g.relation_count(), 0);
    }

    #[test]
    fn clobber_reaches_unknown() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(1), Definiteness::Definite);
        apply_effect(&mut g, &Effect::Clobber);
        assert!(g.may_be_unknown(v(0)));
        assert!(!g.must_point_to(v(0), v(1)));
    }

    #[test]
    fn effect_of_pointer_copy_picks_case_3_only_for_definite_rhs() {
        let mut g = PointsToGraph::empty();
        assert!(matches!(
            effect_of_pointer_copy(&g, v(0), v(1)),
            Effect::PossibleLhs { .. }
        ));
        g.add_relation(v(1), v(2), Definiteness::Definite);
        assert!(matches!(
            effect_of_pointer_copy(&g, v(0), v(1)),
            Effect::PointerAssign { .. }
        ));
    }
}