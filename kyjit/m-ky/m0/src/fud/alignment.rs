//! Phase 5 §5.1.9 — alignment (Wolfe §9.3).
//!
//! Alignment shifts each statement of a loop by a constant number of iterations
//! so that a loop-carried dependence becomes loop-independent. After aligning
//! statement `s` by `a_s`, a dependence `src → dst` of distance `d` has new
//! distance `d + a_src - a_dst`; it is removed exactly when
//! `a_dst - a_src = d`. A whole loop can be aligned iff those equations are
//! mutually consistent, i.e. iff every cycle in the alignment graph has
//! accumulated distance zero (Wolfe §9.3).
//!
//! ```text
//! Alignment graph: one node per statement, two edges per dependence.
//!     Edge 1: dependence edge with distance d.
//!     Edge 2: inverse edge with distance -d.
//! If every cycle has accumulated distance zero, alignment can remove
//! all loop-carried dependences.
//! ```
//!
//! The consistency check is a weighted union-find over those equations: two
//! statements connected by a chain of dependences get a fixed relative shift,
//! and a dependence that closes a cycle with a nonzero total makes the system
//! inconsistent, so the loop cannot be aligned. The returned shifts are
//! normalised to have minimum zero, so the loop's outermost alignment is stable.

use serde::{Deserialize, Serialize};

use crate::ir::dependence::Dependence;
use crate::ir::Loop;

/// One statement's alignment: run it `shift` iterations later than written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alignment {
    /// The loop-body statement index.
    pub stmt: usize,
    /// How many iterations to shift the statement by.
    pub shift: i64,
}

/// A weighted union-find over the statements, encoding `shift[b] - shift[a] = w`
/// for each dependence `a → b` of distance `w`.
pub struct ShiftDsu {
    parent: Vec<usize>,
    rank: Vec<u32>,
    /// `offset[x] = shift[x] - shift[parent[x]]`.
    offset: Vec<i64>,
}

impl ShiftDsu {
    pub fn new(n: usize) -> ShiftDsu {
        ShiftDsu { parent: (0..n).collect(), rank: vec![0; n], offset: vec![0; n] }
    }

    fn find(&mut self, x: usize) -> (usize, i64) {
        if self.parent[x] == x {
            return (x, 0);
        }
        let p = self.parent[x];
        let (root, poff) = self.find(p);
        self.parent[x] = root;
        self.offset[x] += poff;
        (root, self.offset[x])
    }

    /// Assert `shift[b] - shift[a] = w`. Returns `false` if it contradicts an
    /// existing constraint (a cycle with nonzero total distance).
    pub fn union(&mut self, a: usize, b: usize, w: i64) -> bool {
        let (ra, oa) = self.find(a);
        let (rb, ob) = self.find(b);
        if ra == rb {
            return ob - oa == w;
        }
        if self.rank[ra] < self.rank[rb] {
            // parent[ra] = rb, offset[ra] = shift[ra] - shift[rb]
            //   = (shift[a] - oa) - (shift[b] - ob) = -w + (ob - oa).
            self.parent[ra] = rb;
            self.offset[ra] = ob - oa - w;
        } else {
            // parent[rb] = ra, offset[rb] = shift[rb] - shift[ra] = w - (ob - oa).
            self.parent[rb] = ra;
            self.offset[rb] = w - (ob - oa);
            if self.rank[ra] == self.rank[rb] {
                self.rank[ra] += 1;
            }
        }
        true
    }

    /// The shift of every node, relative to each component's root.
    pub fn shifts(&mut self, n: usize) -> Vec<i64> {
        let mut out = Vec::with_capacity(n);
        for x in 0..n {
            let (_r, o) = self.find(x);
            out.push(o);
        }
        out
    }
}

