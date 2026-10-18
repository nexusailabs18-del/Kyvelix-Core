//! Phase 6 §6.1.2 — simple bounds (Wolfe §8.1.2).
//!
//! A [`Region`] is an axis-aligned box of *element indices*, given as inclusive
//! `(lower, upper)` pairs, one per dimension. The array IR records a flat
//! element location (`index · iv + offset`), so a region derived from an access
//! is one-dimensional; multi-dimensional regions appear when a caller states
//! them (see [`crate::region::kill`] and the §5.1.1 linearizer).
//!
//! ## The two operations and why they are exact
//!
//! ```text
//! union        [l_a : u_a] ∪ [l_b : u_b] = [min l : max u]   (per dim)
//! intersect    [l_a : u_a] ∩ [l_b : u_b] = [max l : min u]   (per dim)
//! ```
//!
//! The union is the *smallest box* containing both operands, so it is the least
//! uninformative over-approximation available in this lattice: a `MayUse` set
//! built with it never claims an element a path did not read, but it does merge
//! `A[0:3] ∪ A[8:11]` into `A[0:11]`. That loss is the point of §6.1.3:
//! [`crate::region::triplet::TripletRegion`] keeps the stride and settles such
//! unions exactly.
//!
//! An empty region is representable: a dimension with `lower > upper`. The
//! intersection of two disjoint boxes produces one and [`Region::is_empty`]
//! reports it, so `Kill` intersections — empty whenever two paths overwrite
//! different ranges — are expressible rather than an error.
//!
//! ## Arity
//!
//! `union` and `intersect` return `None` when the operands disagree on the
//! number of dimensions. A 1-D flattened range and a 2-D `[rows, cols]` box
//! describe different address spaces; quietly bounding-boxing them together
//! would let a summary claim a region no access ever produced. (The §6.1.2
//! sketch takes `Region` for both operands and returns `Region`; the `Option`
//! is the deviation that keeps arity an invariant instead of a hope.)

use serde::{Deserialize, Serialize};

use crate::ir::{Access, AccessKind, CmpOp, IndexKind, IV, Loop, RangeExpr};

/// Number of dimensions a region can track. Matches the IR's depth cap
/// (`Loop::validate` rejects `depth > 4`), so a region can always name the
/// array a loop indexes.
pub const MAX_DIMS: usize = 4;

/// The enumeration cap for non-affine index kinds in a summary pass. Matches
/// the per-access cap the deciding levels use (`m6::ENUM_CAP`); above it the
/// answer is "no region", never a guessed one.
const ENUM_CAP: usize = 4096;

/// An axis-aligned box of element indices, inclusive on both ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Region {
    /// One inclusive `(lower, upper)` pair per dimension.
    pub dims: Vec<(i64, i64)>,
}

impl Region {
    /// A box over `dims`. Checks nothing; use [`Region::validate`] when the
    /// dims arrived from a certificate.
    pub fn new(dims: Vec<(i64, i64)>) -> Region {
        Region { dims }
    }

    /// The one-dimensional box `[lo, hi]`.
    pub fn span(lo: i64, hi: i64) -> Region {
        Region { dims: vec![(lo, hi)] }
    }

    /// The single point `point`.
    pub fn point(point: &[i64]) -> Region {
        Region { dims: point.iter().map(|&p| (p, p)).collect() }
    }

    /// The region covering every index: the absorbing element of `union` and
    /// the starting point of a hull fold.
    pub fn universe() -> Region {
        Region { dims: vec![(i64::MIN, i64::MAX)] }
    }

    /// The empty region: one dimension with `lower > upper`.
    pub fn empty() -> Region {
        Region { dims: vec![(1, 0)] }
    }

    /// `true` when some dimension is inverted, i.e. the box holds no element.
    pub fn is_empty(&self) -> bool {
        self.dims.iter().any(|&(lo, hi)| lo > hi)
    }

