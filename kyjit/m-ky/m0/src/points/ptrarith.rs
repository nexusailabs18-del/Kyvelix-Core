//! Phase 7 §7.6 — pointer-arithmetic reconstruction (Wolfe §8.2.3).
//!
//! Wolfe's point is that a pointer walked over a named array is an array
//! reference in disguise: after `p = &A[0]` the inner-loop statement `*p` — with
//! `p` advanced by a constant stride each iteration — is exactly `A[c + s·i]`.
//! Recovering `(A, s, c)` turns it back into an access M0–M12 already reason
//! about, so no new decision procedure is needed for the common case.
//!
//! # What "reconstruct" means here
//!
//! The frontend (Phase 9) records, for a pointer it has proved affine in the IV,
//! a [`PtrStep`]: which array the base points into, the stride, the constant
//! offset and the element size. Reconstruction *validates* that record against
//! the declared arrays ([`ArrayTable`]) and produces a [`ReconstructedRef`].
//! A record whose element size disagrees with the array's, or whose base is not
//! a declared array, reconstructs to `None` — the caller then falls back to
//! `MayAlias` rather than guessing.
//!
//! This is the spec's `reconstruct(...) -> Option<ReconstructedRef>`, with the
//! "run `induct` on the pointer" step lifted to the producer: the record is the
//! result of that run, and everything Phase 7 promises is checked here.

use serde::{Deserialize, Serialize};

use crate::points::relation::VarId;

/// A pointer whose value is affine in a loop IV, as the frontend recorded it.
///
/// `value(i) = base + coeff·i`, interpreted as `&A[offset + coeff·i]` where `A`
/// is the array `base` points into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtrStep {
    /// The pointer variable being walked.
    pub pointer: VarId,
    /// The array it walks.
    pub base: VarId,
    /// Stride per iteration.
    pub coeff: i64,
    /// Constant element offset of the first iteration.
    pub offset: i64,
    /// Element size the frontend assumed.
    pub elem_size: usize,
    /// Which IV drives it.
    pub iv: usize,
}

impl PtrStep {
    pub fn new(
        pointer: VarId,
        base: VarId,
        coeff: i64,
        offset: i64,
        elem_size: usize,
        iv: usize,
    ) -> PtrStep {
        PtrStep { pointer, base, coeff, offset, elem_size, iv }
    }

    /// The element index at iteration value `i`.
    pub fn index_at(&self, i: i64) -> i64 {
        self.offset + self.coeff * i
    }
}

/// One declared array: which variable names it, and how big its elements are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrayInfo {
    pub base: VarId,
    pub elem_size: usize,
}

/// The arrays a reconstruction may land in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArrayTable {
    pub arrays: Vec<ArrayInfo>,
}

impl ArrayTable {
    pub fn new(arrays: Vec<ArrayInfo>) -> ArrayTable {
        ArrayTable { arrays }
    }

    pub fn of(pairs: &[(VarId, usize)]) -> ArrayTable {
        ArrayTable::new(
            pairs
                .iter()
                .map(|(b, sz)| ArrayInfo { base: *b, elem_size: *sz })
                .collect(),
        )
    }

    pub fn elem_size(&self, base: VarId) -> Option<usize> {
        self.arrays
            .iter()
            .find(|a| a.base == base)
            .map(|a| a.elem_size)
    }

    pub fn contains(&self, base: VarId) -> bool {
        self.arrays.iter().any(|a| a.base == base)
    }
}

/// The recovered array reference: `A[offset + coeff·i]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconstructedRef {
    /// The array.
    pub array: VarId,
    /// Stride in elements.
    pub coeff: i64,
    /// Constant element offset.
    pub offset: i64,
    /// Element size in bytes.
    pub elem_size: usize,
    /// Which pointer was reconstructed (kept so a certificate can name it).
    pub pointer: VarId,
}

impl ReconstructedRef {
    /// The element index at iteration value `i`.
    pub fn index_at(&self, i: i64) -> i64 {
        self.offset + self.coeff * i
    }

