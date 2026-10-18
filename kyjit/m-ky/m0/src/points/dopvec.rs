//! Phase 7 §7.5 — Fortran 90 dope vectors and array pointers (Wolfe §8.2.2).
//!
//! In Fortran 90 an *array pointer* (`real, pointer :: p(:)`) is not a machine
//! address: it is a descriptor — a **dope vector** — holding the base address,
//! the element size, and for each dimension a lower bound, an extent and a
//! stride. `p => A` associates `p`'s dope vector with `A`'s, and from then on
//! `p(i)` and `A(i)` are the same location.
//!
//! Modelling that descriptor is what lets Phase 7 answer the F90 forms of the
//! alias question:
//!
//! - [`DopeVector::offset_of`] turns an index tuple into an element offset, so
//!   `p(i,j)` lowers to a plain affine access the rest of the pipeline reads.
//! - [`associate`] classifies two descriptors as [`Association`]: identical,
//!   overlapping or disjoint. Disjointness is decided on the *element-offset
//!   interval* each descriptor sweeps, which is exact for the rectangular
//!   descriptors F90 constructs from whole arrays and array sections.
//!
//! A descriptor's origins are absolute *within its array*: `base_offset` is the
//! element offset of the descriptor's (lower,lower,…) corner from the array's
//! first element, so `p => A(3:10)` and `q => A(11:14)` are seen as the
//! disjoint intervals `[2,9]` and `[10,13]` rather than both starting at 0.
//!
//! The C form of the same fact is Phase 7's [`crate::points::allocdep`].

use serde::{Deserialize, Serialize};

use crate::points::relation::VarId;

/// One dimension of a dope vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dim {
    /// Declared lower bound of this dimension.
    pub lower: i64,
    /// Number of elements along this dimension.
    pub extent: i64,
    /// Distance between consecutive elements of this dimension, in elements.
    pub stride: i64,
}

impl Dim {
    pub fn new(lower: i64, extent: i64, stride: i64) -> Dim {
        Dim { lower, extent, stride }
    }
}

/// A Fortran 90 descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DopeVector {
    /// The array this descriptor names.
    pub base: VarId,
    /// Element size in bytes.
    pub elem_size: usize,
    /// Element offset of this descriptor's lower corner within `base`. A whole
    /// array is `0`; a section `A(3:)` of a 1-D array is `2`.
    pub base_offset: i64,
    /// One entry per dimension, outermost first.
    pub dims: Vec<Dim>,
}

impl DopeVector {
    pub fn new(base: VarId, elem_size: usize, dims: Vec<Dim>) -> DopeVector {
        DopeVector { base, elem_size, base_offset: 0, dims }
    }

    /// As [`DopeVector::new`], but with an explicit section origin.
    pub fn with_base_offset(
        base: VarId,
        elem_size: usize,
        base_offset: i64,
        dims: Vec<Dim>,
    ) -> DopeVector {
        DopeVector { base, elem_size, base_offset, dims }
    }
}

impl DopeVector {
    /// A row-major descriptor for `extents` with lower bounds 1.
    ///
    /// Strides are the product of the trailing extents, in elements —
    /// `[d_{k+1}·…·d_n]`.
    pub fn row_major(base: VarId, elem_size: usize, extents: &[i64]) -> DopeVector {
        let n = extents.len();
        let mut dims = Vec::with_capacity(n);
        for k in 0..n {
            let stride: i64 = extents[k + 1..].iter().product();
            dims.push(Dim::new(1, extents[k], stride.max(1)));
        }
        DopeVector::new(base, elem_size, dims)
    }

    /// A column-major descriptor for `extents` with lower bounds 1.
    pub fn col_major(base: VarId, elem_size: usize, extents: &[i64]) -> DopeVector {
        let mut dims = Vec::with_capacity(extents.len());
        for (k, e) in extents.iter().enumerate() {
            let stride: i64 = extents[..k].iter().product();
            dims.push(Dim::new(1, *e, stride.max(1)));
        }
        DopeVector::new(base, elem_size, dims)
    }

    /// The number of dimensions.
    pub fn rank(&self) -> usize {
        self.dims.len()
    }

    /// The element offset of `idx` from `base`'s first element, or `None` when
    /// the index is out of bounds or the arity is wrong.
    pub fn offset_of(&self, idx: &[i64]) -> Option<i64> {
        if idx.len() != self.dims.len() {
            return None;
        }
        let mut off = self.base_offset;
        for (d, i) in self.dims.iter().zip(idx) {
            if *i < d.lower || *i >= d.lower + d.extent {
                return None;
            }
            off = off.checked_add((*i - d.lower).checked_mul(d.stride)?)?;
        }
        Some(off)
    }

