//! M10 — fixed-point iteration.
//!
//! Handles index expressions of the form
//!     A[x] where x = f^k(iv[0]) with f = next, reached by repeatedly
//!     applying `next` until `next[x] == x` (fixed point).
//! Depth-1 only.
//!
//! Decision procedure:
//!   1. Both accesses have `IndexKind::FixedPoint(_)` (or one has
//!      FixedPoint, the other is Affine).
//!   2. Enumerate the fixed point reached from each IV value in the
//!      domain. Cap at 65536.
//!   3. Find smallest common location between the two accesses.
//!   4. Emit Exact with witness (i, j) or None.

use super::Decision;
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;

const ENUM_CAP: usize = 65536;

/// Enumerate (iv_value, fixed_point_value) pairs for a depth-1 loop.
fn enumerate_fixed_points(
    iv: &IV,
    fpt: &FixedPointTable,
) -> Option<Vec<(i64, i64)>> {
    if iv.step <= 0 || iv.lower >= iv.upper || iv.lower < 0 {
        return None;
    }
    let count = ((iv.upper - iv.lower) as u64 + (iv.step as u64) - 1) / (iv.step as u64);
    if count as usize > ENUM_CAP {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut v = iv.lower;
    while v < iv.upper {
        let fp = fpt.eval(v)?;
        out.push((v, fp));
        v = v.checked_add(iv.step)?;
    }
    Some(out)
}

/// Enumerate (iv, location) pairs for any access, using its IndexKind.
fn enumerate_any(access: &Access, iv: &IV) -> Option<Vec<(i64, i64)>> {
    match &access.index_kind {
        IndexKind::Affine => {
            if iv.step <= 0 || iv.lower >= iv.upper { return None; }
            let count = ((iv.upper - iv.lower) as u64 + (iv.step as u64) - 1) / (iv.step as u64);
            if count as usize > ENUM_CAP { return None; }
            let c = access.index.first().copied().unwrap_or(0);
            let off = access.offset.as_const()?;
            let mut out = Vec::with_capacity(count as usize);
            let mut v = iv.lower;
            while v < iv.upper {
                let loc = c.checked_mul(v)?.checked_add(off)?;
                out.push((v, loc));
                v = v.checked_add(iv.step)?;
            }
            Some(out)
        }
        IndexKind::FixedPoint(fpt) => {
            let off = access.offset.as_const()?;
            let raw = enumerate_fixed_points(iv, fpt)?;
            Some(raw.into_iter().map(|(iv_v, fp)| (iv_v, fp + off)).collect())
        }
        _ => None,
    }
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    let uses_fp = |a: &Access| matches!(&a.index_kind, IndexKind::FixedPoint(_));
    if !uses_fp(ap) && !uses_fp(aq) {
        return (Decision::Unknown("M10: no fixed-point index".into()), None);
    }

    if loop_.depth != 1 {
        return (Decision::Unknown("M10: depth>1 unsupported".into()), None);
    }
    if ap.base != aq.base {
        return (Decision::None, None);
    }

    let iv = &loop_.ivs[0];
    let lp = match enumerate_any(ap, iv) {
        Some(v) => v,
        None => {
            return (
                Decision::Unknown("M10: cannot enumerate for p".into()),
                None,
            )
        }
    };
    let lq = match enumerate_any(aq, iv) {
        Some(v) => v,
        None => {
            return (
                Decision::Unknown("M10: cannot enumerate for q".into()),
                None,
            )
        }
    };

    // Find smallest common location.
    let mut best: Option<(i64, i64, i64)> = None;
    for (iv_p, loc_p) in &lp {
        for (iv_q, loc_q) in &lq {
            if loc_p == loc_q {
                match best {
                    None => best = Some((*loc_p, *iv_p, *iv_q)),
                    Some((cur, _, _)) if *loc_p < cur => {
                        best = Some((*loc_p, *iv_p, *iv_q))
                    }
                    _ => {}
                }
            }
        }
    }

    match best {
        Some((_loc, iv_p, iv_q)) => {
            let d = vec![iv_q - iv_p];
            let cert = Certificate {
                level: "M10".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i: vec![iv_p], j: vec![iv_q] },
                checker: "m0-check".into(),
            };
            (Decision::Exact(d), Some(cert))
        }
        None => {
            let cert = Certificate {
                level: "M10".into(),
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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(name: &str, lo: i64, hi: i64) -> IV {
        IV::new(name, lo, hi, 1)
    }

    /// next = [1, 2, 3, 3] → all paths reach 3
    fn sink3() -> FixedPointTable {
        FixedPointTable { next: vec![1, 2, 3, 3] }
    }

    fn acc_fp(kind: AccessKind, fpt: FixedPointTable, offset: i64) -> Access {
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
            index_kind: IndexKind::FixedPoint(fpt),
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
    fn fp_sink_reached() {
        let f = sink3();
        assert_eq!(f.eval(0), Some(3));
        assert_eq!(f.eval(1), Some(3));
        assert_eq!(f.eval(2), Some(3));
        assert_eq!(f.eval(3), Some(3));
    }

    #[test]
    fn fp_out_of_bounds() {
        let f = sink3();
        assert_eq!(f.eval(4), None);
        assert_eq!(f.eval(-1), None);
    }

    #[test]
    fn fp_cycle_returns_none() {
        // next = [1, 0] → cycle 0↔1, no fixed point
        let f = FixedPointTable { next: vec![1, 0] };
        assert_eq!(f.eval(0), None);
        assert_eq!(f.eval(1), None);
    }

    #[test]
    fn decide_same_fixed_point() {
        // A[x] = A[x] where x is fixed at 3 → EXACT [0]
        let l = make_loop_1d(0, 4, vec![
            acc_fp(AccessKind::Write, sink3(), 0),
            acc_fp(AccessKind::Read,  sink3(), 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M10");
    }

    #[test]
    fn decide_fixed_point_offset_shift() {
        // A[x] = A[x + 1]: both start at 0 → x=3. So p at 3, q at 4.
        // But q's location is 4, p's is 3 → no overlap in the enumerable domain.
        let l = make_loop_1d(0, 4, vec![
            acc_fp(AccessKind::Write, sink3(), 0),
            acc_fp(AccessKind::Read,  sink3(), 1),
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
    }

    #[test]
    fn decide_fixed_point_vs_affine_disjoint() {
        // A[x] where x=3 vs A[100 + i] → {3} vs {100, 101, 102, 103} disjoint
        let l = make_loop_1d(0, 4, vec![
            acc_fp(AccessKind::Write, sink3(), 0),
            {
                let mut a = acc_fp(AccessKind::Read, sink3(), 0);
                a.index_kind = IndexKind::Affine;
                a.index = vec![1];
                a.offset = OffsetValue::Const(100);
                a
            },
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
    }

    #[test]
    fn decide_too_large_returns_unknown() {
        let l = make_loop_1d(0, 200_000, vec![
            acc_fp(AccessKind::Write, sink3(), 0),
            acc_fp(AccessKind::Read,  sink3(), 0),
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }
    // ================================================================
    // Property fuzz: random fixed-point tables, compare decide()
    // against brute-force iteration with cycle detection.
    // ================================================================

    fn xorshift10(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn fixed_point10(next: &[i64], start: i64, cap: usize) -> Option<i64> {
        let mut seen = std::collections::HashSet::new();
        let mut x = start;
        for _ in 0..cap {
            if x < 0 || (x as usize) >= next.len() { return None; }
            if x == next[x as usize] { return Some(x); }
            if !seen.insert(x) { return None; }
            x = next[x as usize];
        }
        None
    }

    #[test]
    fn fuzz_m10_property() {
        let mut state: u64 = 0x10_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..2000 {
            // Random small tables with mixed structure.
            let n = (xorshift10(&mut state) % 5) as usize + 1;
            let next_p: Vec<i64> = (0..n).map(|_| (xorshift10(&mut state) % (n as u64 + 2)) as i64 - 1).collect();
            let next_q: Vec<i64> = if xorshift10(&mut state) % 2 == 0 {
                next_p.clone()
            } else {
                (0..n).map(|_| (xorshift10(&mut state) % (n as u64 + 2)) as i64 - 1).collect()
            };
            let off_p = (xorshift10(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift10(&mut state) % 5) as i64 - 2;

            let lo = 0i64;
            let hi = 8i64;

            // Ground truth: iterate fixed_point10 for each IV.
            let mut expected = Some(false);
            let mut wp = std::collections::HashSet::new();
            for i in lo..hi {
                match fixed_point10(&next_p, i, 32) {
                    Some(v) => { wp.insert(v + off_p); }
                    None => { expected = None; break; }
                }
            }
            if let Some(_) = expected {
                let mut found = false;
                for j in lo..hi {
                    match fixed_point10(&next_q, j, 32) {
                        Some(v) => { if wp.contains(&(v + off_q)) { found = true; break; } }
                        None => { expected = None; break; }
                    }
                }
                if let Some(b) = expected { expected = Some(found); }
            }

            let l = make_loop_1d(lo, hi, vec![
                acc_fp(AccessKind::Write, FixedPointTable { next: next_p.clone() }, off_p),
                acc_fp(AccessKind::Read,  FixedPointTable { next: next_q.clone() }, off_q),
            ]);

            let (d, _) = decide(&l, 0, 1);
            tested += 1;
            let ok = match (expected, &d) {
                (None, Decision::Unknown(_)) => true,
                (None, _) => false,
                (Some(true), Decision::Exact(_)) => true,
                (Some(true), Decision::Unknown(_)) => { conservative += 1; true }
                (Some(true), _) => false,
                (Some(false), Decision::None) => true,
                (Some(false), Decision::Unknown(_)) => { conservative += 1; true }
                (Some(false), _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "next_p={:?} next_q={:?} op={} oq={} expected={:?} got={:?}",
                        next_p, next_q, off_p, off_q, expected, d
                    ));
                }
            }
        }

        println!("M10 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M10 fuzz found {} unsound", unsound);
    }

}