    /// The number of dimensions.
    pub fn arity(&self) -> usize {
        self.dims.len()
    }

    /// `true` when `point` lies inside the box. A point with the wrong arity is
    /// never contained — a 1-D location does not answer a 2-D question.
    pub fn contains(&self, point: &[i64]) -> bool {
        self.arity() == point.len()
            && self
                .dims
                .iter()
                .zip(point.iter())
                .all(|(&(lo, hi), &p)| lo <= p && p <= hi)
    }

    /// Per-dimension union: `[min l : max u]`. `None` on arity mismatch.
    pub fn union(&self, other: &Region) -> Option<Region> {
        if self.arity() != other.arity() {
            return None;
        }
        Some(Region {
            dims: self
                .dims
                .iter()
                .zip(other.dims.iter())
                .map(|(&(a_lo, a_hi), &(b_lo, b_hi))| (a_lo.min(b_lo), a_hi.max(b_hi)))
                .collect(),
        })
    }

    /// Per-dimension intersection: `[max l : min u]`. `None` on arity mismatch.
    /// Disjoint operands yield an *empty* region, not `None` — that is the
    /// bottom of the lattice and `Kill` has to be able to say it.
    pub fn intersect(&self, other: &Region) -> Option<Region> {
        if self.arity() != other.arity() {
            return None;
        }
        Some(Region {
            dims: self
                .dims
                .iter()
                .zip(other.dims.iter())
                .map(|(&(a_lo, a_hi), &(b_lo, b_hi))| (a_lo.max(b_lo), a_hi.min(b_hi)))
                .collect(),
        })
    }

    /// The smallest box containing every region in `rs`. `None` if `rs` is empty
    /// or the arities disagree.
    pub fn hull(rs: &[Region]) -> Option<Region> {
        let first = rs.first()?;
        let arity = first.arity();
        let mut acc = first.clone();
        for r in &rs[1..] {
            if r.arity() != arity {
                return None;
            }
            acc.dims
                .iter_mut()
                .zip(r.dims.iter())
                .for_each(|((alo, ahi), (blo, bhi))| {
                    *alo = (*alo).min(*blo);
                    *ahi = (*ahi).max(*bhi);
                });
        }
        Some(acc)
    }

    /// `true` when every element of `self` is an element of `other`. Emptiness
    /// follows the lattice: an empty `self` is contained in anything.
    pub fn is_subset_of(&self, other: &Region) -> bool {
        if self.is_empty() {
            return true;
        }
        self.arity() == other.arity()
            && self
                .dims
                .iter()
                .zip(other.dims.iter())
                .all(|(&(a_lo, a_hi), &(b_lo, b_hi))| b_lo <= a_lo && a_hi <= b_hi)
    }

    /// The number of elements in the box, or `None` when the box is unbounded
    /// or the product overflows. The enumerating oracles use it to decide
    /// whether exhaustive comparison is possible.
    pub fn count(&self) -> Option<i64> {
        let mut acc: i64 = 1;
        for &(lo, hi) in &self.dims {
            if hi < lo {
                return Some(0);
            }
            if lo == i64::MIN || hi == i64::MAX {
                return None;
            }
            let n = hi.checked_sub(lo)?.checked_add(1)?;
            acc = acc.checked_mul(n)?;
        }
        Some(acc)
    }

    /// Every point of the box in lexicographic order, or `None` when the box is
    /// unbounded or holds more than `cap` points. An oracle that cannot bound
    /// its own work is a hang rather than a test.
    pub fn enumerate(&self, cap: usize) -> Option<Vec<Vec<i64>>> {
        let n = self.count()?;
        if n > cap as i64 {
            return None;
        }
        if self.arity() == 0 {
            return Some(vec![vec![]]);
        }
        let mut out: Vec<Vec<i64>> = Vec::with_capacity(n as usize);
        let mut cur = vec![0i64; self.arity()];
        enumerate_rec(self, 0, &mut cur, &mut out);
        Some(out)
    }

