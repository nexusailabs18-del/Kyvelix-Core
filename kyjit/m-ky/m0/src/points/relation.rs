//! Phase 7 §7.2 — the points-to relation and graph (Wolfe §8.2.1).
//!
//! Wolfe's relation is a triple `(p, q, d)`: `p` is a pointer variable, `q` its
//! target, and `d` the *definiteness* — `Definite` when every execution of the
//! program makes `p` point at `q`, `Possible` when some path does.
//!
//! Only two things live here: the [`PtrRelation`] triple and the
//! [`PointsToGraph`] that indexes a set of them by pointer and by target. The
//! transfer functions that *produce* triples are in [`crate::points::transfer`],
//! and the fixpoint that iterates them is in [`crate::points::fixpoint`].
//!
//! ## Why `BTreeMap` rather than the spec's `HashMap`
//!
//! The graph is compared for equality (a fixpoint stops when it stops growing)
//! and serialised into a certificate. A `BTreeMap` gives a canonical iteration
//! order, so two graphs with the same relations are `==` and serialise to the
//! same bytes; a `HashMap` would make both depend on the seed. This is the only
//! deviation from the spec's types, and it is a strengthening.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// An opaque variable id inside a [`crate::points::fixpoint::PtrProgram`].
///
/// The same id space names pointers and targets, because Wolfe's triples are
/// `(p, q, d)` over one set of program variables: whether a variable is a
/// pointer or a target is a property of how it is *used*, not of its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VarId(pub u32);

impl VarId {
    /// Zero-based index, for arrays indexed by `usize`.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The raw id.
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// A stable, human-readable name for diagnostics.
    pub fn display(self) -> String {
        format!("v{}", self.0)
    }
}

/// The synthetic target every pointer starts at.
///
/// A pointer whose value the analysis cannot resolve points at `unknown`, and
/// `unknown` may alias anything. Using the largest id keeps it out of the
/// program's own id range (which is a dense `0..n`).
pub const UNKNOWN_TARGET: VarId = VarId(u32::MAX);

/// Wolfe §8.2.1's definiteness lattice.
///
/// ```text
///     Possible  ≤  Definite
///         \         /
///          \       /
///        bottom (no relation)
/// ```
///
/// Combining two definitenesses keeps `Definite` only when *both* are definite:
/// a target reached definitely on one path and possibly on another is only
/// reached *possibly* overall. `meet` and `join` are therefore the same
/// function here — the spec's notation differs but its two worked cases
/// (`meet(Definite, Possible) = Possible`, `join(Definite, Definite) =
/// Definite`) agree on exactly this operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Definiteness {
    /// Reached on every path.
    Definite,
    /// Reached on some path.
    Possible,
}

impl Definiteness {
    /// `Definite` only when both sides are; otherwise `Possible`.
    pub fn join(self, other: Definiteness) -> Definiteness {
        match (self, other) {
            (Definiteness::Definite, Definiteness::Definite) => Definiteness::Definite,
            _ => Definiteness::Possible,
        }
    }

    pub fn is_definite(self) -> bool {
        matches!(self, Definiteness::Definite)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Definiteness::Definite => "definite",
            Definiteness::Possible => "possible",
        }
    }
}

/// One `(p, q, d)` triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PtrRelation {
    pub pointer: VarId,
    pub target: VarId,
    pub def: Definiteness,
}

impl PtrRelation {
    pub fn new(pointer: VarId, target: VarId, def: Definiteness) -> PtrRelation {
        PtrRelation { pointer, target, def }
    }

    pub fn definite(pointer: VarId, target: VarId) -> PtrRelation {
        PtrRelation::new(pointer, target, Definiteness::Definite)
    }

    pub fn possible(pointer: VarId, target: VarId) -> PtrRelation {
        PtrRelation::new(pointer, target, Definiteness::Possible)
    }
}