    /// The inclusive element-offset interval this descriptor sweeps.
    ///
    /// The constructors keep every stride non-negative, so the minimum offset is
    /// the descriptor's own origin and the maximum is the origin plus each
    /// dimension's span. A negative stride (a reversed section) is refused by
    /// returning `None`, and the caller then declines to prove disjointness.
    pub fn element_extent(&self) -> Option<(i64, i64)> {
        let mut span = 0i64;
        for d in &self.dims {
            if d.stride < 0 || d.extent < 0 {
                return None;
            }
            span = span.checked_add((d.extent - 1).checked_mul(d.stride)?)?;
        }
        Some((self.base_offset, self.base_offset + span))
    }

    /// Total number of elements, or `None` on overflow.
    pub fn element_count(&self) -> Option<i64> {
        self.dims.iter().try_fold(1i64, |acc, d| acc.checked_mul(d.extent))
    }
}

/// How two dope vectors relate (Wolfe §8.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Association {
    /// The same base and the same swept interval.
    Identical,
    /// The same base, overlapping intervals.
    Overlapping,
    /// Provably different locations.
    Disjoint,
}

/// Classify two descriptors.
///
/// *Different base variables name different arrays, so they are `Disjoint`* —
/// that is F90's "location independence" for distinct objects. The same base
/// with disjoint offset intervals is also `Disjoint`. Anything else is at worst
/// `Overlapping`: the classifier never claims disjointness it cannot see.
pub fn associate(a: &DopeVector, b: &DopeVector) -> Association {
    if a.base != b.base {
        return Association::Disjoint;
    }
    if a.elem_size != b.elem_size {
        return Association::Overlapping;
    }
    match (a.element_extent(), b.element_extent()) {
        (Some((alo, ahi)), Some((blo, bhi))) => {
            if ahi < blo || bhi < alo {
                Association::Disjoint
            } else if alo == blo && ahi == bhi {
                Association::Identical
            } else {
                Association::Overlapping
            }
        }
        _ => Association::Overlapping,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: u32) -> VarId {
        VarId(n)
    }

    #[test]
    fn row_major_strides_are_the_product_of_trailing_extents() {
        let dv = DopeVector::row_major(v(0), 8, &[2, 3, 4]);
        assert_eq!(dv.dims[0].stride, 12);
        assert_eq!(dv.dims[1].stride, 4);
        assert_eq!(dv.dims[2].stride, 1);
        assert_eq!(dv.element_count(), Some(24));
    }

    #[test]
    fn col_major_strides_are_the_product_of_leading_extents() {
        let dv = DopeVector::col_major(v(0), 8, &[2, 3, 4]);
        assert_eq!(dv.dims[0].stride, 1);
        assert_eq!(dv.dims[1].stride, 2);
        assert_eq!(dv.dims[2].stride, 6);
    }

    #[test]
    fn offset_of_uses_lower_bound_and_stride() {
        let dv = DopeVector::row_major(v(0), 8, &[2, 3]);
        assert_eq!(dv.offset_of(&[1, 1]), Some(0));
        assert_eq!(dv.offset_of(&[2, 3]), Some(5));
        assert_eq!(dv.offset_of(&[0, 1]), None); // below lower bound
        assert_eq!(dv.offset_of(&[1]), None); // wrong arity
    }

    #[test]
    fn distinct_bases_are_disjoint() {
        let a = DopeVector::row_major(v(0), 8, &[4]);
        let b = DopeVector::row_major(v(1), 8, &[4]);
        assert_eq!(associate(&a, &b), Association::Disjoint);
    }

    #[test]
    fn same_base_same_shape_is_identical() {
        let a = DopeVector::row_major(v(0), 8, &[4]);
        let b = DopeVector::row_major(v(0), 8, &[4]);
        assert_eq!(associate(&a, &b), Association::Identical);
    }

    #[test]
    fn disjoint_sections_of_one_array_are_disjoint() {
        // A(1:8) → [0,7]; A(9:12) → [8,11].
        let head = DopeVector::with_base_offset(v(0), 8, 0, vec![Dim::new(1, 8, 1)]);
        let tail = DopeVector::with_base_offset(v(0), 8, 8, vec![Dim::new(1, 4, 1)]);
        assert_eq!(associate(&head, &tail), Association::Disjoint);
    }

    #[test]
    fn overlapping_sections_are_overlapping() {
        let a = DopeVector::with_base_offset(v(0), 8, 0, vec![Dim::new(1, 8, 1)]);
        let b = DopeVector::with_base_offset(v(0), 8, 4, vec![Dim::new(1, 8, 1)]);
        assert_eq!(associate(&a, &b), Association::Overlapping);
    }

    #[test]
    fn negative_stride_declines_to_prove_extent() {
        let dv = DopeVector::new(v(0), 8, vec![Dim::new(1, 4, -1)]);
        assert!(dv.element_extent().is_none());
        let other = DopeVector::row_major(v(0), 8, &[4]);
        assert_eq!(associate(&dv, &other), Association::Overlapping);
    }
}