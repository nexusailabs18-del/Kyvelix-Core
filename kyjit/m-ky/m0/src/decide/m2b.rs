//! M2b — bounded range.
//!
//! Handles index expressions that are non-affine but whose location set
//! is a bounded interval: `A[i % C]`, `A[min(i, K)]`, `A[max(i, K)]`.
//!
//! Decision: compute each access's location interval. Disjoint -> None.
//! Overlapping -> Exact with a witness that realizes a common location.
//! Otherwise -> Unknown.

use super::Decision;
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;

// ============================================================================
// Interval helpers
// ============================================================================

/// Inclusive [lo, hi] interval of locations covered by a range expression,
/// ignoring any access-level offset. Returns None for invalid ranges.
fn range_interval(r: &RangeExpr, ivs: &[IV]) -> Option<(i64, i64)> {
    match r {
        RangeExpr::Mod { iv: _, modulus } => {
            if *modulus > 0 { Some((0, modulus - 1)) } else { None }
        }
        RangeExpr::ModParam { .. } => None,
        RangeExpr::Min { iv, cap } => {
            if *iv >= ivs.len() { return None; }
            let lo = ivs[*iv].lower;
            let hi = ivs[*iv].upper - 1;
            // min(iv, cap) is contiguous: [lo, cap] when the cap is inside
            // the window; the single value `cap` when it is below it.
            Some((lo.min(*cap), hi.min(*cap)))
        }
        RangeExpr::Max { iv, floor } => {
            if *iv >= ivs.len() { return None; }
            let lo = ivs[*iv].lower;
            let hi = ivs[*iv].upper - 1;
            // max(iv, floor) is contiguous: [floor, hi] when the floor is
            // inside the window; the single value `floor` when it is above it.
            Some((lo.max(*floor), hi.max(*floor)))
        }
    }
}

/// Inclusive location interval of an access whose `range` is None,
/// computed over the IV box as index·i + offset.
fn affine_interval(access: &Access, ivs: &[IV]) -> Option<(i64, i64)> {
    let offset = access.offset.as_const()?;
    let mut lo = 0i64;
    let mut hi = 0i64;
    for k in 0..ivs.len() {
        let c = access.index[k];
        let iv_lo = ivs[k].lower;
        let iv_hi = ivs[k].upper - 1;
        let v1 = c.checked_mul(iv_lo)?;
        let v2 = c.checked_mul(iv_hi)?;
        lo = lo.checked_add(v1.min(v2))?;
        hi = hi.checked_add(v1.max(v2))?;
    }
    Some((lo + offset, hi + offset))
}

/// Total location interval for an access, adding offset on the range
/// branch (mirroring the checker's `location_interval`).
fn access_interval(access: &Access, ivs: &[IV]) -> Option<(i64, i64)> {
    match &access.range {
        None => affine_interval(access, ivs),
        Some(r) => {
            let (rlo, rhi) = range_interval(r, ivs)?;
            let off = access.offset.as_const()?;
            Some((rlo + off, rhi + off))
        }
    }
}

fn disjoint(a: (i64, i64), b: (i64, i64)) -> bool { a.1 < b.0 || b.1 < a.0 }

/// Inclusive intersection of two intervals (`None` when disjoint).
/// The decision paths now use [`min_common`] on exact realized sets; this
/// helper survives for the disjointness guard and the unit tests.
#[allow(dead_code)]
fn overlap(a: (i64, i64), b: (i64, i64)) -> Option<(i64, i64)> {
    let lo = a.0.max(b.0);
    let hi = a.1.min(b.1);
    if lo <= hi { Some((lo, hi)) } else { None }
}

/// Location an access produces for a specific IV assignment, respecting
/// the range expression and the access-level offset.
/// Returns None if the offset is symbolic (not a concrete constant).
fn access_location(access: &Access, iv_values: &[i64]) -> Option<i64> {
    let off = access.offset.as_const()?;
    if let Some(range) = &access.range {
        let base = match range {
            RangeExpr::Mod { iv, modulus } => {
                if *modulus <= 0 { return None; }
                iv_values[*iv].rem_euclid(*modulus)
            }
            RangeExpr::ModParam { .. } => return None,
            RangeExpr::Min { iv, cap } => iv_values[*iv].min(*cap),
            RangeExpr::Max { iv, floor } => iv_values[*iv].max(*floor),
        };
        Some(base + off)
    } else {
        let mut loc = off;
        for k in 0..iv_values.len() {
            loc += access.index[k] * iv_values[k];
        }
        Some(loc)
    }
}

/// For a depth-1 access with unit step, find the *smallest* IV value in the
/// domain that realizes location `l`, or None if no such value exists.
///
/// Exact for every range kind: a `Mod` side realizes `l` iff the residue
/// class `(l - off) mod m` is hit by some IV in `[lower, upper)` (the old
/// search only checked the smallest residue itself, rejecting witnesses
/// below `lower` whose class is still hit inside the window); `Min`/`Max`
/// invert directly; affine sides divide. No scan, no cap.
fn solve_side(access: &Access, l: i64, ivs: &[IV]) -> Option<i64> {
    solve_side_with(access, l, ivs, None)
}

