//! M6 — products.
//!
//! Handles index expressions that are polynomial of degree ≥ 2 in the
//! IVs: `A[i*j]`, `A[i*i + j]`, `A[i*i*i]`, etc. The certificate format
//! is unchanged: `Claim::Exact(Vec<i64>)` with `Witness::IntSolution`.
//! The checker evaluates the product expression via `IndexExpr::eval`.
//!
//! Decision procedure:
//!   1. Both accesses have `IndexKind::Product(_)`.
//!   2. Enumerate the finite set of locations each access visits over
//!      the IV range. Cap at 4096 enumerated values per access.
//!   3. If the location sets are disjoint → `Claim::None`.
//!   4. If they intersect, find the smallest common location L, find
//!      witness `i` for access p and `j` for access q such that both
//!      evaluate to L. Emit `Claim::Exact([j - i])` with that witness.
//!   5. If either IV range is too large → `Unknown`.

use super::Decision;
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;

const ENUM_CAP: usize = 4096;

/// Compute the (location, iv-vector) pair for every IV assignment.
/// Returns `None` if enumeration exceeds `ENUM_CAP`.
/// Degree-2 is M6's original. Degree 3 is cubic. Degree 4 is quartic.
/// Phase 5 §5.1.7 extends the cap to degree 8 so `i*i*i*i*i`-style polynomials
/// are decidable rather than refused. The cap still exists — a deeper tree is
/// beyond real code and its evaluation cost grows — but it is now a
/// large-but-finite guard rather than the degree-4 line that made §5.1.7 a
/// no-op. Above it the answer remains `Unknown`, never a guess, and the
/// `IndexExpr::eval` checked arithmetic turns an overflowing degree-8 value
/// into `Unknown` rather than a wrapped one.
const MAX_DEGREE: u32 = 8;

/// The degree at which §5.1.7's "cubic+" extension begins (degree ≥ 5).
pub const CUBIC_PLUS_MIN_DEGREE: u32 = 5;