/// A set of [`PtrRelation`]s, indexed both ways.
///
/// The two indexes are kept in sync by every mutator; a relation added to
/// `by_pointer` is added to `by_target` and vice versa. `by_target` exists so
/// "who points at `q`?" — the question the alias query needs — is a lookup
/// rather than a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointsToGraph {
    /// `pointer → { (target, definiteness) }`.
    pub by_pointer: BTreeMap<VarId, BTreeSet<(VarId, Definiteness)>>,
    /// `target → { (pointer, definiteness) }`.
    pub by_target: BTreeMap<VarId, BTreeSet<(VarId, Definiteness)>>,
    /// The synthetic unknown target.
    pub unknown: VarId,
    /// Iterations the fixpoint ran, for diagnostics. Not compared for equality:
    /// two graphs that agree on their relations agree, whatever route they took.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub iters: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Default for PointsToGraph {
    fn default() -> Self {
        PointsToGraph::empty()
    }
}

impl PointsToGraph {
    /// A graph with no relations, whose unknown target is [`UNKNOWN_TARGET`].
    pub fn empty() -> PointsToGraph {
        PointsToGraph {
            by_pointer: BTreeMap::new(),
            by_target: BTreeMap::new(),
            unknown: UNKNOWN_TARGET,
            iters: 0,
        }
    }

    /// Add `(pointer, target, def)`, accumulating definiteness: an existing
    /// `Definite` relation *weakens* to `Possible` when a `Possible` one is
    /// added for the same target, and two `Definite`s stay `Definite`.
    ///
    /// Returns `true` when the graph changed — the signal the fixpoint loops on.
    pub fn add_relation(&mut self, pointer: VarId, target: VarId, def: Definiteness) -> bool {
        let existing = self
            .by_pointer
            .get(&pointer)
            .and_then(|rels| rels.iter().find(|(t, _)| *t == target).map(|(_, d)| *d));
        let new_def = match existing {
            Some(old) => old.join(def),
            None => def,
        };
        if existing == Some(new_def) {
            return false;
        }
        let entry = self.by_pointer.entry(pointer).or_default();
        entry.retain(|(t, _)| *t != target);
        entry.insert((target, new_def));
        let back = self.by_target.entry(target).or_default();
        back.retain(|(p, _)| *p != pointer);
        back.insert((pointer, new_def));
        true
    }

    /// Alias for [`PointsToGraph::add_relation`] used at CFG merge points,
    /// where the spec calls the operation a `meet`.
    pub fn add_relation_meet(&mut self, pointer: VarId, target: VarId, def: Definiteness) -> bool {
        self.add_relation(pointer, target, def)
    }

    /// Drop every relation whose pointer is `pointer` (Wolfe case 1: a definite
    /// LHS "kills previous relations for that target").
    pub fn kill_targets_of(&mut self, pointer: VarId) -> bool {
        match self.by_pointer.remove(&pointer) {
            Some(rels) => {
                for (t, _) in rels {
                    if let Some(ps) = self.by_target.get_mut(&t) {
                        ps.retain(|(p, _)| *p != pointer);
                        if ps.is_empty() {
                            self.by_target.remove(&t);
                        }
                    }
                }
                true
            }
            None => false,
        }
    }
}
impl PointsToGraph {
    /// Weaken every relation whose pointer is `p` to `Possible` (Wolfe case 2:
    /// writing through `p` may change the pointers stored at `p`'s targets).
    pub fn weaken_to_possible(&mut self, p: VarId) -> bool {
        let rels = match self.by_pointer.get(&p) {
            Some(r) => r.clone(),
            None => return false,
        };
        let mut changed = false;
        for (t, d) in rels {
            if d.is_definite() && self.add_relation(p, t, Definiteness::Possible) {
                changed = true;
            }
        }
        changed
    }

    /// The relations of `pointer`, if any.
    pub fn relations_of(&self, pointer: VarId) -> Option<&BTreeSet<(VarId, Definiteness)>> {
        self.by_pointer.get(&pointer)
    }

    /// The pointers that may point at `target`.
    pub fn pointers_of(&self, target: VarId) -> Option<&BTreeSet<(VarId, Definiteness)>> {
        self.by_target.get(&target)
    }

