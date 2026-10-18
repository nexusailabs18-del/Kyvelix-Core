//! Phase 5 §5.1.1 — storage association (Wolfe §7.5).
//!
//! Maps a multi-dimensional element access to a flat linear address so that
//! Phase 7 (points-to) can reason about byte addresses instead of array
//! indices, and so a frontend can emit `A[i][j]` without computing the
//! flattening itself.
//!
//! ```text
//! row-major:    address = base + (i1·n2 + i2)·elem_size
//! column-major: address = base + (i2·n1 + i1)·elem_size   (Fortran)
//! ```
//!
//! ## Equivalence, without a frontend
//!
//! Fortran `EQUIVALENCE` and common blocks overlay distinct names on the same
//! storage. [`EquivalenceClass`] records a group of names and the byte offset
//! each contributes; [`linearize_with_equivalence`] resolves an access whose
//! base lives in a class to the class's shared address.
//!
//! ## Bounds checking is part of the contract
//!
//! A linearization that silently indexes past a dimension would report an
//! address two different accesses do not, in fact, share — or worse, claim two
//! that do not. `linearize` returns `None` unless every index is within
//! `0..dims[k]`, and `None` on overflow. The discipline matches the rest of the
//! IR: an out-of-range computation is *refused*, never guessed.

use serde::{Deserialize, Serialize};

use crate::ir::Access;

/// The storage order of a multi-dimensional array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// C, JavaScript: the last index varies fastest.
    RowMajor,
    /// Fortran: the first index varies fastest.
    ColumnMajor,
}

impl Layout {
    /// A short name for diagnostics and certificate text.
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::RowMajor => "row-major",
            Layout::ColumnMajor => "column-major",
        }
    }
}

/// The element index (not byte address) of `indices` under `layout`, using
/// `dims` for the strides. Returns `None` when `indices` and `dims` disagree in
/// arity, when any index is out of `0..dims[k]`, or on overflow.
pub fn linear_index(layout: Layout, indices: &[i64], dims: &[usize]) -> Option<i64> {
    if indices.len() != dims.len() || dims.is_empty() {
        return None;
    }
    for (k, &i) in indices.iter().enumerate() {
        if i < 0 || i as u128 >= dims[k] as u128 {
            return None;
        }
    }
    let mut acc: i64 = 0;
    match layout {
        Layout::RowMajor => {
            for (k, &i) in indices.iter().enumerate() {
                let mut stride: i64 = 1;
                for &n in &dims[k + 1..] {
                    stride = stride.checked_mul(n as i64)?;
                }
                acc = acc.checked_add(i.checked_mul(stride)?)?;
            }
        }
        Layout::ColumnMajor => {
            for (k, &i) in indices.iter().enumerate() {
                let mut stride: i64 = 1;
                for &n in &dims[..k] {
                    stride = stride.checked_mul(n as i64)?;
                }
                acc = acc.checked_add(i.checked_mul(stride)?)?;
            }
        }
    }
    Some(acc)
}

/// The byte address of `indices`: `base + linear_index · elem_size`.
///
/// Also checks that the total element count of `dims` fits in `i64`, so a
/// `dims` table that would overflow a stride is refused rather than wrapped.
pub fn linearize(layout: Layout, indices: &[i64], dims: &[usize], elem_size: i64) -> Option<i64> {
    linearize_from(0, layout, indices, dims, elem_size)
}

/// [`linearize`] with an explicit `base` byte offset (the array's start).
pub fn linearize_from(
    base: i64,
    layout: Layout,
    indices: &[i64],
    dims: &[usize],
    elem_size: i64,
) -> Option<i64> {
    let elem = linear_index(layout, indices, dims)?;
    let bytes = elem.checked_mul(elem_size)?;
    base.checked_add(bytes)
}

/// A group of names aliased onto one storage region, as Fortran
/// `EQUIVALENCE`/common blocks describe.
///
/// `offsets[k]` is the byte offset of `members[k]` from the region's start, so
/// `A` and `B` in `EQUIVALENCE (A, B(2))` have offsets `[0, 8]` when a B
/// element is 8 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquivalenceClass {
    pub members: Vec<String>,
    pub offsets: Vec<i64>,
}

impl EquivalenceClass {
    pub fn new(members: Vec<String>, offsets: Vec<i64>) -> EquivalenceClass {
        EquivalenceClass { members, offsets }
    }

    /// The byte offset of `name` within this class, if it is a member.
    pub fn offset_of(&self, name: &str) -> Option<i64> {
        self.members
            .iter()
            .position(|m| m == name)
            .and_then(|i| self.offsets.get(i).copied())
    }
}