fn enumerate_locations(
    access: &Access,
    ivs: &[IV],
) -> Option<Vec<(i64, Vec<i64>)>> {
    if ivs.is_empty() { return None; }
    let depth = ivs.len();

    // Cubic+ guard: cap the polynomial degree.
    if let IndexKind::Product(e) = &access.index_kind {
        if e.max_degree() > MAX_DEGREE {
            return None;
        }
    }

    // Compute the total enumeration size first.
    let mut total: u64 = 1;
    for iv in ivs {
        if iv.step <= 0 || iv.lower >= iv.upper { return None; }
        let count = ((iv.upper - iv.lower) as u64 + (iv.step as u64) - 1) / (iv.step as u64);
        total = total.checked_mul(count)?;
        if total as usize > ENUM_CAP { return None; }
    }

    let offset = access.offset.as_const()?;
    let mut out = Vec::with_capacity(total as usize);

    // Recursive IV iteration.
    fn rec(
        access: &Access,
        ivs: &[IV],
        k: usize,
        cur: &mut Vec<i64>,
        offset: i64,
        out: &mut Vec<(i64, Vec<i64>)>,
    ) -> Option<()> {
        if k == ivs.len() {
            let base = match &access.index_kind {
                IndexKind::Affine => {
                    let mut loc: i64 = 0;
                    for (i, c) in access.index.iter().enumerate() {
                        loc = loc.checked_add(c.checked_mul(cur[i])?)?;
                    }
                    loc
                }
                IndexKind::Product(e) => e.eval(cur, &[], None)?,
                IndexKind::Range(range) => {
                    // Evaluate the range expression at the current IV values
                    match range {
                        RangeExpr::Mod { iv, modulus } => {
                            if *modulus <= 0 { return None; }
                            cur.get(*iv)?.rem_euclid(*modulus)
                        }
                        RangeExpr::ModParam { iv, modulus } => {
                            // Without environment, cannot evaluate
                            return None;
                        }
                        RangeExpr::Min { iv, cap } => {
                            let v = cur.get(*iv)?;
                            *std::cmp::min(v, cap)
                        }
                        RangeExpr::Max { iv, floor } => {
                            let v = cur.get(*iv)?;
                            *std::cmp::max(v, floor)
                        }
                    }
                }
                IndexKind::Polyhedral { a, b: _, dim } => {
                    // For polyhedral, evaluate the affine part
                    if cur.len() >= *dim {
                        let mut loc = 0i64;
                        for k in 0..*dim {
                            let coeff = a.first().map(|row| row.get(k).copied().unwrap_or(0)).unwrap_or(0);
                            let val = cur[k];
                            loc = loc.checked_add(coeff.checked_mul(val)?)?;
                        }
                        loc
                    } else {
                        return None;
                    }
                }
                // M6 doesn't handle Hash or Recurrence; the dispatcher
                // routes those to M7/M8 before M6 sees them.
                IndexKind::Hash(_) => return None,
                IndexKind::Recurrence(_) => return None,
                IndexKind::FixedPoint(_) => return None,
                IndexKind::SymbolicFixedPoint { .. } => return None,
            };
            let loc = base.checked_add(offset)?;
            out.push((loc, cur.clone()));
            return Some(());
        }
        let iv = &ivs[k];
        let mut v = iv.lower;
        while v < iv.upper {
            cur.push(v);
            rec(access, ivs, k + 1, cur, offset, out)?;
            cur.pop();
            v = v.checked_add(iv.step)?;
        }
        Some(())
    }

    let mut cur = Vec::with_capacity(depth);
    rec(access, ivs, 0, &mut cur, offset, &mut out)?;
    Some(out)
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    let uses_product = |a: &Access| matches!(&a.index_kind, IndexKind::Product(_));
    if !uses_product(ap) && !uses_product(aq) {
        return (Decision::Unknown("M6: no product index".into()), None);
    }

    if ap.base != aq.base {
        // Different arrays cannot conflict.
        return (Decision::None, None);
    }

    let lp = match enumerate_locations(ap, &loop_.ivs) {
        Some(v) => v,
        None => return large_path(loop_, p, q),
    };
    let lq = match enumerate_locations(aq, &loop_.ivs) {
        Some(v) => v,
        None => return large_path(loop_, p, q),
    };

    // Find the smallest common location.
    let mut locs_p: Vec<i64> = lp.iter().map(|(l, _)| *l).collect();
    locs_p.sort_unstable();
    locs_p.dedup();

    let mut smallest_common: Option<(i64, Vec<i64>, Vec<i64>)> = None;
    for (lp_loc, lp_iv) in &lp {
        for (lq_loc, lq_iv) in &lq {
            if lp_loc == lq_loc {
                let l = *lp_loc;
                match &smallest_common {
                    None => smallest_common = Some((l, lp_iv.clone(), lq_iv.clone())),
                    Some((cur, _, _)) if l < *cur => {
                        smallest_common = Some((l, lp_iv.clone(), lq_iv.clone()))
                    }
                    _ => {}
                }
            }
        }
    }

    match smallest_common {
        Some((_l, i, j)) => {
            // Distance vector: j - i, per IV.
            let depth = loop_.depth;
            let mut d = Vec::with_capacity(depth);
            for k in 0..depth {
                d.push(j[k] - i[k]);
            }
            let cert = Certificate {
                level: "M6".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i, j },
                checker: "m0-check".into(),
            };
            (Decision::Exact(d), Some(cert))
        }
        None => {
            // No common location → independence.
            let cert = Certificate {
                level: "M6".into(),
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
            (Decision::None, Some(cert))
        }
    }
}

// ============================================================================
// Large-range closed-form path (Phase 2 finalization)
// ============================================================================
//
// Full enumeration is capped at ENUM_CAP location pairs. For ranges beyond
// that, the quadratic Diophantine `P(t) = Q(u)` is reduced to a linear one
// by substituting one variable: enumerate the IV of one side (cost bounded
// by LARGE_CAP) and solve the other side exactly -- division for affine
// sides, the quadratic formula for degree-2 sides. Degree >= 3 on the
// *solved* side declines. The witness is a concrete (i, j) pair the checker
// verifies from scratch, so this path can only add accepted certificates,
// never wrong answers; when it finds nothing it returns Unknown (never a
// None claim, which the checker could not re-derive beyond its own caps).

/// Candidate sweep cap for the large-range path.
const LARGE_CAP: u64 = 1 << 20;