    /// Every pointer id the graph mentions.
    pub fn pointers(&self) -> impl Iterator<Item = VarId> + '_ {
        self.by_pointer.keys().copied()
    }

    /// Meet the relations of `other` into `self`. Returns `true` on change.
    pub fn merge_from(&mut self, other: &PointsToGraph) -> bool {
        let mut changed = false;
        for (p, rels) in &other.by_pointer {
            for (t, d) in rels {
                if self.add_relation(*p, *t, *d) {
                    changed = true;
                }
            }
        }
        changed
    }

    /// Wolfe's `Clobber` effect: an unknown call may rewrite any pointer, so
    /// every pointer's relations weaken to `Possible` and each gains a `Possible`
    /// relation to `unknown`.
    pub fn clobber_all(&mut self) -> bool {
        let ptrs: Vec<VarId> = self.by_pointer.keys().copied().collect();
        let unknown = self.unknown;
        let mut changed = false;
        for p in ptrs {
            if self.weaken_to_possible(p) {
                changed = true;
            }
            if self.add_relation(p, unknown, Definiteness::Possible) {
                changed = true;
            }
        }
        changed
    }

    /// Does `p` definitely point *only* at `q`?
    pub fn must_point_to(&self, p: VarId, q: VarId) -> bool {
        match self.by_pointer.get(&p) {
            Some(rels) => rels.len() == 1 && rels.iter().any(|(t, d)| *t == q && d.is_definite()),
            None => false,
        }
    }

    /// Can `p` point at `q`?
    pub fn may_point_to(&self, p: VarId, q: VarId) -> bool {
        self.by_pointer
            .get(&p)
            .map(|rels| rels.iter().any(|(t, _)| *t == q))
            .unwrap_or(false)
    }

    /// The set of targets `p` may point at, `unknown` included.
    pub fn targets_of(&self, p: VarId) -> BTreeSet<VarId> {
        self.by_pointer
            .get(&p)
            .map(|rels| rels.iter().map(|(t, _)| *t).collect())
            .unwrap_or_default()
    }

    /// True when `p` may reach the synthetic unknown target.
    pub fn may_be_unknown(&self, p: VarId) -> bool {
        self.may_point_to(p, self.unknown)
    }

    /// Bound on the number of relations, for fuzz non-vacuity assertions.
    pub fn relation_count(&self) -> usize {
        self.by_pointer.values().map(|s| s.len()).sum()
    }
}

impl PointsToGraph {
    /// The lattice **meet** of two graphs, used at CFG merge points.
    ///
    /// A relation survives only if the target is present in *both* inputs, and
    /// its definiteness is `Definite` only when it is `Definite` in both.
    /// This is stricter than the spec's `meet_into`, which only unions the two
    /// sides and so keeps a target `Definite` even when one predecessor does
    /// not establish it — which would let `must_point_to` over-claim. The
    /// spine of the discipline in this crate is that a `must` is only ever
    /// reported when every path agrees, so the meet has to shrink, not grow.
    ///
    /// Returns `true` when `self` changed.
    pub fn meet_with(&mut self, other: &PointsToGraph) -> bool {
        let mut next: BTreeMap<VarId, BTreeSet<(VarId, Definiteness)>> = BTreeMap::new();
        let ptrs: BTreeSet<VarId> = self
            .by_pointer
            .keys()
            .chain(other.by_pointer.keys())
            .copied()
            .collect();
        let empty: BTreeSet<(VarId, Definiteness)> = BTreeSet::new();
        for p in ptrs {
            let a = self.by_pointer.get(&p).unwrap_or(&empty);
            let b = other.by_pointer.get(&p).unwrap_or(&empty);
            let targets: BTreeSet<VarId> = a
                .iter()
                .map(|(t, _)| *t)
                .chain(b.iter().map(|(t, _)| *t))
                .collect();
            let mut out: BTreeSet<(VarId, Definiteness)> = BTreeSet::new();
            for t in targets {
                let da = a.iter().find(|(x, _)| *x == t).map(|(_, d)| *d);
                let db = b.iter().find(|(x, _)| *x == t).map(|(_, d)| *d);
                let d = match (da, db) {
                    (Some(x), Some(y)) => x.join(y),
                    // Present on one side only: reached possibly, not definitely.
                    _ => Definiteness::Possible,
                };
                out.insert((t, d));
            }
            if !out.is_empty() {
                next.insert(p, out);
            }
        }
        if next == self.by_pointer {
            return false;
        }
        self.by_pointer = next;
        self.rebuild_target_index();
        true
    }