    /// Rejects malformed boxes: no dimensions, more than [`MAX_DIMS`], or a
    /// dimension that reverses the unbounded sentinels.
    pub fn validate(&self) -> Result<(), String> {
        if self.dims.is_empty() {
            return Err("region has no dimensions".into());
        }
        if self.dims.len() > MAX_DIMS {
            return Err(format!("region has {} dimensions, max {}", self.dims.len(), MAX_DIMS));
        }
        for &(lo, hi) in &self.dims {
            if lo == i64::MAX && hi == i64::MIN {
                return Err("dimension reverses the unbounded sentinels".into());
            }
        }
        Ok(())
    }

    /// Compact diagnostic form: `[0:7]` or `[*:7]` for an unbounded side.
    pub fn to_string_repr(&self) -> String {
        let mut s = String::new();
        for (k, &(lo, hi)) in self.dims.iter().enumerate() {
            if k > 0 {
                s.push(',');
            }
            let lo_s = if lo == i64::MIN { "*".into() } else { lo.to_string() };
            let hi_s = if hi == i64::MAX { "*".into() } else { hi.to_string() };
            s.push_str(&format!("[{}:{}]", lo_s, hi_s));
        }
        s
    }
}

fn enumerate_rec(r: &Region, d: usize, cur: &mut Vec<i64>, out: &mut Vec<Vec<i64>>) {
    if d == r.arity() {
        out.push(cur.clone());
        return;
    }
    let (lo, hi) = r.dims[d];
    if hi < lo {
        return;
    }
    for v in lo..=hi {
        cur[d] = v;
        enumerate_rec(r, d + 1, cur, out);
    }
}

/// A set of regions — the §6.1.1 payload of `MayUse` / `MayMod` / `Kill`.
///
/// Kept normalized (overlapping equal-arity regions merged, then sorted) by
/// every mutation so two analyzers that discover the same regions in different
/// orders produce byte-identical certificates. Without that, the §6.2
/// `FunctionSummary` arm's comparison of a rebuilt summary against the claimed
/// one would be a comparison of insertion order rather than of meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RegionSet {
    pub regions: Vec<Region>,
}

impl RegionSet {
    pub fn new() -> RegionSet {
        RegionSet::default()
    }

    /// Add one region, widening any same-arity box it overlaps. Empty regions
    /// are dropped: they carry no information and would make `Kill` sets that
    /// look non-empty but contain nothing.
    pub fn insert(&mut self, r: Region) {
        if r.is_empty() {
            return;
        }
        for existing in self.regions.iter_mut() {
            if existing.arity() == r.arity() {
                if let Some(u) = existing.union(&r) {
                    *existing = u;
                    return self.normalize();
                }
            }
        }
        self.regions.push(r);
        self.regions.sort_by_key(|r| (r.arity(), r.dims.clone()));
    }

    /// Merge every pair of equal-arity overlapping regions until stable, then
    /// sort. Idempotent; each pass strictly reduces the number of regions, so
    /// it terminates.
    pub fn normalize(&mut self) {
        loop {
            let mut merged = None;
            'outer: for i in 0..self.regions.len() {
                for j in i + 1..self.regions.len() {
                    if self.regions[i].arity() != self.regions[j].arity() {
                        continue;
                    }
                    if let Some(u) = self.regions[i].union(&self.regions[j]) {
                        if u != self.regions[i] && u != self.regions[j] {
                            merged = Some((i, j, u));
                            break 'outer;
                        }
                    }
                }
            }
            match merged {
                Some((i, j, u)) => {
                    self.regions[i] = u;
                    self.regions.remove(j);
                }
                None => break,
            }
        }
        self.regions.sort_by_key(|r| (r.arity(), r.dims.clone()));
    }

    /// The union of two sets.
    pub fn join(&self, other: &RegionSet) -> RegionSet {
        let mut out = self.clone();
        for r in &other.regions {
            out.insert(r.clone());
        }
        out
    }

    /// Every region of `self` is contained in some region of `other` of the
    /// same arity. This is the direction the checker needs: the claimed summary
    /// may be *wider* than the rebuilt one, never narrower.
    pub fn is_subset_of(&self, other: &RegionSet) -> bool {
        self.regions.iter().all(|r| {
            other
                .regions
                .iter()
                .any(|o| r.arity() == o.arity() && r.is_subset_of(o))
        })
    }

    /// `true` when no region of this set contains `point`.
    pub fn misses(&self, point: &[i64]) -> bool {
        !self.regions.iter().any(|r| r.contains(point))
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
}