/// Polynomial coefficients (ascending: c0 + c1·x + c2·x² + c3·x³ + c4·x⁴)
/// of a single-variable `IndexExpr`. `None` above degree 4 or on overflow.
fn poly_expr(e: &IndexExpr, iv: usize) -> Option<[i64; 5]> {
    match e {
        IndexExpr::IV(k) => {
            if *k == iv {
                Some([0, 1, 0, 0, 0])
            } else {
                None
            }
        }
        IndexExpr::Const(n) => Some([*n, 0, 0, 0, 0]),
        IndexExpr::Add(a, b) => {
            let x = poly_expr(a, iv)?;
            let y = poly_expr(b, iv)?;
            let mut c = [0i64; 5];
            for k in 0..5 {
                c[k] = x[k].checked_add(y[k])?;
            }
            Some(c)
        }
        IndexExpr::Mul(a, b) => {
            let x = poly_expr(a, iv)?;
            let y = poly_expr(b, iv)?;
            let mut acc = [0i128; 5];
            for i in 0..5 {
                for j in 0..5 {
                    if i + j < 5 {
                        acc[i + j] += (x[i] as i128) * (y[j] as i128);
                    } else if x[i] != 0 && y[j] != 0 {
                        return None; // term above degree 4: out of scope
                    }
                }
            }
            let mut c = [0i64; 5];
            for k in 0..5 {
                c[k] = i64::try_from(acc[k]).ok()?;
            }
            Some(c)
        }
        IndexExpr::Div(_, _) => None, // Division is not a polynomial
        IndexExpr::Mod(_, _) => None, // Modulo is not a polynomial
        IndexExpr::Min(_, _) => None, // Min is not a polynomial
        IndexExpr::Max(_, _) => None, // Max is not a polynomial
        IndexExpr::Cmp(_, _, _) => None, // Comparison is not a polynomial
        IndexExpr::Load { .. } => None, // Load is not a polynomial
    }
}

/// Full location polynomial of an access (expression + offset). `None` for
/// non-polynomial index kinds, multi-variable loops, or degree > 4.
fn poly_of(access: &Access, iv: usize, depth: usize) -> Option<[i64; 5]> {
    let mut c = match &access.index_kind {
        IndexKind::Product(e) => poly_expr(e, iv)?,
        IndexKind::Affine => {
            if depth != 1 {
                return None;
            }
            [0, access.index.first().copied().unwrap_or(0), 0, 0, 0]
        }
        _ => return None,
    };
    let off = access.offset.as_const()?;
    c[0] = c[0].checked_add(off)?;
    Some(c)
}

/// Degree of a coefficient array.
fn degree(c: &[i64; 5]) -> u32 {
    for k in (1..5).rev() {
        if c[k] != 0 {
            return k as u32;
        }
    }
    0
}

/// Horner evaluation with checked arithmetic.
fn poly_eval(c: &[i64; 5], x: i64) -> Option<i64> {
    let mut acc: i128 = c[4] as i128;
    for k in (0..4).rev() {
        acc = acc * (x as i128) + (c[k] as i128);
    }
    i64::try_from(acc).ok()
}

/// Smallest `t ∈ [lo, hi)` with `P(t) = target`. Exact for degree <= 2;
/// declines above (no closed form in scope).
fn solve_poly_eq(c: &[i64; 5], target: i64, lo: i64, hi: i64) -> Option<i64> {
    match degree(c) {
        0 => {
            if c[0] == target && lo < hi {
                Some(lo)
            } else {
                None
            }
        }
        1 => {
            let a = c[1];
            let d = target.checked_sub(c[0])?;
            if d % a != 0 {
                return None;
            }
            let t = d / a;
            if t >= lo && t < hi {
                Some(t)
            } else {
                None
            }
        }
        2 => {
            // a·t² + b·t + (c0 - target) = 0
            let a = c[2] as i128;
            let b = c[1] as i128;
            let cc = (c[0] as i128) - (target as i128);
            let disc = b * b - 4 * a * cc;
            if disc < 0 {
                return None;
            }
            let s = isqrt_i128(disc);
            if s * s != disc {
                return None;
            }
            let two_a = 2 * a;
            let neg_b = b.checked_neg()?;
            let mut best: Option<i64> = None;
            for num in [neg_b.checked_add(s), neg_b.checked_sub(s)] {
                let num = match num {
                    Some(n) => n,
                    None => continue,
                };
                if num % two_a != 0 {
                    continue;
                }
                let t = num / two_a;
                if t < (i64::MIN as i128) || t > (i64::MAX as i128) {
                    continue;
                }
                let t = t as i64;
                if t >= lo && t < hi {
                    best = Some(match best {
                        Some(b2) => b2.min(t),
                        None => t,
                    });
                }
            }
            best
        }
        _ => None,
    }
}