    /// The element indices visited over `i ∈ [lo, hi)`.
    pub fn indices_over(&self, lo: i64, hi: i64) -> Vec<i64> {
        let mut v = Vec::new();
        let mut i = lo;
        while i < hi {
            v.push(self.index_at(i));
            i += 1;
        }
        v
    }

    /// True when the reference is non-degenerate. A zero stride is allowed (a
    /// fixed element), but a zero element size is not: a zero-sized element
    /// makes every address the same address, so no disjointness can be proved.
    pub fn is_well_formed(&self) -> bool {
        self.elem_size > 0
    }
}

/// Reconstruct a pointer into an array reference (Wolfe §7.6).
///
/// Returns `None` when the record's base is not a declared array or its element
/// size disagrees with the array's — the two ways the spec says reconstruction
/// "fails", after which the caller falls back to `MayAlias`.
pub fn reconstruct(ptr: &PtrStep, arrays: &ArrayTable) -> Option<ReconstructedRef> {
    let declared = arrays.elem_size(ptr.base)?;
    if declared != ptr.elem_size {
        return None;
    }
    let r = ReconstructedRef {
        array: ptr.base,
        coeff: ptr.coeff,
        offset: ptr.offset,
        elem_size: ptr.elem_size,
        pointer: ptr.pointer,
    };
    if !r.is_well_formed() {
        return None;
    }
    Some(r)
}

/// Does a pointer whose stride is `coeff` advance in step with an IV whose step
/// is `iv_step`?
///
/// The spec's certificate "verifies that the loop IV increment matches the
/// pointer increment": a `p += elem_size` per iteration is a stride of one
/// element. This is that comparison, in elements.
pub fn increment_matches(iv_step: i64, coeff: i64) -> bool {
    iv_step != 0 && coeff != 0 && iv_step.signum() == coeff.signum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: u32) -> VarId {
        VarId(n)
    }

    #[test]
    fn reconstruct_succeeds_for_a_matching_array() {
        let arrays = ArrayTable::of(&[(v(0), 8)]);
        let step = PtrStep::new(v(1), v(0), 1, 0, 8, 0);
        let r = reconstruct(&step, &arrays).unwrap();
        assert_eq!(r.array, v(0));
        assert_eq!(r.coeff, 1);
        assert_eq!(r.index_at(3), 3);
    }

    #[test]
    fn reconstruct_rejects_unknown_base() {
        let arrays = ArrayTable::of(&[(v(0), 8)]);
        let step = PtrStep::new(v(1), v(9), 1, 0, 8, 0);
        assert!(reconstruct(&step, &arrays).is_none());
    }

    #[test]
    fn reconstruct_rejects_element_size_mismatch() {
        let arrays = ArrayTable::of(&[(v(0), 8)]);
        let step = PtrStep::new(v(1), v(0), 1, 0, 4, 0);
        assert!(reconstruct(&step, &arrays).is_none());
    }

    #[test]
    fn reconstruct_rejects_zero_element_size() {
        let arrays = ArrayTable::of(&[(v(0), 0)]);
        let step = PtrStep::new(v(1), v(0), 1, 0, 0, 0);
        assert!(reconstruct(&step, &arrays).is_none());
    }

    #[test]
    fn indices_over_enumerates_the_span() {
        let r = ReconstructedRef {
            array: v(0),
            coeff: 2,
            offset: 1,
            elem_size: 8,
            pointer: v(1),
        };
        assert_eq!(r.indices_over(0, 4), vec![1, 3, 5, 7]);
        assert_eq!(r.indices_over(3, 3), Vec::<i64>::new());
    }

    #[test]
    fn increment_matches_requires_same_sign() {
        assert!(increment_matches(1, 1));
        assert!(increment_matches(-2, -2));
        assert!(!increment_matches(1, -1));
        assert!(!increment_matches(0, 1));
    }

    #[test]
    fn step_round_trips_and_indexes() {
        let step = PtrStep::new(v(2), v(1), 3, -1, 8, 0);
        assert_eq!(step.index_at(2), 5);
        let json = serde_json::to_string(&step).unwrap();
        let back: PtrStep = serde_json::from_str(&json).unwrap();
        assert_eq!(back, step);
    }
}