/// Every element index an access touches over `loop_`'s iteration space, as a
/// one-dimensional [`Region`] (the IR's flat-location model).
///
/// `None` means "this access's region is not computable here", and every caller
/// must treat that as **no information** — never as "empty". Reporting an empty
/// region for a failure would put a false `Kill` into a summary and let Phase 7
/// delete a live read. The shapes refused:
///
/// - a parametric `offset` with no environment to resolve it against;
/// - `IndexKind::Affine` whose `index` arity disagrees with `loop_.depth`;
/// - non-affine index kinds whose enumeration exceeds [`ENUM_CAP`] points;
/// - an access whose guard makes it dead on every iteration.
///
/// Guards narrow the domain exactly as M4 does: the access runs only on the
/// iterations its `guard` permits, so `A[i]` under `i < 4` over `[0,8)` has
/// region `[0:3]` rather than `[0:7]`.
pub fn region_of_access(access: &Access, loop_: &Loop) -> Option<Region> {
    // A bounded-range access lives in a different location space from
    // `index · iv + offset`; the IR says so explicitly ("index/offset are stale
    // placeholders"). Its region is the range, not the affine form.
    if let Some(rg) = &access.range {
        return region_of_range(rg, loop_);
    }
    let offset = access.offset.as_const()?;
    let ivs = realized_ivs(access, loop_)?;
    match &access.index_kind {
        IndexKind::Affine => {
            if access.index.len() != loop_.depth {
                return None;
            }
            let mut lo: i128 = offset as i128;
            let mut hi: i128 = offset as i128;
            for (k, &c) in access.index.iter().enumerate() {
                let (dlo, dhi) = (ivs[k].lower, ivs[k].upper);
                // A separable linear form attains its extremes at the endpoint
                // its own coefficient's sign selects. Exact for affine forms:
                // `index` carries no mixed term, so no corner argument is needed.
                let (plo, phi) = if c >= 0 { (dlo, dhi) } else { (dhi, dlo) };
                lo += (c as i128) * plo as i128;
                hi += (c as i128) * phi as i128;
            }
            if hi < lo {
                std::mem::swap(&mut lo, &mut hi);
            }
            Some(Region::span(i64::try_from(lo).ok()?, i64::try_from(hi).ok()?))
        }
        // Non-affine kinds are decided by enumeration over the realized domain.
        // The cap is what stops a `Recurrence` on a wide range from turning a
        // summary pass into a hang; above it the answer is `None`, which the
        // caller reports as "no region" rather than as a wrong one.
        _ => {
            let mut pts: Vec<i64> = Vec::new();
            let mut cur = vec![0i64; loop_.depth];
            enumerate_locs(access, &ivs, 0, &mut cur, &mut pts)?;
            if pts.is_empty() {
                return None;
            }
            Some(Region::span(*pts.iter().min()?, *pts.iter().max()?))
        }
    }
}

