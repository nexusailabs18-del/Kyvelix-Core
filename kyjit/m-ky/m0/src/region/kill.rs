//! Phase 6 §6.1.4 — the array kill analysis (Wolfe §8.1.4).
//!
//! Two questions, both about *array* elements rather than scalars:
//!
//! 1. [`is_self_conflict_free`] — does this assignment touch a different element
//!    on every iteration? (Wolfe's access-matrix rank test: the assignment
//!    writes `n` subscripts inside a nest of `d` loops; if the `n x d` data
//!    access matrix has rank `d`, distinct iteration vectors give distinct
//!    elements, so no iteration overwrites another's result.)
//! 2. [`kills`] — does this assignment definitely overwrite the element a
//!    previous write touched? If so, that previous write is dead: no later read
//!    can observe it, and `Kill` in [`super::dataflow`] says exactly that.
//!
//! ## Why the rank test is the interesting one
//!
//! Rank `d` means the map "iteration vector -> element" is injective, so a loop
//! that writes `A[i]` over `[0,n)` leaves *every* one of those elements freshly
//! written. That is what licenses deleting a prior `A[0..n)` initialization, and
//! it is why `Kill` must not be a plain union: a union would claim the kill even
//! when some iterations skip the write.
//!
//! ## Conservative refusals

//! Both functions return `false` when they cannot prove the fact. `false` is the
//! safe answer in both cases: not claiming self-conflict-freedom loses a
//! parallelization opportunity, and not claiming a kill keeps a write alive that
//! might be dead. The direction that would be unsafe — asserting either property
//! without proof — is what [`rank`] and the enumeration oracle in
//! [`tests::phase6_fuzz`] exist to make unreachable.

use crate::ir::{Access, IndexKind, Loop};

use super::bounds::{locations_of_access, region_of_access};

/// The cap on enumerated iterations for the exact self-conflict check. Beyond
/// it the analysis refuses rather than trusting an unbounded enumeration.
const ENUM_CAP: usize = 4096;

/// How completely an assignment covers the region it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fullness {
    /// Every iteration writes a distinct element: the access matrix has full
    /// row rank over the loops containing the assignment.
    EveryElementOnce,
    /// Some iterations write the same element as another iteration. A later
    /// write hides an earlier one, so a `Kill` claim over this region needs the
    /// last-writer argument instead of the rank argument.
    Overwrites,
    /// Not decidable here — non-affine index, a dead access, or a domain too
    /// large to enumerate.
    Unknown,
}

/// The data access matrix of one access inside a nest: one row per subscript
/// form, each row being that subscript's coefficients over the IVs, and a
/// trailing column of the subscript's constant term.
///
/// The flat IR carries a single subscript (`Access::index` plus `offset`), so
/// the matrix has one row. Callers that model multi-dimensional arrays pass
/// their own rows to [`rank`]; `access_matrix` is the shape the IR can produce
/// by itself. Wolfe's §8.1.4 example — `A[i+j][i-j]` — needs the two-row form,
/// which is why `rank` takes a matrix rather than a single vector.
pub fn access_matrix(access: &Access, loop_: &Loop) -> Vec<Vec<i64>> {
    if access.index.len() != loop_.depth {
        return vec![];
    }
    let offset = match access.offset.as_const() {
        Some(v) => v,
        None => return vec![], // symbolic offset - cannot form concrete matrix
    };
    let mut row: Vec<i64> = access.index.clone();
    row.push(offset);
    vec![row]
}