/// The shared body of `solve_side` / `solve_side_env`. `env = Some(_)`
/// resolves `ModParam` moduli; `env = None` makes a `ModParam` side
/// unsolvable (the caller then declines to Unknown).
fn solve_side_with(
    access: &Access,
    l: i64,
    ivs: &[IV],
    env: Option<&[(String, i64)]>,
) -> Option<i64> {
    let iv0 = ivs.first()?;
    if iv0.step != 1 {
        // Non-unit steps need congruence solving on the lattice; decline
        // (conservative: Unknown, never a wrong witness).
        return None;
    }
    let (lo, hi) = (iv0.lower, iv0.upper);
    let off = access.offset.as_const()?;

    // Smallest iv >= lo with iv ≡ delta (mod m).
    let smallest_congruent = |delta: i64, m: i64| -> Option<i64> {
        let d = delta.rem_euclid(m);
        let v = lo + (d - lo).rem_euclid(m);
        if v < hi { Some(v) } else { None }
    };

    match &access.range {
        Some(RangeExpr::Mod { modulus, .. }) => {
            if *modulus <= 0 { return None; }
            let delta = l.checked_sub(off)?;
            if delta < 0 || delta >= *modulus { return None; }
            smallest_congruent(delta, *modulus)
        }
        Some(RangeExpr::ModParam { modulus, .. }) => {
            let env = env?;
            let m = modulus.eval(env)?;
            if m <= 0 { return None; }
            let delta = l.checked_sub(off)?;
            if delta < 0 || delta >= m { return None; }
            smallest_congruent(delta, m)
        }
        Some(RangeExpr::Min { cap, .. }) => {
            let target = l.checked_sub(off)?;
            if target < *cap {
                if target >= lo && target < hi { Some(target) } else { None }
            } else if target == *cap {
                let v = lo.max(*cap);
                if v < hi { Some(v) } else { None }
            } else {
                None
            }
        }
        Some(RangeExpr::Max { floor, .. }) => {
            let target = l.checked_sub(off)?;
            if target > *floor {
                if target >= lo && target < hi { Some(target) } else { None }
            } else if target == *floor {
                if lo < hi && lo <= *floor { Some(lo) } else { None }
            } else {
                None
            }
        }
        None => {
            let target = l.checked_sub(off)?;
            let c = access.index.first().copied().unwrap_or(0);
            if c == 0 {
                if target == 0 && lo < hi { Some(lo) } else { None }
            } else if target % c != 0 {
                None
            } else {
                let v = target / c;
                if v >= lo && v < hi { Some(v) } else { None }
            }
        }
    }
}

// ============================================================================
// Decision
// ============================================================================

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    if ap.range.is_none() && aq.range.is_none() {
        return (Decision::Unknown("M2b: no range on either access".into()), None);
    }

    // Compute both location intervals (range + offset, or affine).
    let rp = match access_interval(ap, &loop_.ivs) {
        Some(iv) => iv,
        None => return (Decision::Unknown("M2b: cannot compute interval for p".into()), None),
    };
    let rq = match access_interval(aq, &loop_.ivs) {
        Some(iv) => iv,
        None => return (Decision::Unknown("M2b: cannot compute interval for q".into()), None),
    };

    // Disjoint intervals -> None.
    if disjoint(rp, rq) {
        let cert = Certificate {
            level: "M2b".into(),
            loop_hash: loop_.hash(),
            pair: (p, q),
            claim: Claim::None,
            witness: Witness::UnsatProof {
                constraints: vec![CConstraint {
                    coeffs: vec![0; loop_.depth],
                    constant: -1,
                }],
            },
            checker: "m0-check".into(),
        };
        return (Decision::None, Some(cert));
    }

    // Overlapping: search the overlap for a common location that both
    // sides can realize.
    if loop_.depth != 1 {
        return (Decision::Unknown("M2b: depth>1 unsupported".into()), None);
    }

    // Exact witness search: closed-form realized sets, no scan, no cap.
    let ra = match realized_set(ap, &loop_.ivs, None) {
        Some(r) => r,
        None => return (Decision::Unknown("M2b: cannot characterize p exactly".into()), None),
    };
    let rb = match realized_set(aq, &loop_.ivs, None) {
        Some(r) => r,
        None => return (Decision::Unknown("M2b: cannot characterize q exactly".into()), None),
    };
    if let Some(l) = min_common(&ra, &rb) {
        if let (Some(i0), Some(j0)) =
            (solve_side(ap, l, &loop_.ivs), solve_side(aq, l, &loop_.ivs))
        {
            // Defensive: recompute both locations and require equality at
            // the common value. A mismatch means the characterization and
            // the evaluator disagree -- decline rather than emit.
            let li = match access_location(ap, &[i0]) {
                Some(v) => v,
                None => return (Decision::Unknown("M2b: symbolic offset".into()), None),
            };
            let lj = match access_location(aq, &[j0]) {
                Some(v) => v,
                None => return (Decision::Unknown("M2b: symbolic offset".into()), None),
            };
            if li == lj && li == l {
                let cert = Certificate {
                    level: "M2b".into(),
                    loop_hash: loop_.hash(),
                    pair: (p, q),
                    claim: Claim::Exact(vec![j0 - i0]),
                    witness: Witness::IntSolution { i: vec![i0], j: vec![j0] },
                    checker: "m0-check".into(),
                };
                return (Decision::Exact(vec![j0 - i0]), Some(cert));
            }
        }
    }

    (Decision::Unknown("M2b: overlap but no witness found".into()), None)
}


// ============================================================================
// M9 — runtime modulus (ModParam)
// ============================================================================

/// Environment-aware interval computation. Handles `ModParam` by
/// resolving the modulus against `env`.
fn range_interval_env(
    r: &RangeExpr,
    ivs: &[IV],
    env: &[(String, i64)],
) -> Option<(i64, i64)> {
    match r {
        RangeExpr::ModParam { iv: _, modulus } => {
            let m = modulus.eval(env)?;
            if m > 0 { Some((0, m - 1)) } else { None }
        }
        _ => range_interval(r, ivs),
    }
}

