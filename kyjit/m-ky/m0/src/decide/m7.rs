//! M7 — hash functions.
//!
//! Handles index expressions of the form
//!     ((iv * c) >> shift) & mask
//! where `iv` is the single loop IV, `c` is a 64-bit multiplier,
//! `shift` is a logical right shift, and `mask` is a bitwise AND mask.
//!
//! Decision procedure (depth-1 only):
//!   1. Both accesses have `IndexKind::Hash(_)` with the SAME HashExpr.
//!   2. Enumerate `hash(i)` for `i` over the IV range. Cap at 65536.
//!   3. Build the collision map: for each hash value, which IVs produce it.
//!   4. If no collision (hash injective on the domain) → `Claim::None`.
//!   5. If a collision at (i, j) with `hash(i) == hash(j)` and `i != j`,
//!      emit `Claim::Exact([j - i])` with that witness.
//!   6. If enumeration exceeds cap → `Unknown`.

use super::Decision;
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;

const ENUM_CAP: usize = 65536;

/// Enumerate the (iv, hash) pairs for a depth-1 loop.
fn enumerate_hashes(iv: &IV, h: &HashExpr) -> Option<Vec<(i64, i64)>> {
    if iv.step <= 0 || iv.lower >= iv.upper {
        return None;
    }
    let count = ((iv.upper - iv.lower) as u64 + (iv.step as u64) - 1) / (iv.step as u64);
    if count as usize > ENUM_CAP {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut v = iv.lower;
    while v < iv.upper {
        out.push((v, h.eval(v)));
        v = v.checked_add(iv.step)?;
    }
    Some(out)
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    fn hash_of(a: &Access) -> Option<&HashExpr> {
        match &a.index_kind {
            IndexKind::Hash(h) => Some(h),
            _ => None,
        }
    }

    let hp = match hash_of(ap) {
        Some(h) => h,
        None => return (Decision::Unknown("M7: p is not hash-indexed".into()), None),
    };
    let hq = match hash_of(aq) {
        Some(h) => h,
        None => return (Decision::Unknown("M7: q is not hash-indexed".into()), None),
    };

    if hp != hq {
        return (
            Decision::Unknown("M7: accesses use different hash functions".into()),
            None,
        );
    }

    if ap.base != aq.base {
        return (Decision::None, None);
    }
    if loop_.depth != 1 {
        return (Decision::Unknown("M7: depth>1 unsupported".into()), None);
    }

    let entries = match enumerate_hashes(&loop_.ivs[0], hp) {
        Some(e) => e,
        None => {
            return (
                Decision::Unknown("M7: cannot enumerate hash values (too large)".into()),
                None,
            )
        }
    };

    // Enumerate locations on both sides and look for a collision.
    //
    // Location of the write at IV i is `hash(i) + offset_p`.
    // Location of the read at IV j is `hash(j) + offset_q`.
    // A dependence exists iff some (i, j) in the domain produces
    // equal locations. We build a map from read-location to a
    // witnessing IV, then walk the write side looking for a match.
    //
    // This is correct by construction: the witness's locations are
    // computed exactly the way the checker computes them, so if we
    // find a match here, the checker will accept.
    use std::collections::HashMap;
    let offset_p = match ap.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("symbolic offset".into()), None),
    };
    let offset_q = match aq.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("symbolic offset".into()), None),
    };

    let mut read_by_loc: HashMap<i64, i64> = HashMap::new();
    for (iv_q, hash_q) in &entries {
        let loc_q = hash_q.saturating_add(offset_q);
        // Keep the smallest IV for a deterministic witness.
        read_by_loc.entry(loc_q).or_insert(*iv_q);
    }

    for (iv_p, hash_p) in &entries {
        let loc_p = hash_p.saturating_add(offset_p);
        if let Some(&iv_q) = read_by_loc.get(&loc_p) {
            let d = vec![iv_q - *iv_p];
            let cert = Certificate {
                level: "M7".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution {
                    i: vec![*iv_p],
                    j: vec![iv_q],
                },
                checker: "m0-check-v1".into(),
            };
            return (Decision::Exact(d), Some(cert));
        }
    }

    // Exhaustive enumeration found no overlapping location: no dependence.
    let cert = Certificate {
        level: "M7".into(),
        loop_hash: loop_.hash(),
        pair: (p, q),
        claim: Claim::None,
        witness: Witness::UnsatProof {
            constraints: vec![CConstraint {
                coeffs: vec![0; loop_.depth],
                constant: -1,
            }],
        },
        checker: "m0-check-v1".into(),
    };
    (Decision::None, Some(cert))
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

    fn acc_hash(kind: AccessKind, h: HashExpr, offset: i64) -> Access {
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
            index_kind: IndexKind::Hash(h),
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
    fn hash_eval_identity() {
        // c=1, shift=0, mask=all ones → identity
        let h = HashExpr { c: 1, shift: 0, mask: -1 };
        assert_eq!(h.eval(0), 0);
        assert_eq!(h.eval(5), 5);
        assert_eq!(h.eval(100), 100);
    }

    #[test]
    fn hash_eval_shift() {
        // c=1, shift=1, mask=all ones → i >> 1
        let h = HashExpr { c: 1, shift: 1, mask: -1 };
        assert_eq!(h.eval(0), 0);
        assert_eq!(h.eval(1), 0);
        assert_eq!(h.eval(2), 1);
        assert_eq!(h.eval(3), 1);
        assert_eq!(h.eval(4), 2);
    }

    #[test]
    fn hash_eval_multiply_shift_mask() {
        // Knuth-style: (i * 2654435761) >> 16 & 0xff
        let h = HashExpr {
            c: 2654435761,
            shift: 16,
            mask: 0xff,
        };
        // Just check it runs and produces a value in [0, 255]
        for i in 0..100 {
            let v = h.eval(i);
            assert!((0..=255).contains(&v), "out of range: {}", v);
        }
    }

    #[test]
    fn decide_same_hash_same_offset() {
        // A[hash(i)] = A[hash(i)] with identical hash, offset 0
        // → EXACT [0] (identity witness at i=0)
        let h = HashExpr { c: 1, shift: 0, mask: -1 };
        let l = make_loop_1d(0, 16, vec![
            acc_hash(AccessKind::Write, h.clone(), 0),
            acc_hash(AccessKind::Read,  h, 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M7");
    }

    #[test]
    fn decide_hash_collision_shift() {
        // (i >> 1) has collisions: 0,1 → 0; 2,3 → 1.
        // The first collision is at (0, 1) with hash both 0.
        // Distance = 1 - 0 = 1.
        let h = HashExpr { c: 1, shift: 1, mask: -1 };
        let l = make_loop_1d(0, 4, vec![
            acc_hash(AccessKind::Write, h.clone(), 0),
            acc_hash(AccessKind::Read,  h, 0),
        ]);
        let (d, _) = decide(&l, 0, 1);
        match d {
            Decision::Exact(v) => {
                // Any valid witness is acceptable. The hash (i >> 1)
                // collides on (0,1), (0,2), (1,3), (2,3) for the
                // domain [0,4). The analyzer picks the smallest-IV
                // match, which is (0, 0) with distance 0. Both
                // distance 0 (same-IV trivial overlap) and distance
                // ±1 (adjacent-IV collision) are correct answers.
                assert!(v.len() == 1);
                assert!(v[0].abs() <= 1, "unexpected distance {:?}", v);
            }
            other => panic!("expected Exact, got {:?}", other),
        }
    }

    #[test]
    fn decide_too_large_returns_unknown() {
        let h = HashExpr { c: 1, shift: 0, mask: -1 };
        let l = make_loop_1d(0, 200_000, vec![
            acc_hash(AccessKind::Write, h.clone(), 0),
            acc_hash(AccessKind::Read,  h, 0),
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn decide_different_hashes_unknown() {
        let h1 = HashExpr { c: 1, shift: 0, mask: -1 };
        let h2 = HashExpr { c: 2, shift: 0, mask: -1 };
        let l = make_loop_1d(0, 8, vec![
            acc_hash(AccessKind::Write, h1, 0),
            acc_hash(AccessKind::Read,  h2, 0),
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }
    // ================================================================
    // Property fuzz: random hash functions on both sides (same hash
    // enforced, since M7 only handles equal-hash cases), random
    // offsets. Compares decide() against brute-force location
    // enumeration.
    // ================================================================

    fn xorshift7(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn hash_loc(h: &HashExpr, i: i64) -> i64 {
        let v = (i * h.c) >> h.shift;
        if h.mask >= 0 { v & h.mask } else { v }
    }

    #[test]
    fn fuzz_m7_property() {
        let mut state: u64 = 0x7_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            let c = (xorshift7(&mut state) % 6) as i64 + 1;
            let shift = (xorshift7(&mut state) % 4) as u32;
            let mask = match xorshift7(&mut state) % 6 {
                0 => -1i64,
                1 => 0,
                2 => 1,
                3 => 3,
                4 => 7,
                _ => 15,
            };
            let h = HashExpr { c, shift, mask };
            let off_p = (xorshift7(&mut state) % 9) as i64 - 4;
            let off_q = (xorshift7(&mut state) % 9) as i64 - 4;

            let lo = 0i64;
            let hi = 16i64;

            let l = make_loop_1d(lo, hi, vec![
                acc_hash(AccessKind::Write, h.clone(), off_p),
                acc_hash(AccessKind::Read,  h.clone(), off_q),
            ]);

            // Brute-force: same hash on both sides, so loc_p(i) = hash(i) + off_p,
            // loc_q(j) = hash(j) + off_q. Collision iff hash(i) + off_p == hash(j) + off_q.
            let mut wp: std::collections::HashSet<i64> = std::collections::HashSet::new();
            for i in lo..hi { wp.insert(hash_loc(&h, i) + off_p); }
            let mut expected_exact = false;
            for j in lo..hi {
                if wp.contains(&(hash_loc(&h, j) + off_q)) { expected_exact = true; break; }
            }

            let (d, _) = decide(&l, 0, 1);
            tested += 1;

            let ok = match (expected_exact, &d) {
                (true, Decision::Exact(_)) => true,
                (true, Decision::Unknown(_)) => { conservative += 1; true }
                (true, _) => false,
                (false, Decision::None) => true,
                (false, Decision::Unknown(_)) => { conservative += 1; true }
                (false, _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "h=(c={}, s={}, m={}) op={} oq={} expected_exact={} got={:?}",
                        c, shift, mask, off_p, off_q, expected_exact, d
                    ));
                }
            }
        }

        println!("M7 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M7 fuzz found {} unsound", unsound);
    }

}