/// Enumerate an access's locations over the realized IV domain. The recursion
/// bounds itself by [`ENUM_CAP`] through [`count_iterations`], so a wide range
/// yields "no region" instead of a hang.
fn enumerate_locs(
    access: &Access,
    ivs: &[IV],
    d: usize,
    cur: &mut Vec<i64>,
    out: &mut Vec<i64>,
) -> Option<()> {
    if d == ivs.len() {
        out.push(eval_location(access, cur)?);
        return Some(());
    }
    let mut v = ivs[d].lower;
    loop {
        cur[d] = v;
        enumerate_locs(access, ivs, d + 1, cur, out)?;
        if v >= ivs[d].upper {
            return Some(());
        }
        v = v.checked_add(ivs[d].step)?;
        if out.len() > ENUM_CAP {
            return None;
        }
    }
}

/// The element index an access computes at a concrete IV vector, including its
/// offset — mirroring exactly what M0 and M6–M11 evaluate. This is what makes it
/// usable as the §6.3 oracle: [`region_of_access`] derives bounds analytically
/// (the affine corner argument), this derives locations one at a time, and the
/// fuzz test requires the first to contain the second.
pub fn eval_location(access: &Access, iv: &[i64]) -> Option<i64> {
    let offset = access.offset.as_const()?;
    let base = match &access.index_kind {
        IndexKind::Affine => {
            if access.index.len() != iv.len() {
                return None;
            }
            let mut acc: i64 = 0;
            for (&c, &v) in access.index.iter().zip(iv.iter()) {
                acc = acc.checked_add(c.checked_mul(v)?)?;
            }
            acc
        }
        IndexKind::Product(e) => e.eval(iv, &[], None)?,
        IndexKind::Hash(h) => h.eval(*iv.first()?),
        IndexKind::Recurrence(r) => {
            let k = *iv.first()?;
            if k < 0 {
                return None;
            }
            r.eval(k as usize)?
        }
        IndexKind::FixedPoint(t) => t.eval(*iv.first()?)?,
        IndexKind::SymbolicFixedPoint { next_expr, max_steps } => {
            let mut c = *iv.first()?;
            let mut converged = false;
            for _ in 0..*max_steps {
                let nx = next_expr.eval(&[c], &[], None)?;
                if nx == c {
                    converged = true;
                    break;
                }
                c = nx;
            }
            if !converged {
                return None;
            }
            c
        }
        IndexKind::Range(range) => {
            match range {
                RangeExpr::Mod { iv: idx, modulus } => {
                    if *modulus <= 0 { return None; }
                    iv.get(*idx)?.rem_euclid(*modulus)
                }
                RangeExpr::ModParam { iv: idx, modulus } => {
                    // Without environment, cannot evaluate
                    return None;
                }
                RangeExpr::Min { iv: idx, cap } => {
                    let v = iv.get(*idx)?;
                    *std::cmp::min(v, cap)
                }
                RangeExpr::Max { iv: idx, floor } => {
                    let v = iv.get(*idx)?;
                    *std::cmp::max(v, floor)
                }
            }
        }
        IndexKind::Polyhedral { a, b: _, dim } => {
            if iv.len() >= *dim {
                let mut loc = 0i64;
                for k in 0..*dim {
                    let coeff = a.first().map(|row| row.get(k).copied().unwrap_or(0)).unwrap_or(0);
                    let val = iv[k];
                    loc = loc.checked_add(coeff.checked_mul(val)?)?;
                }
                loc
            } else {
                return None;
            }
        }
    };
    base.checked_add(offset)
}

