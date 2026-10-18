//! Phase 7 §7.5/§7.8 — the alias query (Wolfe §8.2.2).
//!
//! The points-to graph answers "where does `p` point?"; the alias query answers
//! the question the dependence machinery actually asks: "can `A[p]` and `A[q]`
//! be the same location?". Three answers, and the whole soundness argument is
//! that `NoAlias` and `MustAlias` are only ever returned when the graph forces
//! them:
//!
//! - [`Alias::MustAlias`] — both pointers definitely point at the same single
//!   target.
//! - [`Alias::NoAlias`] — their target sets are disjoint and neither may be
//!   unknown.
//! - [`Alias::MayAlias`] — everything else, including every unknown.
//!
//! For pointers that reconstruct into array references ([`ReconstructedRef`]),
//! disjointness is decided on the `(array, stride, offset)` triples by
//! [`arrays_disjoint`], with [`enumerate_alias`] as the independent oracle the
//! fuzz suite compares against.

use serde::{Deserialize, Serialize};

use crate::points::ptrarith::ReconstructedRef;
use crate::points::relation::{PointsToGraph, VarId};

/// The three-way alias answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alias {
    /// Definitely the same location.
    MustAlias,
    /// Possibly the same location.
    MayAlias,
    /// Definitely different locations.
    NoAlias,
}

impl Alias {
    pub fn as_str(self) -> &'static str {
        match self {
            Alias::MustAlias => "must_alias",
            Alias::MayAlias => "may_alias",
            Alias::NoAlias => "no_alias",
        }
    }

    pub fn is_must(self) -> bool {
        matches!(self, Alias::MustAlias)
    }

    pub fn is_no(self) -> bool {
        matches!(self, Alias::NoAlias)
    }

    /// True when the two locations are definitely distinct.
    pub fn is_disjoint(self) -> bool {
        matches!(self, Alias::NoAlias)
    }
}

/// The alias relation between two pointer variables, read off the graph.
pub fn query(g: &PointsToGraph, p: VarId, q: VarId) -> Alias {
    if p == q {
        return Alias::MustAlias;
    }
    let tp = g.targets_of(p);
    let tq = g.targets_of(q);
    // A pointer with no relations, or one that may be `unknown`, may point
    // anywhere: never claim disjointness for it.
    if tp.is_empty() || tq.is_empty() || g.may_be_unknown(p) || g.may_be_unknown(q) {
        return Alias::MayAlias;
    }
    if tp.len() == 1 && tq.len() == 1 && tp == tq {
        let dp = g.relations_of(p).and_then(|r| r.iter().next()).map(|(_, d)| *d);
        let dq = g.relations_of(q).and_then(|r| r.iter().next()).map(|(_, d)| *d);
        if dp.map(|d| d.is_definite()).unwrap_or(false)
            && dq.map(|d| d.is_definite()).unwrap_or(false)
        {
            return Alias::MustAlias;
        }
        return Alias::MayAlias;
    }
    if tp.intersection(&tq).next().is_some() {
        Alias::MayAlias
    } else {
        Alias::NoAlias
    }
}

/// Are two reconstructed array references provably disjoint?
///
/// Sound, not complete: it returns `true` only for the two shapes it can settle
/// — different arrays, or two constant (zero-stride) references to different
/// elements of the same array. Every strided case is `false` (unknown), which
/// the caller turns into `MayAlias`.
pub fn arrays_disjoint(a: &ReconstructedRef, b: &ReconstructedRef) -> bool {
    if a.array != b.array {
        return true;
    }
    if a.coeff == 0 && b.coeff == 0 {
        return a.offset != b.offset;
    }
    false
}