/// §5.1.9: the alignment offsets that remove every dependence in `dependences`,
/// or `None` when no alignment can (a cycle with nonzero total distance).
///
/// `loop_` supplies the number of statements to size the graph; a dependence
/// mentioning a statement index beyond `loop_.body.len()` widens the graph
/// rather than being silently dropped, so a caller cannot hide a dependence by
/// numbering it out of range.
pub fn compute_alignment(loop_: &Loop, dependences: &[Dependence]) -> Option<Vec<Alignment>> {
    let n = loop_.body.len().max(
        dependences
            .iter()
            .flat_map(|d| [d.src, d.dst])
            .max()
            .map(|m| m + 1)
            .unwrap_or(0),
    );
    if n == 0 {
        return Some(vec![]);
    }
    let mut dsu = ShiftDsu::new(n);
    for d in dependences {
        // a_dst - a_src = distance, with the *first* distance component (the
        // outermost loop level). Alignment across the inner levels is a deeper,
        // separate problem; the outermost component is what makes a single loop
        // nest alignable.
        let w = *d.distance.first().unwrap_or(&0);
        if !dsu.union(d.src, d.dst, w) {
            return None;
        }
    }
    let mut shifts = dsu.shifts(n);
    let min = *shifts.iter().min().unwrap_or(&0);
    for s in shifts.iter_mut() {
        *s -= min;
    }
    Some(
        shifts
            .into_iter()
            .enumerate()
            .map(|(stmt, shift)| Alignment { stmt, shift })
            .collect(),
    )
}

/// §5.1.9: does every cycle in the alignment graph have accumulated distance
/// zero, i.e. can this dependence set be aligned?
pub fn is_alignable(loop_: &Loop, dependences: &[Dependence]) -> bool {
    compute_alignment(loop_, dependences).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::dependence::DepKind;
    use crate::ir::{Access, AccessKind, IndexKind, IV, OffsetValue};

    fn loop_n(n: usize) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 8, 1)],
            body: (0..n)
                .map(|k| Access {
                    kind: AccessKind::Write,
                    base: "A".into(),
                    index: vec![1],
                    elem_size: 8,
                    offset: OffsetValue::Const(k as i64),
                    expr: None,
                    indirect: None,
                    guard: None,
                    range: None,
                    index_kind: IndexKind::Affine,
                })
                .collect(),
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    fn dep(src: usize, dst: usize, distance: i64) -> Dependence {
        Dependence::new(src, dst, DepKind::Flow, vec![distance])
    }

    #[test]
    fn a_single_dependence_aligns_to_zero() {
        let l = loop_n(2);
        let deps = vec![dep(0, 1, 1)];
        let a = compute_alignment(&l, &deps).unwrap();
        // shift[1] - shift[0] = 1; normalised min -> shift[0]=0, shift[1]=1.
        assert_eq!(a, vec![Alignment { stmt: 0, shift: 0 }, Alignment { stmt: 1, shift: 1 }]);
    }

    #[test]
    fn the_shifts_really_remove_every_dependence() {
        let l = loop_n(3);
        let deps = vec![dep(0, 1, 1), dep(1, 2, 2), dep(0, 2, 3)];
        let a = compute_alignment(&l, &deps).unwrap();
        let shift = |s: usize| a.iter().find(|x| x.stmt == s).map(|x| x.shift).unwrap_or(0);
        for d in &deps {
            let new = d.distance[0] + shift(d.src) - shift(d.dst);
            assert_eq!(new, 0, "dependence {d:?} not removed");
        }
    }

    #[test]
    fn an_inconsistent_cycle_is_refused() {
        let l = loop_n(2);
        let deps = vec![dep(0, 1, 1), dep(1, 0, 1)];
        assert!(compute_alignment(&l, &deps).is_none());
        assert!(!is_alignable(&l, &deps));
    }

    #[test]
    fn a_zero_total_cycle_is_alignable() {
        let l = loop_n(2);
        let deps = vec![dep(0, 1, 2), dep(1, 0, -2)];
        assert!(is_alignable(&l, &deps));
        let a = compute_alignment(&l, &deps).unwrap();
        assert_eq!(a.iter().find(|x| x.stmt == 1).unwrap().shift, 2);
    }

    #[test]
    fn no_dependences_align_trivially() {
        let l = loop_n(3);
        assert_eq!(
            compute_alignment(&l, &[]).unwrap(),
            vec![
                Alignment { stmt: 0, shift: 0 },
                Alignment { stmt: 1, shift: 0 },
                Alignment { stmt: 2, shift: 0 },
            ]
        );
    }
}