/// The IVs an access actually runs over: the loop's IVs restricted to the
/// realized lattice, then narrowed by the access's own affine guard. The
/// returned IVs are **inclusive** on `upper`, unlike the IR's exclusive bound.
///
/// `upper` is exclusive in the IR and `step` may exceed 1, so the last value the
/// loop performs is `lower + step·⌊(upper-1-lower)/step⌋`, not `upper - 1`.
/// Getting this wrong makes `A[i]` over `[0,8)` step 2 report region `[0:7]` and
/// a summary claim a write to `A[7]` that never happens.
///
/// `None` means the access is **dead**: a non-positive step, an empty loop, or a
/// guard no iteration satisfies. A dead access contributes nothing to `MayUse`
/// or `MayMod`, and per §8.1.4 nothing it "writes" belongs in `Kill` either.
fn realized_ivs(access: &Access, loop_: &Loop) -> Option<Vec<IV>> {
    if loop_.ivs.is_empty() || loop_.ivs.len() != loop_.depth {
        return None;
    }
    let mut out: Vec<IV> = Vec::with_capacity(loop_.depth);
    for iv in &loop_.ivs {
        if iv.step <= 0 || iv.lower >= iv.upper {
            return None;
        }
        let last = (iv.upper - 1 - iv.lower) / iv.step * iv.step + iv.lower;
        out.push(IV::new(iv.name.clone(), iv.lower, last, iv.step));
    }
    if let Some(g) = &access.guard {
        for c in &g.constraints {
            let iv = out.get_mut(c.iv)?;
            match c.op {
                CmpOp::Lt => iv.upper = iv.upper.min(c.bound - 1),
                CmpOp::Le => iv.upper = iv.upper.min(c.bound),
                CmpOp::Gt => iv.lower = iv.lower.max(c.bound + 1),
                CmpOp::Ge => iv.lower = iv.lower.max(c.bound),
            };
            if iv.lower > iv.upper {
                return None;
            }
        }
    }
    Some(out)
}

/// The region of a `RangeExpr` access, in location space.
///
/// The bounds are the IR's own documented semantics for each shape: `A[i % C]`
/// covers `[0, C-1]`; `A[min(i,K)]` covers `[lower, min(upper-1, K)]`;
/// `A[max(i,K)]` covers `[max(lower,K), upper-1]`. `ModParam` is refused — its
/// modulus is a runtime parameter, and a region invented for an unbound modulus
/// would be a claim about an array whose size the analysis does not know.
pub fn region_of_range(range: &RangeExpr, loop_: &Loop) -> Option<Region> {
    let k = *range_iv(range)?;
    match range {
        RangeExpr::Mod { modulus, .. } => {
            if *modulus <= 0 {
                return None;
            }
            Some(Region::span(0, modulus - 1))
        }
        RangeExpr::ModParam { .. } => None,
        RangeExpr::Min { cap, .. } => {
            let iv = loop_.ivs.get(k).filter(|iv| iv.step > 0 && iv.lower < iv.upper)?;
            let hi = (iv.upper - 1).min(*cap);
            if iv.lower > hi {
                return None;
            }
            Some(Region::span(iv.lower, hi))
        }
        RangeExpr::Max { floor, .. } => {
            let iv = loop_.ivs.get(k).filter(|iv| iv.step > 0 && iv.lower < iv.upper)?;
            let lo = iv.lower.max(*floor);
            if lo > iv.upper - 1 {
                return None;
            }
            Some(Region::span(lo, iv.upper - 1))
        }
    }
}

fn range_iv(range: &RangeExpr) -> Option<&usize> {
    Some(match range {
        RangeExpr::Mod { iv, .. } | RangeExpr::ModParam { iv, .. } => iv,
        RangeExpr::Min { iv, .. } | RangeExpr::Max { iv, .. } => iv,
    })
}

/// The locations an access visits, enumerated exactly, or `None` when the domain
/// exceeds `cap` iterations or the access is dead.
///
/// This is the §6.3 oracle. The fuzzers compare [`region_of_access`]'s box
/// against this set and require containment; because the analytic affine path
/// uses a corner argument and this path does not, the two can disagree in
/// principle — which is exactly why the comparison is worth running.
pub fn locations_of_access(access: &Access, loop_: &Loop, cap: usize) -> Option<Vec<i64>> {
    let ivs = realized_ivs(access, loop_)?;
    let mut total: u64 = 1;
    for iv in &ivs {
        let count = ((iv.upper - iv.lower) as u64 + iv.step as u64 - 1) / iv.step as u64;
        total = total.checked_mul(count)?;
        if total as usize > cap {
            return None;
        }
    }
    let mut out = Vec::with_capacity(total as usize);
    let mut cur = vec![0i64; ivs.len()];
    enumerate_locs(access, &ivs, 0, &mut cur, &mut out)?;
    Some(out)
}