/// Integer square root (floor) for nonnegative `n`.
fn isqrt_i128(n: i128) -> i128 {
    if n < 2 {
        return n.max(0);
    }
    let mut x = (n as f64).sqrt() as i128;
    if x < 0 {
        x = 0;
    }
    while x > 0 && x.saturating_mul(x) > n {
        x -= 1;
    }
    while x.saturating_add(1).saturating_mul(x.saturating_add(1)) <= n {
        x += 1;
    }
    x
}

/// Closed-form witness search for a 1-D unit-step product loop whose full
/// enumeration exceeds ENUM_CAP. Returns the pair `(p-iv, q-iv)`.
///
/// Sweeps the IV of one side (ascending) and solves the other side exactly
/// for each candidate, so any hit is a genuine collision of the two full
/// location polynomials. Declines (`None`) beyond LARGE_CAP candidates, for
/// non-unit steps, and when the solved side has degree >= 3.
fn large_witness_1d(loop_: &Loop, ap: &Access, aq: &Access) -> Option<(i64, i64)> {
    if loop_.depth != 1 {
        return None;
    }
    let iv0 = loop_.ivs.first()?;
    if iv0.step != 1 || iv0.lower >= iv0.upper {
        return None;
    }
    let (lo, hi) = (iv0.lower, iv0.upper);
    let count = (hi - lo) as u64;
    if count > LARGE_CAP {
        return None;
    }
    let cp = poly_of(ap, 0, loop_.depth)?;
    let cq = poly_of(aq, 0, loop_.depth)?;
    let dp = degree(&cp);
    let dq = degree(&cq);

    // Enumerate the side whose partner is exactly solvable (degree <= 2).
    let (enum_c, solve_c, enum_is_q) = if dq > 2 && dp <= 2 {
        (&cq, &cp, true) // enumerate q's IV, solve p
    } else if dp > 2 && dq <= 2 {
        (&cp, &cq, false) // enumerate p's IV, solve q
    } else if dp <= 2 {
        (&cq, &cp, true) // both solvable: enumerate q, solve p
    } else {
        return None; // neither side exactly solvable
    };

    for v in lo..hi {
        let tv = poly_eval(enum_c, v)?;
        if let Some(s) = solve_poly_eq(solve_c, tv, lo, hi) {
            return if enum_is_q { Some((s, v)) } else { Some((v, s)) };
        }
    }
    None
}