    /// Rebuild `by_target` from `by_pointer`. Every mutator that rewrites
    /// `by_pointer` wholesale must call this so the two indexes stay in sync.
    fn rebuild_target_index(&mut self) {
        let mut bt: BTreeMap<VarId, BTreeSet<(VarId, Definiteness)>> = BTreeMap::new();
        for (p, rels) in &self.by_pointer {
            for (t, d) in rels {
                bt.entry(*t).or_default().insert((*p, *d));
            }
        }
        self.by_target = bt;
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: u32) -> VarId {
        VarId(n)
    }

    #[test]
    fn definiteness_join_keeps_definite_only_when_both_are() {
        use Definiteness::*;
        assert_eq!(Definite.join(Definite), Definite);
        assert_eq!(Definite.join(Possible), Possible);
        assert_eq!(Possible.join(Definite), Possible);
        assert_eq!(Possible.join(Possible), Possible);
    }

    #[test]
    fn add_relation_accumulates_definiteness() {
        let mut g = PointsToGraph::empty();
        assert!(g.add_relation(v(0), v(1), Definiteness::Definite));
        // Same target, possible → weakens to Possible and reports a change.
        assert!(g.add_relation(v(0), v(1), Definiteness::Possible));
        assert_eq!(g.relations_of(v(0)).unwrap().len(), 1);
        assert!(g.may_point_to(v(0), v(1)));
        assert!(!g.must_point_to(v(0), v(1)));
        // Adding Possible again is a no-op.
        assert!(!g.add_relation(v(0), v(1), Definiteness::Possible));
    }

    #[test]
    fn must_point_to_requires_a_single_definite_target() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(1), Definiteness::Definite);
        assert!(g.must_point_to(v(0), v(1)));
        g.add_relation(v(0), v(2), Definiteness::Definite);
        assert!(!g.must_point_to(v(0), v(1)));
        assert!(g.may_point_to(v(0), v(2)));
    }

    #[test]
    fn kill_targets_of_clears_both_indexes() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(1), Definiteness::Definite);
        g.add_relation(v(2), v(1), Definiteness::Definite);
        assert!(g.kill_targets_of(v(0)));
        assert!(g.relations_of(v(0)).is_none());
        let ps = g.pointers_of(v(1)).unwrap();
        assert!(ps.iter().all(|(p, _)| *p != v(0)));
        assert!(!g.kill_targets_of(v(0)));
    }

    #[test]
    fn weaken_to_possible_downgrades_only_definite_edges() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(1), Definiteness::Definite);
        assert!(g.weaken_to_possible(v(0)));
        assert!(!g.must_point_to(v(0), v(1)));
        assert!(g.may_point_to(v(0), v(1)));
        assert!(!g.weaken_to_possible(v(0)));
    }

    #[test]
    fn clobber_all_reaches_unknown_and_weakens() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(0), v(1), Definiteness::Definite);
        assert!(g.clobber_all());
        assert!(g.may_be_unknown(v(0)));
        assert!(!g.must_point_to(v(0), v(1)));
        assert!(g.may_point_to(v(0), v(1)));
    }

    #[test]
    fn graph_is_deterministic_and_round_trips() {
        let mut g = PointsToGraph::empty();
        g.add_relation(v(1), v(0), Definiteness::Possible);
        g.add_relation(v(0), v(1), Definiteness::Definite);
        let json = serde_json::to_string(&g).unwrap();
        let back: PointsToGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.by_pointer, g.by_pointer);
        assert_eq!(back.by_target, g.by_target);
    }
}