/// The rank of an integer matrix over the rationals, by fraction-free Gaussian
/// elimination (Bareiss) in `i128` so no rounding can inflate or deflate it.
///
/// Exact arithmetic matters here: a floating-point rank on
/// `[[1,1],[1,1.0000000001]]` would report 2 for a matrix that is rank 1 over
/// the integers, and rank 2 is precisely the claim that licenses a `Kill`.
///
/// Empty matrices have rank 0, which is the right identity for "no subscripts,
/// no constraints".
pub fn rank(m: &[Vec<i64>]) -> usize {
    let rows = m.len();
    if rows == 0 {
        return 0;
    }
    let cols = m.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return 0;
    }
    let mut a: Vec<Vec<i128>> = m
        .iter()
        .map(|r| {
            let mut v: Vec<i128> = r.iter().map(|&x| x as i128).collect();
            v.resize(cols, 0);
            v
        })
        .collect();

    let mut r = 0usize;
    let mut prev_pivot: i128 = 1;
    for c in 0..cols {
        if r >= rows {
            break;
        }
        // Find a pivot at or below row r in column c.
        let piv = (r..rows).find(|&i| a[i][c] != 0);
        let Some(p) = piv else { continue };
        a.swap(r, p);
        // Bareiss: a[i][j] = (a[r][c]*a[i][j] - a[i][c]*a[r][j]) / prev_pivot.
        // Sylvester's identity makes that division exact, so no fraction ever
        // appears and the rank cannot drift with rounding.
        for i in r + 1..rows {
            for j in c + 1..cols {
                let num = a[r][c] * a[i][j] - a[i][c] * a[r][j];
                a[i][j] = num / prev_pivot;
            }
            a[i][c] = 0;
        }
        prev_pivot = a[r][c];
        r += 1;
    }
    r
}

/// Wolfe's §8.1.4 rank test for one access: is the assignment injective over
/// the iterations of `loop_`?
///
/// `d` = the number of loops containing the assignment (`loop_.depth`), `n` =
/// the number of subscripts (one, in this IR). Rank `d` implies every iteration
/// writes a different element. Because the flat model has one subscript, rank is
/// at most 1, so the test is conclusive only for depth-1 loops — a depth-2 loop
/// writing `A[i]` has rank 1 < 2 and is correctly reported as *not*
/// self-conflict-free (`j` changes the loop, not the element).
///
/// For the shapes the rank test cannot settle, this falls back to exact
/// enumeration of the access's locations when the iteration space fits in
/// [`ENUM_CAP`]: distinct count == iteration count is the definition, so the
/// fallback is not a weaker test but a direct one. `Fullness::Unknown` reports
/// the cases neither covers.
pub fn fullness(access: &Access, loop_: &Loop) -> Fullness {
    if access.kind != crate::ir::AccessKind::Write {
        return Fullness::Unknown;
    }
    if access.range.is_some() || !matches!(access.index_kind, IndexKind::Affine) {
        return Fullness::Unknown;
    }
    let d = loop_.depth;
    if loop_.ivs.iter().any(|iv| iv.step <= 0 || iv.lower >= iv.upper) {
        return Fullness::Unknown;
    }
    let m = access_matrix(access, loop_);
    let r = rank(&m);
    if r == d {
        return Fullness::EveryElementOnce;
    }
    if r < d && d > 1 {
        // Rank too low: distinct iteration vectors may still collide. The
        // single-subscript IR always has r <= 1, so this is the common case.
        return match enumerate_count(access, loop_) {
            Some((n_distinct, n_iters)) if n_distinct == n_iters => Fullness::EveryElementOnce,
            Some((_, _)) => Fullness::Overwrites,
            None => Fullness::Unknown,
        };
    }
    // r == d == 1 handled above; r > d cannot happen for a single subscript.
    match enumerate_count(access, loop_) {
        Some((n_distinct, n_iters)) if n_distinct == n_iters => Fullness::EveryElementOnce,
        Some((_, _)) => Fullness::Overwrites,
        None => Fullness::Unknown,
    }
}

/// `true` when every iteration of `loop_` assigns a different element of
/// `access`'s array. The spec's `-> bool` API; `Fullness::Unknown` maps to
/// `false` because "cannot prove injective" must not license a kill.
pub fn is_self_conflict_free(access: &Access, loop_: &Loop) -> bool {
    fullness(access, loop_) == Fullness::EveryElementOnce
}