/// Resolve an access whose base is a member of one of `classes` to a byte
/// address on the class's shared storage.
///
/// The access's own location is `index · iv + offset` (its element within the
/// class member), so an affine access has a *symbolic* address. This function
/// therefore handles the constant-element case: it linearizes the single
/// explicit element `access.offset` (times `elem_size`) against the member's
/// offset in the class. For the general affine case the caller should use
/// [`linearize_from`] directly with the class offset.
///
/// Returns `None` when the base is not in any class, or when the offset is not
/// a constant this function can read.
pub fn linearize_with_equivalence(access: &Access, classes: &[EquivalenceClass]) -> Option<i64> {
    let base = access.base.as_str();
    let class = classes.iter().find(|c| c.offset_of(base).is_some())?;
    let member_off = class.offset_of(base)?;
    let elem = access.offset.as_const()?;
    let bytes = elem.checked_mul(access.elem_size)?;
    member_off.checked_add(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AccessKind, IndexKind, OffsetValue};

    fn aff(kind: AccessKind, base: &str, offset: i64, elem: i64) -> Access {
        Access {
            kind,
            base: base.into(),
            index: vec![1],
            elem_size: elem,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    #[test]
    fn row_major_matches_hand_computation() {
        // dims = [2,3]; C order: (i,j) -> i*3 + j.
        assert_eq!(linear_index(Layout::RowMajor, &[0, 0], &[2, 3]), Some(0));
        assert_eq!(linear_index(Layout::RowMajor, &[1, 2], &[2, 3]), Some(5));
        assert_eq!(linear_index(Layout::RowMajor, &[1, 0], &[2, 3]), Some(3));
    }

    #[test]
    fn column_major_matches_fortran() {
        // dims = [2,3]; Fortran order: (i,j) -> j*2 + i.
        assert_eq!(linear_index(Layout::ColumnMajor, &[1, 2], &[2, 3]), Some(5));
        assert_eq!(linear_index(Layout::ColumnMajor, &[0, 1], &[2, 3]), Some(2));
        assert_eq!(linear_index(Layout::ColumnMajor, &[1, 0], &[2, 3]), Some(1));
    }

    #[test]
    fn both_layouts_cover_every_element_exactly_once() {
        // The two linearizations are permutations of 0..n, so their images are
        // equal as sets (the fuzz oracle in `tests/phase5_layout.rs` uses this).
        let dims = [3usize, 4, 2];
        let mut row = Vec::new();
        let mut col = Vec::new();
        for i in 0..3 {
            for j in 0..4 {
                for k in 0..2 {
                    row.push(linear_index(Layout::RowMajor, &[i, j, k], &dims).unwrap());
                    col.push(linear_index(Layout::ColumnMajor, &[i, j, k], &dims).unwrap());
                }
            }
        }
        row.sort();
        col.sort();
        assert_eq!(row, col);
        assert_eq!(row, (0..24).collect::<Vec<_>>());
    }

    #[test]
    fn out_of_range_is_refused() {
        assert_eq!(linear_index(Layout::RowMajor, &[2, 0], &[2, 3]), None);
        assert_eq!(linear_index(Layout::RowMajor, &[0, -1], &[2, 3]), None);
        assert_eq!(linear_index(Layout::RowMajor, &[0], &[2, 3]), None);
    }

    #[test]
    fn linearize_scales_by_element_size_from_base() {
        assert_eq!(linearize(Layout::RowMajor, &[1, 2], &[2, 3], 8), Some(5 * 8));
        assert_eq!(linearize_from(1000, Layout::RowMajor, &[0, 0], &[2, 3], 8), Some(1000));
        assert_eq!(linearize_from(1000, Layout::RowMajor, &[1, 2], &[2, 3], 8), Some(1040));
    }

    #[test]
    fn equivalence_resolves_aliases() {
        let classes = vec![EquivalenceClass::new(vec!["A".into(), "B".into()], vec![0, 8])];
        // A-element 3 with elem_size 8 -> byte 24.
        let a = aff(AccessKind::Read, "A", 3, 8);
        assert_eq!(linearize_with_equivalence(&a, &classes), Some(24));
        // B-element 0 -> member offset 8 + 0 = 8.
        let b = aff(AccessKind::Write, "B", 0, 8);
        assert_eq!(linearize_with_equivalence(&b, &classes), Some(8));
        // C is not a member.
        let c = aff(AccessKind::Read, "C", 0, 8);
        assert_eq!(linearize_with_equivalence(&c, &classes), None);
    }
}