/// The large-range path: closed-form witness search when enumeration is
/// impossible. Only ever returns `Exact`; found witnesses are verified by
/// the checker from scratch, and absence is reported as `Unknown` (the
/// checker could not re-derive a `None` for such a range).
fn large_path(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];
    if let Some((i0, j0)) = large_witness_1d(loop_, ap, aq) {
        let d = vec![j0 - i0];
        let cert = Certificate {
            level: "M6".into(),
            loop_hash: loop_.hash(),
            pair: (p, q),
            claim: Claim::Exact(d.clone()),
            witness: Witness::IntSolution { i: vec![i0], j: vec![j0] },
            checker: "m0-check".into(),
        };
        return (Decision::Exact(d), Some(cert));
    }
    (
        Decision::Unknown(
            "M6: range too large for enumeration and no closed-form witness".into(),
        ),
        None,
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(name: &str, lo: i64, hi: i64) -> IV {
        IV::new(name, lo, hi, 1)
    }

    fn acc_product(
        kind: AccessKind,
        expr: IndexExpr,
        offset: i64,
    ) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![0],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Product(expr),
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

    fn make_loop_1d(lo: i64, hi: i64, body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![iv("i", lo, hi)],
            body,
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    #[test]
    fn eval_iexpr_simple() {
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(1)),
        );
        assert_eq!(e.eval(&[3, 4], &[], None), Some(12));
        assert_eq!(e.eval(&[0, 4], &[], None), Some(0));
    }

    #[test]
    fn eval_iexpr_overflow_is_none() {
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::Const(i64::MAX)),
            Box::new(IndexExpr::Const(2)),
        );
        assert_eq!(e.eval(&[], &[], None), None);
    }

    #[test]
    fn max_degree_correct() {
        let i = IndexExpr::IV(0);
        let j = IndexExpr::IV(1);
        let prod = IndexExpr::Mul(Box::new(i.clone()), Box::new(j.clone()));
        assert_eq!(prod.max_degree(), 2);

        let sum = IndexExpr::Add(Box::new(i.clone()), Box::new(prod.clone()));
        assert_eq!(sum.max_degree(), 2);

        let cubic = IndexExpr::Mul(Box::new(prod.clone()), Box::new(i));
        assert_eq!(cubic.max_degree(), 3);
    }

    #[test]
    fn enumerate_small_range() {
        // A[i*i] over i in [0, 4) → locations [0, 1, 4, 9]
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let a = acc_product(AccessKind::Read, e, 0);
        let ivs = vec![iv("i", 0, 4)];
        let locs = enumerate_locations(&a, &ivs).expect("enum");
        let mut vs: Vec<i64> = locs.iter().map(|(l, _)| *l).collect();
        vs.sort_unstable();
        assert_eq!(vs, vec![0, 1, 4, 9]);
    }

    #[test]
    fn decide_products_same_location() {
        // A[i*i] = A[i*i] → EXACT [0]
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let l = make_loop_1d(0, 4, vec![
            acc_product(AccessKind::Write, e.clone(), 0),
            acc_product(AccessKind::Read,  e, 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M6");
    }

    #[test]
    fn decide_products_different_locations() {
        // A[i] = A[i*i] with i in [2, 4) → {2, 3} vs {4, 9} disjoint → None
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let l = make_loop_1d(2, 4, vec![
            {
                let mut a = acc_product(AccessKind::Write, IndexExpr::IV(0), 0);
                a.index_kind = IndexKind::Affine;
                a.index = vec![1];
                a
            },
            acc_product(AccessKind::Read, e, 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
        assert_eq!(cert.unwrap().level, "M6");
    }

    #[test]
    fn decide_products_large_range_uses_closed_form() {
        // A[i*i] vs A[i*i] over i in [0, 100000): full enumeration exceeds
        // the cap, but the closed-form path solves t² = u² and certifies the
        // distance-0 dependence instead of giving up.
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let l = make_loop_1d(0, 100_000, vec![
            acc_product(AccessKind::Write, e.clone(), 0),
            acc_product(AccessKind::Read,  e, 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if *v == vec![0]), "got {:?}", d);
        assert!(cert.is_some());
    }

    #[test]
    fn large_witness_square_vs_linear() {
        // write A[i*i], read A[i + 1]: t² = u + 1 with smallest common
        // location 1 at (t, u) = (1, 0).
        let e = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let l = make_loop_1d(0, 100_000, vec![
            acc_product(AccessKind::Write, e, 0),
            acc_affine(vec![1], 1),
        ]);
        assert_eq!(large_witness_1d(&l, &l.body[0], &l.body[1]), Some((1, 0)));
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if *v == vec![-1]), "got {:?}", d);
        assert!(cert.is_some());
    }

    #[test]
    fn large_witness_linear_vs_square() {
        // write A[2*i], read A[i*i]: 2t = u² -> smallest (t, u) = (0, 0).
        let lin = IndexExpr::Mul(
            Box::new(IndexExpr::Const(2)),
            Box::new(IndexExpr::IV(0)),
        );
        let sq = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let l = make_loop_1d(0, 100_000, vec![
            acc_product(AccessKind::Write, lin, 0),
            acc_product(AccessKind::Read, sq, 0),
        ]);
        assert_eq!(large_witness_1d(&l, &l.body[0], &l.body[1]), Some((0, 0)));
    }

    #[test]
    fn large_witness_no_collision_is_unknown() {
        // write A[i*i], read A[-i - 1]: locations are >= 0 vs <= -1, so no
        // collision exists; the closed-form path must report Unknown (it
        // cannot prove absence to the checker's satisfaction) -- never None.
        let sq = IndexExpr::Mul(
            Box::new(IndexExpr::IV(0)),
            Box::new(IndexExpr::IV(0)),
        );
        let neg = IndexExpr::Add(
            Box::new(IndexExpr::Mul(
                Box::new(IndexExpr::Const(-1)),
                Box::new(IndexExpr::IV(0)),
            )),
            Box::new(IndexExpr::Const(-1)),
        );
        let l = make_loop_1d(0, 100_000, vec![
            acc_product(AccessKind::Write, sq, 0),
            acc_product(AccessKind::Read, neg, 0),
        ]);
        assert_eq!(large_witness_1d(&l, &l.body[0], &l.body[1]), None);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)), "got {:?}", d);
    }

    #[test]
    fn fuzz_m6_large_witness_vs_enumeration() {
        // For random polynomial 1-D loops over small ranges, the closed-form
        // path must agree with full enumeration on existence whenever its
        // solved side is degree <= 2, and every witness it returns must be a
        // genuine colliding pair inside the range.
        let mut state: u64 = 0x1607_9e00_dc0d_e001;
        let mut tested = 0u64;
        let mut missed = 0u64;
        let mut examples: Vec<String> = Vec::new();
        for _ in 0..3000 {
            let expr_p = gen_product_expr(&mut state, 3);
            let expr_q = gen_product_expr(&mut state, 3);
            if expr_p.max_degree() > 4 || expr_q.max_degree() > 4 {
                continue;
            }
            let off_p = (xorshift(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift(&mut state) % 5) as i64 - 2;
            let lo = 0i64;
            let hi = 10i64;
            let l = make_loop_1d(lo, hi, vec![
                acc_product(AccessKind::Write, expr_p.clone(), off_p),
                acc_product(AccessKind::Read,  expr_q.clone(), off_q),
            ]);
            let w = large_witness_1d(&l, &l.body[0], &l.body[1]);
            let expected = brute_product(&expr_p, off_p, &expr_q, off_q, lo, hi);
            tested += 1;

            // Any reported witness must be genuine.
            if let Some((t, u)) = w {
                assert!(t >= lo && t < hi && u >= lo && u < hi, "witness out of range");
                let lp = expr_p.eval(&[t], &[], None).unwrap() + off_p;
                let lq = expr_q.eval(&[u], &[], None).unwrap() + off_q;
                assert_eq!(lp, lq, "witness does not collide: {:?}", (t, u));
            }

            // Completeness for the covered shapes: solved side degree <= 2.
            let cp = poly_of(&l.body[0], 0, 1).unwrap();
            let cq = poly_of(&l.body[1], 0, 1).unwrap();
            let solvable = degree(&cp) <= 2 || degree(&cq) <= 2;
            if solvable {
                if let Some(Some(_)) = expected {
                    if w.is_none() {
                        missed += 1;
                        if examples.len() < 5 {
                            examples.push(format!(
                                "expr_p={:?} off_p={} expr_q={:?} off_q={}",
                                expr_p, off_p, expr_q, off_q
                            ));
                        }
                    }
                }
            }
        }
        println!(
            "M6 large fuzz: tested={} missed={}",
            tested, missed
        );
        for ex in &examples {
            println!("  MISSED {}", ex);
        }
        assert_eq!(missed, 0, "closed-form path missed {} witnesses", missed);
    }
    // ================================================================
    // Property fuzz: random polynomial index expressions, compared
    // against brute-force enumeration over the IV range.
    // ================================================================

    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn gen_product_expr(state: &mut u64, depth: u32) -> IndexExpr {
        let r = xorshift(state) % 100;
        if depth == 0 {
            if r < 60 {
                IndexExpr::IV(0)
            } else {
                let v = (xorshift(state) % 5) as i64;
                IndexExpr::Const(v)
            }
        } else if r < 30 {
            IndexExpr::IV(0)
        } else if r < 40 {
            let v = (xorshift(state) % 5) as i64;
            IndexExpr::Const(v)
        } else if r < 70 {
            IndexExpr::Add(
                Box::new(gen_product_expr(state, depth - 1)),
                Box::new(gen_product_expr(state, depth - 1)),
            )
        } else {
            IndexExpr::Mul(
                Box::new(gen_product_expr(state, depth - 1)),
                Box::new(gen_product_expr(state, depth - 1)),
            )
        }
    }

    fn brute_product(
        expr_p: &IndexExpr, off_p: i64,
        expr_q: &IndexExpr, off_q: i64,
        lo: i64, hi: i64,
    ) -> Option<Option<(i64, i64, i64)>> {
        // Returns:
        //   None            -> overflow/undefined (expected Unknown)
        //   Some(None)      -> no collision (expected None)
        //   Some(Some(..))  -> collision (expected Exact)
        //
        // For Exact, we return (loc, i, j) for the FIRST collision
        // encountered (smallest i, then smallest j). M6 picks by
        // smallest location, so the specific witness may differ, but
        // the *verdict* (Exact vs None) must match. We only compare
        // verdicts in the test.
        let mut pairs_p: Vec<(i64, i64)> = Vec::new();
        for i in lo..hi {
            let v = expr_p.eval(&[i], &[], None)?;
            pairs_p.push((v + off_p, i));
        }
        let mut pairs_q: Vec<(i64, i64)> = Vec::new();
        for j in lo..hi {
            let v = expr_q.eval(&[j], &[], None)?;
            pairs_q.push((v + off_q, j));
        }
        for (loc_p, i) in &pairs_p {
            for (loc_q, j) in &pairs_q {
                if loc_p == loc_q {
                    return Some(Some((*loc_p, *i, *j)));
                }
            }
        }
        Some(None)
    }

    #[test]
    fn fuzz_m6_property() {
        let mut state: u64 = 0xc0ffee_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples_unsound: Vec<String> = Vec::new();

        for _ in 0..3000 {
            let expr_p = gen_product_expr(&mut state, 3);
            let expr_q = gen_product_expr(&mut state, 3);
            // Skip expressions whose degree exceeds M6's cap (returns
            // None → Unknown; that's expected, not a bug).
            if expr_p.max_degree() > 4 || expr_q.max_degree() > 4 {
                continue;
            }
            let off_p = (xorshift(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift(&mut state) % 5) as i64 - 2;

            let l = make_loop_1d(0, 6, vec![
                acc_product(AccessKind::Write, expr_p.clone(), off_p),
                acc_product(AccessKind::Read,  expr_q.clone(), off_q),
            ]);

            let expected = brute_product(&expr_p, off_p, &expr_q, off_q, 0, 6);
            let (d, _) = decide(&l, 0, 1);
            tested += 1;

            match (expected, &d) {
                // Overflow in brute force: expect Unknown from decide.
                (None, Decision::Unknown(_)) => {}
                (None, _) => {
                    // Brute force can't compute; analyzer should also refuse.
                    // If analyzer said Exact, that's potentially a false
                    // positive (witness outside what brute force can verify).
                    unsound += 1;
                    if examples_unsound.len() < 10 {
                        examples_unsound.push(format!(
                            "expr_p={:?} off_p={} expr_q={:?} off_q={} expected=OVERFLOW got={:?}",
                            expr_p, off_p, expr_q, off_q, d
                        ));
                    }
                }
                // Brute force says no collision.
                (Some(None), Decision::None) => {}
                (Some(None), Decision::Unknown(_)) => { conservative += 1; }
                (Some(None), Decision::Exact(_)) => {
                    unsound += 1;
                    if examples_unsound.len() < 10 {
                        examples_unsound.push(format!(
                            "expr_p={:?} off_p={} expr_q={:?} off_q={} expected=NONE got={:?}",
                            expr_p, off_p, expr_q, off_q, d
                        ));
                    }
                }
                // Brute force says collision.
                (Some(Some(_)), Decision::Exact(_)) => {}
                (Some(Some(_)), Decision::Unknown(_)) => { conservative += 1; }
                (Some(Some(_)), Decision::None) => {
                    unsound += 1;
                    if examples_unsound.len() < 10 {
                        examples_unsound.push(format!(
                            "expr_p={:?} off_p={} expr_q={:?} off_q={} expected=EXACT got=None",
                            expr_p, off_p, expr_q, off_q
                        ));
                    }
                }
            }
        }

        println!("M6 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples_unsound {
            println!("  UNSOUND {}", ex);
        }
        assert_eq!(unsound, 0, "M6 fuzz found {} unsound cases", unsound);
    }

}