/// Environment-aware access interval.
fn access_interval_env(
    access: &Access,
    ivs: &[IV],
    env: &[(String, i64)],
) -> Option<(i64, i64)> {
    match &access.range {
        None => affine_interval(access, ivs),
        Some(r) => {
            let (rlo, rhi) = range_interval_env(r, ivs, env)?;
            let off = access.offset.as_const()?;
            Some((rlo + off, rhi + off))
        }
    }
}

/// Environment-aware location of an access at a concrete IV vector.
fn access_location_env(
    access: &Access,
    iv_values: &[i64],
    env: &[(String, i64)],
) -> Option<i64> {
    let off = access.offset.as_const()?;
    if let Some(range) = &access.range {
        let base = match range {
            RangeExpr::Mod { iv, modulus } => {
                if *modulus <= 0 { return None; }
                iv_values[*iv].rem_euclid(*modulus)
            }
            RangeExpr::ModParam { iv, modulus } => {
                let m = modulus.eval(env)?;
                if m <= 0 { return None; }
                iv_values[*iv].rem_euclid(m)
            }
            RangeExpr::Min { iv, cap } => iv_values[*iv].min(*cap),
            RangeExpr::Max { iv, floor } => iv_values[*iv].max(*floor),
        };
        Some(base + off)
    } else {
        let mut loc = off;
        for k in 0..iv_values.len() {
            loc += access.index[k] * iv_values[k];
        }
        Some(loc)
    }
}

/// Env-aware version of `solve_side` (resolves `ModParam` against `env`).
fn solve_side_env(
    access: &Access,
    l: i64,
    ivs: &[IV],
    env: &[(String, i64)],
) -> Option<i64> {
    solve_side_with(access, l, ivs, Some(env))
}

// ============================================================================
// Exact witness search (replaces the capped linear scan)
// ============================================================================
//
// The old search walked the overlap of the two location intervals value by
// value, capped at 4096 candidates. A common location beyond the 4097th
// candidate was missed entirely. The search below is exact for depth-1
// unit-step loops: each side's realized location set is a union of at most
// two contiguous intervals (`Mod` sides whose IV window wraps the modulus),
// or an arithmetic progression (affine sides). The smallest common element
// of two such sets is found in O(1) per piece pair, so witnesses are found
// with no scan and no cap. Every witness is still re-verified by the
// checker from scratch; decline (Unknown) is the only failure mode.

/// The realized location set of one access on a depth-1 unit-step loop.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Realized {
    /// Every integer location in the inclusive interval.
    Interval(i64, i64),
    /// Two intervals (a `Mod` side whose IV window wraps the modulus).
    TwoIntervals((i64, i64), (i64, i64)),
    /// Locations `start + k·step`, `k ∈ [0, count)`, `step > 0`.
    Progression { start: i64, step: i64, count: i64 },
}

impl Realized {
    fn intervals(&self) -> Vec<(i64, i64)> {
        match self {
            Realized::Interval(a, b) => vec![(*a, *b)],
            Realized::TwoIntervals(a, b) => vec![*a, *b],
            Realized::Progression { .. } => vec![],
        }
    }
}

/// The realized location set of `access`, exactly. `None` on non-unit step
/// or overflow (the caller then declines to Unknown -- never a wrong answer).
fn realized_set(
    access: &Access,
    ivs: &[IV],
    env: Option<&[(String, i64)]>,
) -> Option<Realized> {
    let iv0 = ivs.first()?;
    if iv0.step != 1 || iv0.lower >= iv0.upper {
        return None;
    }
    let (lo, hi) = (iv0.lower, iv0.upper); // iv ∈ [lo, hi)
    let off = access.offset.as_const()?;
    match &access.range {
        Some(r @ (RangeExpr::Mod { .. } | RangeExpr::ModParam { .. })) => {
            let m = match r {
                RangeExpr::Mod { modulus, .. } => *modulus,
                RangeExpr::ModParam { modulus, .. } => {
                    let env = env?;
                    modulus.eval(env)?
                }
                _ => unreachable!(),
            };
            if m <= 0 {
                return None;
            }
            let count = hi.checked_sub(lo)?;
            if count >= m {
                // The IV window covers every residue.
                Some(Realized::Interval(off, off.checked_add(m - 1)?))
            } else {
                // Shorter than the modulus: the residues it hits form a
                // contiguous cyclic range [first, last].
                let first = lo.rem_euclid(m);
                let last = hi.checked_sub(1)?.rem_euclid(m);
                if first <= last {
                    Some(Realized::Interval(
                        off.checked_add(first)?,
                        off.checked_add(last)?,
                    ))
                } else {
                    Some(Realized::TwoIntervals(
                        (off.checked_add(first)?, off.checked_add(m - 1)?),
                        (off, off.checked_add(last)?),
                    ))
                }
            }
        }
        Some(RangeExpr::Min { cap, .. }) => {
            // Values are min(iv, cap) + off: always a contiguous interval.
            let top = hi.checked_sub(1)?;
            let v_lo = lo.min(*cap);
            let v_hi = top.min(*cap);
            Some(Realized::Interval(
                v_lo.checked_add(off)?,
                v_hi.checked_add(off)?,
            ))
        }
        Some(RangeExpr::Max { floor, .. }) => {
            let top = hi.checked_sub(1)?;
            let v_lo = lo.max(*floor);
            let v_hi = top.max(*floor);
            Some(Realized::Interval(
                v_lo.checked_add(off)?,
                v_hi.checked_add(off)?,
            ))
        }
        None => {
            let c = access.index.first().copied().unwrap_or(0);
            let count = hi - lo;
            if c == 0 {
                Some(Realized::Interval(off, off))
            } else if c > 0 {
                let start = c.checked_mul(lo)?.checked_add(off)?;
                Some(Realized::Progression { start, step: c, count })
            } else {
                // Decreasing: rewrite with a positive step.
                let start = c.checked_mul(hi - 1)?.checked_add(off)?;
                Some(Realized::Progression { start, step: -c, count })
            }
        }
    }
}