/// `(distinct location count, iteration count)`, or `None` when the space is too
/// large or the access dead.
fn enumerate_count(access: &Access, loop_: &Loop) -> Option<(usize, usize)> {
    let locs = locations_of_access(access, loop_, ENUM_CAP)?;
    let mut iters: usize = 1;
    for iv in &loop_.ivs {
        if iv.step <= 0 || iv.lower >= iv.upper {
            return None;
        }
        let n = ((iv.upper - 1 - iv.lower) / iv.step + 1) as usize;
        iters = iters.checked_mul(n)?;
    }
    let mut sorted = locs.clone();
    sorted.sort();
    sorted.dedup();
    Some((sorted.len(), iters))
}

/// Does the write `def` kill every value the earlier write `previous` left?
///
/// Defined as: `previous` is dead exactly when *every* element `previous` can
/// write is an element `def` certainly writes, and `def` certainly executes.
/// Under that condition no later read can observe `previous`'s value at any
/// element it touched.
///
/// Refusals (`false`) are taken whenever the fact is not provable: a different
/// array (`base` mismatch), a parametric offset, a dead `def`, an uncomputable
/// region, or a `def` whose location set does not cover `previous`'s.
///
/// ## What this does *not* check
///
/// Program order. "Kills" also requires `def` to run after `previous` on every
/// path — that is the CFG's job, and [`super::dataflow`] only calls this within
/// a block, where statement order is the order given, or across a back edge
/// where the iteration argument replaces it. Asking `kills` to decide ordering
/// would mix two different soundness obligations into one boolean.
pub fn kills(def: &Access, previous: &Access, loop_: &Loop) -> bool {
    if def.kind != crate::ir::AccessKind::Write || previous.base != def.base {
        return false;
    }
    // A `RangeExpr` access's `index`/`offset` are stale placeholders, so its
    // locations cannot be enumerated; compare the two boxes instead, which is
    // weaker but still one-directional (a subset claim between sound boxes is
    // itself sound).
    if previous.range.is_some() || def.range.is_some() {
        return match (region_of_access(previous, loop_), region_of_access(def, loop_)) {
            (Some(p), Some(d)) => p.is_subset_of(&d),
            _ => false,
        };
    }
    let Some(prev_locs) = locations_of_access(previous, loop_, ENUM_CAP) else {
        return false;
    };
    let Some(def_locs) = locations_of_access(def, loop_, ENUM_CAP) else {
        return false;
    };
    let mut set = std::collections::HashSet::new();
    for &l in &def_locs {
        set.insert(l);
    }
    prev_locs.iter().all(|l| set.contains(l))
}

/// The region a killing write overwrites, for the `Kill` transfer function.
/// `None` when the write is not certain to cover anything (a dead access or a
/// refusal), which the dataflow treats as "this block kills nothing".
pub fn killed_region(def: &Access, loop_: &Loop) -> Option<super::bounds::Region> {
    if def.kind != crate::ir::AccessKind::Write {
        return None;
    }
    region_of_access(def, loop_)
}

/// The subscripts an access has — one in this IR, kept as a function so callers
/// state the rank test in Wolfe's `n x d` terms.
pub fn subscript_count(_access: &Access) -> usize {
    1
}

/// `true` when `access` writes a location that is invariant across the loop
/// (the classic "loop-invariant array element" case: `A[0] = ...` inside a
/// loop). Such a write is *always* self-conflicting, and every iteration's
/// value except the last is dead.
pub fn is_invariant_write(access: &Access, _loop_: &Loop) -> bool {
    if access.kind != crate::ir::AccessKind::Write || access.range.is_some() {
        return false;
    }
    if !matches!(access.index_kind, IndexKind::Affine) {
        return false;
    }
    access.index.iter().all(|&c| c == 0)
}
