//! Phase 5 §5.1.10 — cross-loop distance (Wolfe §9.2, fusion).
//!
//! When two adjacent loops are candidates for fusion, the question is whether
//! fusing them preserves the dependence distances of both. The cross-loop
//! distance is the per-component gap between the two loops' iteration spaces.
//!
//! ```text
//! Cross-loop distance = difference between iteration vectors of the two loops.
//! If d1 = i_T - i_S for loop 1 and d2 = j_T - j_S for loop 2,
//! the cross-loop distance is d = (j_T - i_S) - (j_S - i_T).
//! ```
//!
//! Two entry points are provided, because the spec's single signature has no
//! statement pair to work with:
//!
//! - [`compute_cross_loop_distance`] takes the spec's `(&Loop, &Loop)` and
//!   reports the offset between the two loops' *entry iterations* — the
//!   quantity fusion needs when the loops start at different points.
//! - [`cross_loop_vectors`] implements the spec's literal arithmetic for a
//!   caller that holds the four iteration vectors `i_S`, `i_T`, `j_S`, `j_T`.
//!
//! Both are total on their inputs and return `None` rather than guessing when
//! the two loops disagree in arity.

use serde::{Deserialize, Serialize};

use crate::ir::Loop;

/// The cross-loop distance between two fused loops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossLoopDistance {
    /// Index of the first (earlier) loop.
    pub first_loop: usize,
    /// Index of the second (later) loop.
    pub second_loop: usize,
    /// The per-level distance, outermost first.
    pub distance: Vec<i64>,
}

/// §5.1.10's vector arithmetic: `(j_T - i_S) - (j_S - i_T)`, componentwise.
///
/// Returns `None` when the four vectors disagree in arity or the arithmetic
/// overflows.
pub fn cross_loop_vectors(
    i_s: &[i64],
    i_t: &[i64],
    j_s: &[i64],
    j_t: &[i64],
) -> Option<Vec<i64>> {
    if i_s.len() != i_t.len() || i_s.len() != j_s.len() || i_s.len() != j_t.len() {
        return None;
    }
    let mut out = Vec::with_capacity(i_s.len());
    for k in 0..i_s.len() {
        let a = j_t[k].checked_sub(i_s[k])?;
        let b = j_s[k].checked_sub(i_t[k])?;
        out.push(a.checked_sub(b)?);
    }
    Some(out)
}

/// §5.1.10: the cross-loop distance between two adjacent loops, as the offset
/// between their entry iterations (`second.lower - first.lower` per level).
///
/// `first_loop`/`second_loop` are the fusion candidate indices the caller
/// assigns; they label the result. Returns `None` when the loops disagree in
/// arity or either has no IVs (a loop with no IV has no iteration space to
/// fuse).
pub fn compute_cross_loop_distance(
    first_loop: &Loop,
    second_loop: &Loop,
) -> Option<CrossLoopDistance> {
    if first_loop.ivs.is_empty() || second_loop.ivs.is_empty() {
        return None;
    }
    if first_loop.ivs.len() != second_loop.ivs.len() {
        return None;
    }
    let mut distance = Vec::with_capacity(first_loop.ivs.len());
    for (a, b) in first_loop.ivs.iter().zip(second_loop.ivs.iter()) {
        distance.push(b.lower.checked_sub(a.lower)?);
    }
    Some(CrossLoopDistance { first_loop: 0, second_loop: 1, distance })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IndexKind, IV};

    fn loop_at(lower: i64, upper: i64) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", lower, upper, 1)],
            body: vec![],
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    #[test]
    fn offset_is_entry_difference() {
        let d = compute_cross_loop_distance(&loop_at(0, 8), &loop_at(3, 11)).unwrap();
        assert_eq!(d.distance, vec![3]);
    }

    #[test]
    fn arity_mismatch_is_refused() {
        let l2 = Loop {
            depth: 2,
            ivs: vec![
                IV::new("i", 0, 8, 1),
                IV::new("j", 0, 8, 1),
            ],
            body: vec![],
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        };
        assert_eq!(compute_cross_loop_distance(&loop_at(0, 4), &l2), None);
        let _ = IndexKind::Affine;
    }

    #[test]
    fn vector_arithmetic_matches_the_spec_formula() {
        // d = (j_T - i_S) - (j_S - i_T)
        // i_S = 0, i_T = 3 (loop 1 distance 3)
        // j_S = 1, j_T = 5 (loop 2 distance 4)
        // d = (5 - 0) - (1 - 3) = 5 - (-2) = 7
        assert_eq!(cross_loop_vectors(&[0], &[3], &[1], &[5]), Some(vec![7]));
        // i_S=0,i_T=2,j_S=0,j_T=2 -> (2-0)-(0-2) = 2-(-2) = 4.
        assert_eq!(cross_loop_vectors(&[0], &[2], &[0], &[2]), Some(vec![4]));
        // Arity mismatch.
        assert_eq!(cross_loop_vectors(&[0], &[3, 4], &[1], &[5]), None);
    }
}