/// The union of every access's region over the whole loop body. A loop that
/// touches nothing yields an empty set rather than `None`: "no regions" is the
/// honest answer, while `None` would mean "I could not tell".
pub fn region_of_accesses(loop_: &Loop) -> RegionSet {
    let mut set = RegionSet::new();
    for a in &loop_.body {
        if let Some(r) = region_of_access(a, loop_) {
            set.insert(r);
        }
    }
    set
}

/// The regions of every access of `loop_` whose kind is `k`.
pub fn region_of_kind(loop_: &Loop, k: AccessKind) -> RegionSet {
    let mut set = RegionSet::new();
    for a in &loop_.body {
        if a.kind == k {
            if let Some(r) = region_of_access(a, loop_) {
                set.insert(r);
            }
        }
    }
    set
}



/// `true` when the access reads rather than writes.
pub fn is_read(access: &Access) -> bool {
    access.kind == AccessKind::Read
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Cmp, Guard, IndexExpr, OffsetValue, IV};

    fn aff(kind: AccessKind, coeff: i64, offset: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![coeff],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn loop1(lo: i64, hi: i64, step: i64) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", lo, hi, step)],
            body: vec![],
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    #[test]
    fn union_and_intersection_follow_wolfe_8_1_2() {
        let a = Region::span(0, 3);
        let b = Region::span(8, 11);
        assert_eq!(a.union(&b), Some(Region::span(0, 11)));
        // Disjoint intersection is the empty box, not `None`: `Kill` must be able
        // to say "no path overwrites a common element".
        let i = a.intersect(&b).unwrap();
        assert!(i.is_empty());
        assert_eq!(Region::span(0, 9).intersect(&Region::span(5, 20)), Some(Region::span(5, 9)));
    }

    #[test]
    fn arity_mismatch_is_refused_not_bounding_boxed() {
        let one = Region::span(0, 7);
        let two = Region::new(vec![(0, 3), (0, 3)]);
        assert_eq!(one.union(&two), None);
        assert_eq!(one.intersect(&two), None);
        assert!(!one.is_subset_of(&two));
    }

    #[test]
    fn region_of_a_stepped_loop_uses_the_last_realized_value() {
        // [0,8) step 2 realizes 0,2,4,6 — not 7. The classic off-by-one here
        // claims a write to A[7] that the loop never performs.
        let l = loop1(0, 8, 2);
        let a = aff(AccessKind::Write, 1, 0);
        assert_eq!(region_of_access(&a, &l), Some(Region::span(0, 6)));
        assert_eq!(locations_of_access(&a, &l, 64), Some(vec![0, 2, 4, 6]));
    }

    #[test]
    fn guard_narrows_the_region_and_a_dead_guard_yields_no_region() {
        let mut l = loop1(0, 8, 1);
        let mut a = aff(AccessKind::Write, 1, 0);
        a.guard = Some(Guard { constraints: vec![Cmp { iv: 0, op: CmpOp::Lt, bound: 4 }] });
        l.body.push(a.clone());
        assert_eq!(region_of_access(&a, &l), Some(Region::span(0, 3)));
        // A guard no iteration satisfies is a dead access: `None`, and `None` is
        // not "empty" — the caller must not put it into `Kill`.
        let dead = Access { guard: Some(Guard { constraints: vec![Cmp { iv: 0, op: CmpOp::Gt, bound: 100 }] }), ..a };
        assert_eq!(region_of_access(&dead, &l), None);
        assert_eq!(locations_of_access(&dead, &l, 64), None);
    }

    #[test]
    fn negative_coefficient_swaps_the_endpoints() {
        let l = loop1(0, 8, 1);
        let a = aff(AccessKind::Read, -1, 10);
        assert_eq!(region_of_access(&a, &l), Some(Region::span(3, 10)));
    }

    #[test]
    fn analytic_region_contains_the_enumerated_locations() {
        // The §6.3 fuzz strategy, in miniature and over a fixed shape set: the
        // corner argument used for affine regions must not exclude a location
        // the loop actually visits.
        let l = loop1(0, 6, 1);
        for (c, k) in [(1i64, 0i64), (2, -3), (-1, 5), (3, 1)] {
            let a = aff(AccessKind::Write, c, k);
            let r = region_of_access(&a, &l).unwrap();
            for loc in locations_of_access(&a, &l, 4096).unwrap() {
                assert!(r.contains(&[loc]), "c={c} k={k}: {loc} outside {}", r.to_string_repr());
            }
        }
    }

    #[test]
    fn product_region_matches_enumeration() {
        let mut l = loop1(0, 5, 1);
        let a = Access {
            index_kind: IndexKind::Product(IndexExpr::Mul(
                Box::new(IndexExpr::IV(0)),
                Box::new(IndexExpr::IV(0)),
            )),
            ..aff(AccessKind::Read, 0, 0)
        };
        l.body.push(a.clone());
        // i*i over 0..5 = {0,1,4,9,16}
        assert_eq!(region_of_access(&a, &l), Some(Region::span(0, 16)));
        assert_eq!(locations_of_access(&a, &l, 4096), Some(vec![0, 1, 4, 9, 16]));
    }

    #[test]
    fn range_accesses_use_the_ir_documented_bounds() {
        let l = loop1(0, 10, 1);
        assert_eq!(
            region_of_range(&RangeExpr::Mod { iv: 0, modulus: 4 }, &l),
            Some(Region::span(0, 3))
        );
        assert_eq!(
            region_of_range(&RangeExpr::Min { iv: 0, cap: 6 }, &l),
            Some(Region::span(0, 6))
        );
        assert_eq!(
            region_of_range(&RangeExpr::Max { iv: 0, floor: 3 }, &l),
            Some(Region::span(3, 9))
        );
        // Modulus unknown at analysis time: refuse rather than invent a size.
        assert_eq!(
            region_of_range(
                &RangeExpr::ModParam { iv: 0, modulus: crate::param_expr::ParamExpr::Param("p".into()) },
                &l
            ),
            None
        );
    }

    #[test]
    fn region_set_merges_overlap_and_keeps_arity_separate() {
        let mut s = RegionSet::new();
        s.insert(Region::span(0, 3));
        s.insert(Region::span(2, 7));
        assert_eq!(s.regions.len(), 1);
        assert_eq!(s.regions[0], Region::span(0, 7));
        s.insert(Region::new(vec![(0, 1), (0, 1)]));
        assert_eq!(s.regions.len(), 2, "a 2-D box is not merged into a 1-D span");
        assert!(s.misses(&[9]));
        assert!(!s.misses(&[5]));
        // Empty regions carry no information and are dropped, so a `Kill` set
        // never looks non-empty while containing nothing.
        s.insert(Region::empty());
        assert_eq!(s.regions.len(), 2);
    }

    #[test]
    fn parametric_offset_is_refused() {
        let l = loop1(0, 8, 1);
        let a = Access {
            offset: OffsetValue::Param(crate::param_expr::ParamExpr::Param("n".into())),
            ..aff(AccessKind::Write, 1, 0)
        };
        assert_eq!(region_of_access(&a, &l), None);
    }

    #[test]
    fn enumerate_is_bounded_and_lexicographic() {
        let r = Region::new(vec![(0, 1), (0, 2)]);
        assert_eq!(
            r.enumerate(16),
            Some(vec![
                vec![0, 0],
                vec![0, 1],
                vec![0, 2],
                vec![1, 0],
                vec![1, 1],
                vec![1, 2]
            ])
        );
        assert_eq!(r.enumerate(5), None);
        assert_eq!(Region::universe().count(), None);
    }
}