/// Smallest element of the progression `start + k·step` (`k ∈ [0, count)`)
/// that lies in `[lo, hi]`.
fn first_in_interval(start: i64, step: i64, count: i64, lo: i64, hi: i64) -> Option<i64> {
    if count <= 0 || step <= 0 {
        return None;
    }
    let k_lo = if start >= lo {
        0
    } else {
        let d = lo.checked_sub(start)?;
        // ceil(d / step)
        d.checked_add(step - 1)? / step
    };
    if k_lo < 0 || k_lo >= count {
        return None;
    }
    let v = start.checked_add(k_lo.checked_mul(step)?)?;
    if v <= hi { Some(v) } else { None }
}

/// Smallest common location of two realized sets, if any. Exact: each set
/// is closed-form, so absence is a proof (the caller still answers Unknown
/// rather than None, because the checker must be able to re-derive it).
fn min_common(a: &Realized, b: &Realized) -> Option<i64> {
    let mut best: Option<i64> = None;
    let mut consider = |v: i64| {
        best = Some(best.map_or(v, |b| b.min(v)));
    };

    // Interval ∩ interval.
    for &(la, ha) in &a.intervals() {
        for &(lb, hb) in &b.intervals() {
            let lo = la.max(lb);
            let hi = ha.min(hb);
            if lo <= hi {
                consider(lo);
            }
        }
    }

    // Progression ∩ interval (either orientation).
    if let Realized::Progression { start, step, count } = a {
        for &(lb, hb) in &b.intervals() {
            if let Some(v) = first_in_interval(*start, *step, *count, lb, hb) {
                consider(v);
            }
        }
    }
    if let Realized::Progression { start, step, count } = b {
        for &(la, ha) in &a.intervals() {
            if let Some(v) = first_in_interval(*start, *step, *count, la, ha) {
                consider(v);
            }
        }
    }

    // Progression ∩ progression (linear Diophantine via extended gcd).
    // Unreachable through today's entry points (at least one side always
    // carries a range); kept for completeness and exercised by this
    // module's unit tests.
    if let (
        Realized::Progression { start: sa, step: ssa, count: ca },
        Realized::Progression { start: sb, step: ssb, count: cb },
    ) = (a, b)
    {
        if let Some((k0, m0, dk, dm)) =
            crate::solver::gcd::general_solution_2(*ssa, -(*ssb), sb - sa)
        {
            // k = k0 + dk·t must lie in [0, ca); m = m0 + dm·t in [0, cb).
            let tl = match (t_low(k0, dk, *ca), t_low(m0, dm, *cb)) {
                (Some(x), Some(y)) => Some(x.max(y)),
                _ => None,
            };
            let th = match (t_high(k0, dk, *ca), t_high(m0, dm, *cb)) {
                (Some(x), Some(y)) => Some(x.min(y)),
                _ => None,
            };
            if let (Some(tl), Some(th)) = (tl, th) {
                if tl <= th {
                    // v(t) = sa + (k0 + dk·t)·ssa is monotone in t, so the
                    // smallest common value is at one end of the range.
                    for t in [tl, th] {
                        if let Some(k) = dk.checked_mul(t).and_then(|x| k0.checked_add(x)) {
                            if let Some(kv) = k.checked_mul(*ssa) {
                                if let Some(v) = sa.checked_add(kv) {
                                    consider(v);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    best
}

/// Smallest `t` with `x0 + d·t >= 0` (or `None` if impossible).
fn t_low(x0: i64, d: i64, count: i64) -> Option<i64> {
    if count <= 0 {
        return None;
    }
    if d == 0 {
        if x0 >= 0 && x0 < count { Some(i64::MIN / 4) } else { None }
    } else if d > 0 {
        // t >= ceil(-x0 / d)
        let n = x0.checked_neg()?;
        Some(ceil_div_pos(n, d))
    } else {
        // d < 0 with g = -d: t >= ceil((x0 - (count - 1)) / g)
        let g = d.checked_neg()?;
        let n = x0.checked_sub(count - 1)?;
        Some(ceil_div_pos(n, g))
    }
}

/// Largest `t` with `x0 + d·t <= count - 1` (or `None` if impossible).
fn t_high(x0: i64, d: i64, count: i64) -> Option<i64> {
    if count <= 0 {
        return None;
    }
    if d == 0 {
        if x0 >= 0 && x0 < count { Some(i64::MAX / 4) } else { None }
    } else if d > 0 {
        // t <= floor((count - 1 - x0) / d)
        let n = (count - 1).checked_sub(x0)?;
        Some(n.div_euclid(d))
    } else {
        // d < 0 with g = -d: t <= floor(x0 / g)
        let g = d.checked_neg()?;
        Some(x0.div_euclid(g))
    }
}

/// Ceiling division by a positive divisor.
fn ceil_div_pos(n: i64, d: i64) -> i64 {
    n.div_euclid(d) + if n.rem_euclid(d) != 0 { 1 } else { 0 }
}

/// M9 entry point: like `decide`, but resolves `ModParam` against `env`.
/// Returns `(Decision, Option<Certificate>)`. Falls back to `decide` when
/// neither access uses `ModParam`.
pub fn decide_param(
    loop_: &Loop,
    p: usize,
    q: usize,
    env: &[(String, i64)],
) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    let uses_modparam =
        |a: &Access| matches!(&a.range, Some(RangeExpr::ModParam { .. }));
    if !uses_modparam(ap) && !uses_modparam(aq) {
        return decide(loop_, p, q);
    }

    if loop_.depth != 1 {
        return (Decision::Unknown("M9: depth>1 unsupported".into()), None);
    }

    let rp = match access_interval_env(ap, &loop_.ivs, env) {
        Some(x) => x,
        None => {
            return (
                Decision::Unknown("M9: cannot compute interval for p".into()),
                None,
            )
        }
    };
    let rq = match access_interval_env(aq, &loop_.ivs, env) {
        Some(x) => x,
        None => {
            return (
                Decision::Unknown("M9: cannot compute interval for q".into()),
                None,
            )
        }
    };

    if disjoint(rp, rq) {
        let cert = Certificate {
            level: "M9".into(),
            loop_hash: loop_.hash(),
            pair: (p, q),
            claim: Claim::None,
            witness: Witness::WithEnv {
                env: env.to_vec(),
                inner: Box::new(Witness::UnsatProof {
                    constraints: vec![CConstraint {
                        coeffs: vec![0; loop_.depth],
                        constant: -1,
                    }],
                }),
            },
            checker: "m0-check".into(),
        };
        return (Decision::None, Some(cert));
    }

    // Exact witness search: closed-form realized sets, no scan, no cap.
    let ra = match realized_set(ap, &loop_.ivs, Some(env)) {
        Some(r) => r,
        None => return (Decision::Unknown("M9: cannot characterize p exactly".into()), None),
    };
    let rb = match realized_set(aq, &loop_.ivs, Some(env)) {
        Some(r) => r,
        None => return (Decision::Unknown("M9: cannot characterize q exactly".into()), None),
    };
    if let Some(l) = min_common(&ra, &rb) {
        if let (Some(i0), Some(j0)) = (
            solve_side_env(ap, l, &loop_.ivs, env),
            solve_side_env(aq, l, &loop_.ivs, env),
        ) {
            let li = access_location_env(ap, &[i0], env);
            let lj = access_location_env(aq, &[j0], env);
            if li == Some(l) && lj == Some(l) {
                let cert = Certificate {
                    level: "M9".into(),
                    loop_hash: loop_.hash(),
                    pair: (p, q),
                    claim: Claim::Exact(vec![j0 - i0]),
                    witness: Witness::WithEnv {
                        env: env.to_vec(),
                        inner: Box::new(Witness::IntSolution { i: vec![i0], j: vec![j0] }),
                    },
                    checker: "m0-check".into(),
                };
                return (Decision::Exact(vec![j0 - i0]), Some(cert));
            }
        }
    }

    (Decision::Unknown("M9: overlap but no witness found".into()), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param_expr::ParamExpr;

    fn iv(name: &str, lo: i64, hi: i64) -> IV {
        IV::new(name, lo, hi, 1)
    }
    fn ivs1(lo: i64, hi: i64) -> Vec<IV> { vec![iv("i", lo, hi)] }

    fn acc_range(kind: AccessKind, r: RangeExpr) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: Some(r),
            index_kind: IndexKind::Affine,
        }
    }

    fn acc_affine(index: Vec<i64>, offset: i64) -> Access {
        Access {
            kind: AccessKind::Read,
            base: "A".into(),
            index,
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn make_loop_1d(body: Vec<Access>) -> Loop {
        Loop { depth: 1, ivs: vec![iv("i", 0, 64)], body, recursion: None, chase: None, assertions: vec![] , scalars: vec![], stmts: None,}
    }

    // ---- range_interval ------------------------------------------------

    #[test]
    fn range_mod_valid() {
        let r = RangeExpr::Mod { iv: 0, modulus: 4 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), Some((0, 3)));
    }
    #[test]
    fn range_mod_invalid() {
        let r = RangeExpr::Mod { iv: 0, modulus: 0 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), None);
    }
    #[test]
    fn range_min_valid() {
        let r = RangeExpr::Min { iv: 0, cap: 32 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), Some((0, 32)));
    }
    #[test]
    #[test]
    fn range_min_saturated() {
        // A cap below the IV window clamps every iteration to the cap: the
        // exact interval is the single value -5, not "unknown".
        let r = RangeExpr::Min { iv: 0, cap: -5 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), Some((-5, -5)));
    }
    #[test]
    fn range_max_valid() {
        let r = RangeExpr::Max { iv: 0, floor: 16 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), Some((16, 63)));
    }
    #[test]
    fn range_max_saturated() {
        // A floor above the IV window clamps every iteration to the floor.
        let r = RangeExpr::Max { iv: 0, floor: 999 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), Some((999, 999)));
    }
    #[test]
    #[test]
    fn range_max_saturated_above() {
        let r = RangeExpr::Max { iv: 0, floor: 9999 };
        assert_eq!(range_interval(&r, &ivs1(0, 64)), Some((9999, 9999)));
    }

    // ---- disjoint / overlap --------------------------------------------

    #[test]
    fn disjoint_yes() { assert!(disjoint((0, 3), (8, 11))); }
    #[test]
    fn disjoint_no()  { assert!(!disjoint((0, 5), (3, 8))); }
    #[test]
    fn overlap_yes()  { assert_eq!(overlap((0, 5), (3, 8)), Some((3, 5))); }
    #[test]
    fn overlap_no()   { assert_eq!(overlap((0, 3), (4, 7)), None); }

    // ---- affine_interval -----------------------------------------------

    #[test]
    fn affine_interval_depth1() {
        let a = acc_affine(vec![2], 3);
        assert_eq!(affine_interval(&a, &ivs1(0, 64)), Some((3, 129)));
    }
    #[test]
    fn affine_interval_depth2() {
        let a = acc_affine(vec![1, 2], 0);
        let ivs = vec![iv("i", 0, 64), iv("j", 0, 64)];
        assert_eq!(affine_interval(&a, &ivs), Some((0, 189)));
    }

    // ---- same-shape -----------------------------------------------------

    #[test]
    fn decide_same_shape_mod() {
        let r = RangeExpr::Mod { iv: 0, modulus: 4 };
        let l = make_loop_1d(vec![
            acc_range(AccessKind::Write, r.clone()),
            acc_range(AccessKind::Read, r),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M2b");
    }
    #[test]
    fn decide_same_shape_min() {
        let r = RangeExpr::Min { iv: 0, cap: 32 };
        let l = make_loop_1d(vec![
            acc_range(AccessKind::Write, r.clone()),
            acc_range(AccessKind::Read, r),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)));
        assert_eq!(cert.unwrap().level, "M2b");
    }
    #[test]
    fn decide_same_shape_max() {
        let r = RangeExpr::Max { iv: 0, floor: 16 };
        let l = make_loop_1d(vec![
            acc_range(AccessKind::Write, r.clone()),
            acc_range(AccessKind::Read, r),
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)));
    }

    // ---- mixed ----------------------------------------------------------

    #[test]
    fn decide_mixed_disjoint() {
        let p_acc = acc_range(AccessKind::Write, RangeExpr::Mod { iv: 0, modulus: 4 });
        let mut q_acc = acc_affine(vec![1], 8);
        q_acc.kind = AccessKind::Read;
        let l = make_loop_1d(vec![p_acc, q_acc]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
        assert_eq!(cert.unwrap().level, "M2b");
    }
    #[test]
    fn decide_mixed_overlap() {
        let p_acc = acc_range(AccessKind::Write, RangeExpr::Mod { iv: 0, modulus: 4 });
        let mut q_acc = acc_affine(vec![1], 0);
        q_acc.kind = AccessKind::Read;
        let l = make_loop_1d(vec![p_acc, q_acc]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)));
        assert_eq!(cert.unwrap().level, "M2b");
    }
    // ================================================================
    // M9 property fuzz: random ModParam/Mod mixes, env values, offsets.
    // Compares decide_param against brute-force enumeration over the
    // IV range with the modulus resolved against env.
    // ================================================================

    fn xorshift9(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn range_loc9(access: &Access, i: i64, env: &[(String, i64)]) -> Option<i64> {
        let off = access.offset.as_const()?;
        if let Some(r) = &access.range {
            let base = match r {
                RangeExpr::Mod { modulus, .. } => {
                    if *modulus <= 0 { return None; }
                    i.rem_euclid(*modulus)
                }
                RangeExpr::ModParam { modulus, .. } => {
                    let m = modulus.eval(env)?;
                    if m <= 0 { return None; }
                    i.rem_euclid(m)
                }
                RangeExpr::Min { cap, .. } => i.min(*cap),
                RangeExpr::Max { floor, .. } => i.max(*floor),
            };
            Some(base + off)
        } else {
            let c = access.index.first().copied().unwrap_or(0);
            Some(c * i + off)
        }
    }

    #[derive(Debug)]
    enum Expected9 { Exact(i64), None, Unknown }

    fn brute9(
        ap: &Access, aq: &Access, lo: i64, hi: i64, env: &[(String, i64)],
    ) -> Expected9 {
        let mut writes = Vec::new();
        for i in lo..hi {
            match range_loc9(ap, i, env) {
                Some(v) => writes.push(v),
                None => return Expected9::Unknown,
            }
        }
        for j in lo..hi {
            match range_loc9(aq, j, env) {
                Some(v) => {
                    if writes.contains(&v) {
                        return Expected9::Exact(0);
                    }
                }
                None => return Expected9::Unknown,
            }
        }
        Expected9::None
    }

    #[test]
    fn fuzz_m9_property() {
        let mut state: u64 = 0x9e3779b97f4a7c15;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut panics = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Random modulus value for env.
            let p_val = (xorshift9(&mut state) % 8) as i64;  // 0..7, includes 0 and 1
            let env = vec![("p".to_string(), p_val)];

            // Write side: random range.
            let wr = match xorshift9(&mut state) % 4 {
                0 => None,
                1 => Some(RangeExpr::Mod { iv: 0, modulus: (xorshift9(&mut state) % 6) as i64 + 1 }),
                2 => Some(RangeExpr::ModParam { iv: 0, modulus: ParamExpr::Param("p".into()) }),
                _ => Some(RangeExpr::Min { iv: 0, cap: (xorshift9(&mut state) % 16) as i64 }),
            };

            // Read side: random range.
            let rr = match xorshift9(&mut state) % 4 {
                0 => None,
                1 => Some(RangeExpr::Mod { iv: 0, modulus: (xorshift9(&mut state) % 6) as i64 + 1 }),
                2 => Some(RangeExpr::ModParam { iv: 0, modulus: ParamExpr::Param("p".into()) }),
                _ => Some(RangeExpr::Max { iv: 0, floor: (xorshift9(&mut state) % 16) as i64 }),
            };

            // Ensure at least one side uses ModParam so decide_param is exercised.
            let uses_p = matches!(wr, Some(RangeExpr::ModParam { .. }))
                      || matches!(rr, Some(RangeExpr::ModParam { .. }));
            if !uses_p { continue; }

            let off_p = (xorshift9(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift9(&mut state) % 5) as i64 - 2;

            let l = Loop {
                depth: 1,
                ivs: vec![IV::new("i", 0, 16, 1)],
                body: vec![
                    Access {
                        kind: AccessKind::Write, base: "A".into(),
                        index: vec![0], elem_size: 8,
                        offset: OffsetValue::Const(off_p),
                        expr: None, indirect: None, guard: None,
                        range: wr.clone(),
                        index_kind: IndexKind::Affine,
                    },
                    Access {
                        kind: AccessKind::Read, base: "A".into(),
                        index: vec![0], elem_size: 8,
                        offset: OffsetValue::Const(off_q),
                        expr: None, indirect: None, guard: None,
                        range: rr.clone(),
                        index_kind: IndexKind::Affine,
                    },
                ],
                recursion: None, chase: None, assertions: vec![],
             scalars: vec![], stmts: None,};

            let expected = brute9(&l.body[0], &l.body[1], 0, 16, &env);

            // Guard against modulus=0 panics.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                decide_param(&l, 0, 1, &env)
            }));
            let (d, _) = match r {
                Ok(x) => x,
                Err(_) => { panics += 1; continue; }
            };
            tested += 1;

            let ok = match (&expected, &d) {
                (Expected9::Unknown, Decision::Unknown(_)) => true,
                (Expected9::Unknown, _) => false,
                (Expected9::None, Decision::None) => true,
                (Expected9::None, Decision::Unknown(_)) => { conservative += 1; true }
                (Expected9::None, _) => false,
                (Expected9::Exact(_), Decision::Exact(_)) => true,
                (Expected9::Exact(_), Decision::Unknown(_)) => { conservative += 1; true }
                (Expected9::Exact(_), _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "wr={:?} rr={:?} off_p={} off_q={} p_val={} expected={:?} got={:?}",
                        wr, rr, off_p, off_q, p_val, expected, d
                    ));
                }
            }
        }

        println!("M9 fuzz: tested={} unsound={} conservative={} panics={}",
                 tested, unsound, conservative, panics);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(panics, 0, "M9 panicked on {} cases", panics);
        assert_eq!(unsound, 0, "M9 fuzz found {} unsound cases", unsound);
    }

    // ---- Exact witness search (replaces the old capped scan) ---------------

    #[test]
    fn exact_witness_beyond_old_scan() {
        // ivs [8000, 100000): write `i mod p` (p = 10000), read `i - 6000`.
        // Overlap of the location intervals is [2000, 9999]. Every common
        // location below 8000 has a residue below `lower` -- the old
        // solve_side rejected those (it only checked the residue itself),
        // and the first location the old code could accept (8000) sits
        // 6000 candidates past the overlap start, beyond its 4096-candidate
        // scan. The exact search must find l = 2000 with i = 12000, j = 8000.
        let wr = RangeExpr::ModParam {
            iv: 0,
            modulus: crate::param_expr::ParamExpr::Param("p".into()),
        };
        let l = Loop {
            depth: 1,
            ivs: vec![iv("i", 8000, 100_000)],
            body: vec![
                acc_range(AccessKind::Write, wr),
                acc_affine(vec![1], -6000),
            ],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        let env = vec![("p".to_string(), 10000i64)];
        let (d, cert) = decide_param(&l, 0, 1, &env);
        assert!(
            matches!(d, Decision::Exact(ref v) if *v == vec![-4000]),
            "expected Exact([-4000]), got {:?}",
            d
        );
        let cert = cert.expect("certificate");
        match cert.witness {
            Witness::WithEnv { inner, .. } => match *inner {
                Witness::IntSolution { i, j } => {
                    assert_eq!(i, vec![12000]);
                    assert_eq!(j, vec![8000]);
                }
                other => panic!("unexpected inner witness {:?}", other),
            },
            other => panic!("unexpected witness {:?}", other),
        }
    }

    #[test]
    fn realized_sets_cover_every_range_kind() {
        // Mod, non-wrapping window.
        let a = acc_range(AccessKind::Write, RangeExpr::Mod { iv: 0, modulus: 100 });
        let r = realized_set(&a, &ivs1(0, 30), None).unwrap();
        assert_eq!(r, Realized::Interval(0, 29));
        // Mod, wrapping window.
        let r = realized_set(&a, &ivs1(90, 110), None).unwrap();
        assert_eq!(r, Realized::TwoIntervals((90, 99), (0, 9)));
        // Mod, window wider than modulus.
        let r = realized_set(&a, &ivs1(0, 250), None).unwrap();
        assert_eq!(r, Realized::Interval(0, 99));
        // Min / Max are contiguous intervals.
        let a = acc_range(AccessKind::Write, RangeExpr::Min { iv: 0, cap: 10 });
        let r = realized_set(&a, &ivs1(0, 30), None).unwrap();
        assert_eq!(r, Realized::Interval(0, 10));
        let a = acc_range(AccessKind::Write, RangeExpr::Max { iv: 0, floor: 10 });
        let r = realized_set(&a, &ivs1(0, 30), None).unwrap();
        assert_eq!(r, Realized::Interval(10, 29));
        // Affine is a progression (both directions).
        let a = acc_affine(vec![3], 5);
        let r = realized_set(&a, &ivs1(0, 4), None).unwrap();
        assert_eq!(r, Realized::Progression { start: 5, step: 3, count: 4 });
        let a = acc_affine(vec![-2], 5);
        let r = realized_set(&a, &ivs1(0, 3), None).unwrap();
        assert_eq!(r, Realized::Progression { start: 1, step: 2, count: 3 });
    }

    #[test]
    fn min_common_unit_cases() {
        let i = |a, b| Realized::Interval(a, b);
        let p = |s, st, c| Realized::Progression { start: s, step: st, count: c };
        assert_eq!(min_common(&i(0, 10), &i(5, 20)), Some(5));
        assert_eq!(min_common(&i(0, 10), &i(11, 20)), None);
        assert_eq!(min_common(&i(0, 10), &p(7, 5, 10)), Some(7));
        assert_eq!(min_common(&i(0, 10), &p(12, 5, 10)), None);
        assert_eq!(min_common(&i(100, 200), &p(7, 5, 100)), Some(102));
        assert_eq!(min_common(&p(0, 4, 10), &p(2, 6, 10)), Some(8));
        assert_eq!(min_common(&p(0, 4, 10), &p(1, 6, 10)), None);
        assert_eq!(
            min_common(&Realized::TwoIntervals((90, 99), (0, 9)), &i(95, 200)),
            Some(95)
        );
    }

    #[test]
    fn fuzz_exact_search_vs_brute() {
        // Random side shapes over random windows (lower is often nonzero, to
        // exercise the residue-below-lower fix). The decision must be sound
        // against brute enumeration, and for these unit-step shapes the exact
        // search must also be *complete*: whenever brute force finds a
        // colliding pair, `decide`/`decide_param` reports Exact.
        let mut state: u64 = 0xe0ac_75ea_c400_0001;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mk = |kind: AccessKind, range: Option<RangeExpr>, off: i64| Access {
            kind,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(off),
            expr: None,
            indirect: None,
            guard: None,
            range,
            index_kind: IndexKind::Affine,
        };
        let mut tested = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();
        for _ in 0..4000 {
            let lo = (rng() % 20) as i64;
            let hi = lo + 1 + (rng() % 40) as i64;
            let wr = match rng() % 4 {
                0 => None,
                1 => Some(RangeExpr::Mod { iv: 0, modulus: (rng() % 12) as i64 + 1 }),
                2 => Some(RangeExpr::ModParam {
                    iv: 0,
                    modulus: crate::param_expr::ParamExpr::Param("p".into()),
                }),
                _ => Some(RangeExpr::Max { iv: 0, floor: (rng() % 30) as i64 }),
            };
            let rr = match rng() % 4 {
                0 => None,
                1 => Some(RangeExpr::Mod { iv: 0, modulus: (rng() % 12) as i64 + 1 }),
                2 => Some(RangeExpr::Min { iv: 0, cap: (rng() % 30) as i64 }),
                _ => Some(RangeExpr::Max { iv: 0, floor: (rng() % 30) as i64 }),
            };
            if wr.is_none() && rr.is_none() {
                continue; // M2b/M9 territory requires at least one range
            }
            let uses_modparam = matches!(wr, Some(RangeExpr::ModParam { .. }))
                || matches!(rr, Some(RangeExpr::ModParam { .. }));
            let off_p = (rng() % 9) as i64 - 4;
            let off_q = (rng() % 9) as i64 - 4;
            let env: Vec<(String, i64)> = vec![("p".into(), (rng() % 12) as i64 + 1)];
            let l = Loop {
                depth: 1,
                ivs: vec![iv("i", lo, hi)],
                body: vec![
                    mk(AccessKind::Write, wr.clone(), off_p),
                    mk(AccessKind::Read, rr.clone(), off_q),
                ],
                recursion: None,
                chase: None,
                assertions: vec![],
             scalars: vec![], stmts: None,};

            // Brute force over the full window (env-aware on both sides).
            let mut brute: Option<(i64, i64)> = None;
            'outer: for i in lo..hi {
                for j in lo..hi {
                    let li = match access_location_env(&l.body[0], &[i], &env) {
                        Some(v) => v,
                        None => continue,
                    };
                    if let Some(lj) = access_location_env(&l.body[1], &[j], &env) {
                        if li == lj {
                            brute = Some((i, j));
                            break 'outer;
                        }
                    }
                }
            }

            let (d, _) = if uses_modparam {
                decide_param(&l, 0, 1, &env)
            } else {
                decide(&l, 0, 1)
            };
            tested += 1;
            match (&d, brute) {
                (Decision::Exact(_), Some(_)) => {}
                (Decision::Exact(_), None) => {
                    panic!(
                        "false Exact: wr={:?} rr={:?} lo={} hi={} offs=({}, {}) env={:?}",
                        wr, rr, lo, hi, off_p, off_q, env
                    );
                }
                (Decision::None, Some(_)) => {
                    panic!("false None: wr={:?} rr={:?} lo={} hi={}", wr, rr, lo, hi);
                }
                (_, Some(_)) => {
                    conservative += 1;
                    if examples.len() < 5 {
                        examples.push(format!(
                            "wr={:?} rr={:?} lo={} hi={} offs=({}, {}) env={:?}",
                            wr, rr, lo, hi, off_p, off_q, env
                        ));
                    }
                }
                (_, None) => {}
            }
        }
        println!(
            "exact search fuzz: tested={} conservative={}",
            tested, conservative
        );
        for ex in &examples {
            println!("  MISSED {}", ex);
        }
        assert_eq!(conservative, 0, "exact search missed {}", conservative);
    }
}