/// The independent oracle: decide the alias relation by enumerating both
/// references over `i, j ∈ [lo, hi)`.
///
/// This is what the fuzz suite compares [`arrays_disjoint`] against — a fact
/// proved by brute force cannot share a bug with the algebraic rule.
pub fn enumerate_alias(a: &ReconstructedRef, b: &ReconstructedRef, lo: i64, hi: i64) -> Alias {
    if a.array != b.array {
        return Alias::NoAlias;
    }
    let mut collides = false;
    let mut i = lo;
    while i < hi {
        let ai = a.index_at(i);
        let mut j = lo;
        while j < hi {
            if ai == b.index_at(j) {
                collides = true;
            }
            j += 1;
        }
        i += 1;
    }
    if !collides {
        Alias::NoAlias
    } else if a.coeff == 0 && b.coeff == 0 && a.offset == b.offset {
        Alias::MustAlias
    } else {
        Alias::MayAlias
    }
}

/// The alias answer for two references over a loop domain, combining the
/// algebraic rule with a domain-aware fallback.
///
/// Unlike [`enumerate_alias`], this never enumerates a large domain: it settles
/// the same-array strided case only when the arithmetic proves disjointness, and
/// otherwise says `MayAlias`.
pub fn query_refs(a: &ReconstructedRef, b: &ReconstructedRef) -> Alias {
    if arrays_disjoint(a, b) {
        Alias::NoAlias
    } else if a.array == b.array && a.coeff == 0 && b.coeff == 0 && a.offset == b.offset {
        Alias::MustAlias
    } else {
        Alias::MayAlias
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::points::relation::Definiteness;

    fn v(n: u32) -> VarId {
        VarId(n)
    }

    fn r(array: u32, coeff: i64, offset: i64) -> ReconstructedRef {
        ReconstructedRef {
            array: v(array),
            coeff,
            offset,
            elem_size: 8,
            pointer: v(99),
        }
    }

    #[test]
    fn identical_pointer_is_must_alias() {
        let g = PointsToGraph::empty();
        assert_eq!(query(&g, v(1), v(1)), Alias::MustAlias);
    }

    #[test]
    fn disjoint_definite_targets_are_no_alias() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(10), Definiteness::Definite);
        g.add_relation(v(2), v(11), Definiteness::Definite);
        assert_eq!(query(&g, v(1), v(2)), Alias::NoAlias);
    }

    #[test]
    fn same_definite_target_is_must_alias() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(10), Definiteness::Definite);
        g.add_relation(v(2), v(10), Definiteness::Definite);
        assert_eq!(query(&g, v(1), v(2)), Alias::MustAlias);
    }

    #[test]
    fn unknown_is_never_reported_disjoint() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(10), Definiteness::Definite);
        g.add_relation(v(2), g.unknown, Definiteness::Possible);
        assert_eq!(query(&g, v(1), v(2)), Alias::MayAlias);
    }

    #[test]
    fn shared_target_is_may_alias() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(10), Definiteness::Possible);
        g.add_relation(v(2), v(10), Definiteness::Possible);
        assert_eq!(query(&g, v(1), v(2)), Alias::MayAlias);
    }

    #[test]
    fn disjoint_arrays_are_no_alias() {
        assert!(arrays_disjoint(&r(1, 1, 0), &r(2, 1, 0)));
        // Same array, constant refs to different elements.
        assert!(arrays_disjoint(&r(1, 0, 3), &r(1, 0, 4)));
        // Same array, a strided ref: not provable.
        assert!(!arrays_disjoint(&r(1, 2, 0), &r(1, 1, 0)));
    }

    #[test]
    fn enumerate_oracle_agrees_on_the_constant_cases() {
        assert_eq!(enumerate_alias(&r(1, 0, 0), &r(1, 0, 1), 0, 8), Alias::NoAlias);
        assert_eq!(enumerate_alias(&r(1, 0, 2), &r(1, 0, 2), 0, 8), Alias::MustAlias);
        assert_eq!(enumerate_alias(&r(1, 2, 0), &r(1, 1, 0), 0, 8), Alias::MayAlias);
        assert_eq!(enumerate_alias(&r(1, 1, 0), &r(2, 1, 0), 0, 8), Alias::NoAlias);
    }

    #[test]
    fn query_refs_never_claims_disjoint_for_a_strided_same_array_pair() {
        // coeff 2 vs coeff 2, same offset → same locations → MayAlias (not No).
        assert_eq!(query_refs(&r(1, 2, 0), &r(1, 2, 0)), Alias::MayAlias);
    }
}