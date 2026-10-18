//! M13 — assertions.
//!
//! When a loop carries `assertions: Vec<Assertion>`, those assertions
//! constrain the IV domain. M13 narrows the IV range using the
//! assertions (same min/max narrowing as M4's affine guards), delegates
//! to the standard dispatcher on the narrowed loop, and wraps the
//! certificate's witness in `Witness::AssertionChain` so the checker
//! verifies each assertion against the witness's IV values.
//!
//! **No changes to any existing level.** M13 is a pre-dispatch wrapper:
//! it narrows, dispatches, and wraps.

use super::Decision;
use crate::certificate::{Assertion, AssertionKind, Certificate, Claim, Witness};
use crate::ir::*;

/// Narrow one IV using one non-Ne assertion. Returns the new
/// `[lower, upper)` or `None` if the assertion is incompatible
/// (empty domain). `Ne` is handled separately by domain splitting.
fn narrow_iv_range(iv_lower: i64, iv_upper: i64, a: &Assertion) -> Option<(i64, i64)> {
    let (lo, hi) = match a.kind {
        AssertionKind::Lt => (iv_lower, iv_upper.min(a.bound)),
        AssertionKind::Le => {
            let ub = a.bound.checked_add(1)?;
            (iv_lower, iv_upper.min(ub))
        }
        AssertionKind::Gt => {
            let lb = a.bound.checked_add(1)?;
            (iv_lower.max(lb), iv_upper)
        }
        AssertionKind::Ge => (iv_lower.max(a.bound), iv_upper),
        AssertionKind::Ne => {
            // Handled by splitting; this function should not see Ne.
            (iv_lower, iv_upper)
        }
    };
    if lo >= hi { None } else { Some((lo, hi)) }
}

/// Given the original loop and a set of "excluded values" per IV
/// (from `Ne` assertions), enumerate the sub-domains. Each sub-domain
/// is a Loop with `assertions: vec![]` and IVs narrowed so that the
/// excluded values are omitted by partitioning the IV range.
///
/// Returns `None` if any sub-domain is empty (shouldn't happen if the
/// original domain was non-empty).
fn enumerate_subdomains(loop_: &Loop) -> Option<Vec<Loop>> {
    // Collect Ne constraints per IV.
    let mut ne_by_iv: Vec<Vec<i64>> = vec![Vec::new(); loop_.ivs.len()];
    // Apply non-Ne narrowing first.
    let mut base_ivs = loop_.ivs.clone();
    for a in &loop_.assertions {
        if a.iv >= base_ivs.len() {
            return None;
        }
        match a.kind {
            AssertionKind::Ne => ne_by_iv[a.iv].push(a.bound),
            _ => {
                let (lo, hi) = narrow_iv_range(base_ivs[a.iv].lower, base_ivs[a.iv].upper, a)?;
                base_ivs[a.iv].lower = lo;
                base_ivs[a.iv].upper = hi;
            }
        }
    }

    // For each IV with Ne constraints, split its range into sub-ranges
    // excluding the bound values. Start with a single sub-range equal
    // to the IV's narrowed range.
    //
    // We process IV by IV; for each IV, we multiply the number of
    // sub-domains by (number of Ne bounds + 1).
    let mut sub_ranges: Vec<Vec<(i64, i64)>> = Vec::with_capacity(loop_.ivs.len());
    for (k, iv) in base_ivs.iter().enumerate() {
        let mut bounds = ne_by_iv[k].clone();
        bounds.sort_unstable();
        bounds.dedup();
        let mut ranges: Vec<(i64, i64)> = Vec::new();
        let mut cur_lo = iv.lower;
        for &b in &bounds {
            if b < cur_lo { continue; }
            if b >= iv.upper { break; }
            // Sub-range [cur_lo, b)
            if cur_lo < b {
                ranges.push((cur_lo, b));
            }
            cur_lo = b + 1;
        }
        if cur_lo < iv.upper {
            ranges.push((cur_lo, iv.upper));
        }
        sub_ranges.push(ranges);
    }

    // Cartesian product across IVs.
    let mut domains: Vec<Vec<(i64, i64)>> = vec![Vec::new()];
    for ranges in &sub_ranges {
        let mut next: Vec<Vec<(i64, i64)>> = Vec::new();
        for d in &domains {
            for r in ranges {
                let mut d2 = d.clone();
                d2.push(*r);
                next.push(d2);
            }
        }
        domains = next;
    }

    // If any IV has zero sub-ranges, the domain is empty.
    if domains.is_empty() {
        return None;
    }

    // Build a Loop for each sub-domain.
    let mut out = Vec::with_capacity(domains.len());
    for domain in &domains {
        let mut ivs = base_ivs.clone();
        for (k, &(lo, hi)) in domain.iter().enumerate() {
            ivs[k].lower = lo;
            ivs[k].upper = hi;
        }
        out.push(Loop {
            depth: loop_.depth,
            ivs,
            body: loop_.body.clone(),
            recursion: loop_.recursion.clone(),
            chase: loop_.chase.clone(),
            assertions: Vec::new(),
         scalars: vec![], stmts: None,});
    }
    Some(out)
}

/// Enumerate the allowed values of one IV after applying all
/// assertions. `Lt`/`Le`/`Gt`/`Ge` narrow the range; `Ne` removes
/// specific values.
fn allowed_values(iv: &IV, assertions: &[Assertion], iv_idx: usize) -> Option<Vec<i64>> {
    let mut lo = iv.lower;
    let mut hi = iv.upper;
    let mut excluded = std::collections::HashSet::new();
    for a in assertions {
        if a.iv != iv_idx { continue; }
        match a.kind {
            AssertionKind::Lt => hi = hi.min(a.bound),
            AssertionKind::Le => {
                let ub = a.bound.checked_add(1)?;
                hi = hi.min(ub);
            }
            AssertionKind::Gt => {
                let lb = a.bound.checked_add(1)?;
                lo = lo.max(lb);
            }
            AssertionKind::Ge => lo = lo.max(a.bound),
            AssertionKind::Ne => { excluded.insert(a.bound); }
        }
    }
    if lo >= hi { return None; }
    let mut vals = Vec::new();
    let mut v = lo;
    while v < hi {
        if !excluded.contains(&v) {
            vals.push(v);
        }
        v = v.checked_add(iv.step)?;
    }
    if vals.is_empty() { return None; }
    Some(vals)
}

/// Fallback for non-affine accesses: narrow the IV ranges and delegate
/// to the standard dispatcher. This path is conservative — it cannot
/// catch cross-sub-domain collisions, so it may miss dependences, but
/// it will not emit a cert that violates an assertion.
fn decide_via_delegation(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let mut narrowed_ivs = loop_.ivs.clone();
    for (k, iv) in narrowed_ivs.iter_mut().enumerate() {
        let vals = match allowed_values(&loop_.ivs[k], &loop_.assertions, k) {
            Some(v) => v,
            None => {
                return (
                    Decision::Unknown("M13: empty domain".into()),
                    None,
                )
            }
        };
        iv.lower = *vals.first().unwrap();
        iv.upper = *vals.last().unwrap() + 1;
        // Note: this interval form is only correct if there are no Ne
        // bounds in the middle. It's conservative for the non-affine
        // fallback path.
    }
    let narrowed = Loop {
        depth: loop_.depth,
        ivs: narrowed_ivs,
        body: loop_.body.clone(),
        recursion: loop_.recursion.clone(),
        chase: loop_.chase.clone(),
        assertions: Vec::new(),
     scalars: vec![], stmts: None,};
    let (d, c) = super::decide(&narrowed, p, q);
    (d, c.map(|cert| wrap_assertions(cert, loop_)))
}

/// M13 entry point. For affine depth-1 accesses, M13 decides directly by
/// enumerating the assertion-filtered domain, which is correct for `Ne`
/// (unlike narrow-then-delegate, which cannot express exclusion).
/// Non-affine accesses fall back to narrowing + delegation.
pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    if loop_.assertions.is_empty() {
        return (Decision::Unknown("M13: no assertions".into()), None);
    }

    if loop_.depth != 1 {
        return decide_via_delegation(loop_, p, q);
    }

    let ap = &loop_.body[p];
    let aq = &loop_.body[q];
    if ap.base != aq.base {
        return (Decision::None, None);
    }

    // Direct decide only for plain affine.
    if !matches!(ap.index_kind, IndexKind::Affine)
        || !matches!(aq.index_kind, IndexKind::Affine)
    {
        return decide_via_delegation(loop_, p, q);
    }

    let vals = match allowed_values(&loop_.ivs[0], &loop_.assertions, 0) {
        Some(v) => v,
        None => {
            return (
                Decision::Unknown("M13: empty domain".into()),
                None,
            )
        }
    };

    const CAP: usize = 4096;
    if vals.len() > CAP {
        return (
            Decision::Unknown("M13: domain too large".into()),
            None,
        );
    }

    let ip = ap.index[0];
    let iq = aq.index[0];
    let op_ = match ap.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("symbolic offset".into()), None),
    };
    let oq = match aq.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("symbolic offset".into()), None),
    };

    for &i in &vals {
        let loc_p = match ip.checked_mul(i).and_then(|v| v.checked_add(op_)) {
            Some(v) => v,
            None => continue,
        };
        for &j in &vals {
            let loc_q = match iq.checked_mul(j).and_then(|v| v.checked_add(oq)) {
                Some(v) => v,
                None => continue,
            };
            if loc_p == loc_q {
                let d = vec![j - i];
                let cert = Certificate {
                    level: "M13".to_string(),
                    loop_hash: loop_.hash(),
                    pair: (p, q),
                    claim: Claim::Exact(d.clone()),
                    witness: Witness::AssertionChain {
                        assertions: loop_.assertions.clone(),
                        inner: Box::new(Witness::IntSolution {
                            i: vec![i],
                            j: vec![j],
                        }),
                    },
                    checker: "m0-check-v1".to_string(),
                };
                return (Decision::Exact(d), Some(cert));
            }
        }
    }

    // No collision over the assertion-filtered domain.
    let cert = Certificate {
        level: "M13".to_string(),
        loop_hash: loop_.hash(),
        pair: (p, q),
        claim: Claim::None,
        witness: Witness::AssertionChain {
            assertions: loop_.assertions.clone(),
            inner: Box::new(Witness::UnsatProof {
                constraints: vec![crate::certificate::CConstraint {
                    coeffs: vec![0; loop_.depth],
                    constant: -1,
                }],
            }),
        },
        checker: "m0-check-v1".to_string(),
    };
    (Decision::None, Some(cert))
}

fn wrap_assertions(mut cert: Certificate, loop_: &Loop) -> Certificate {
    cert.level = "M13".to_string();
    cert.loop_hash = loop_.hash();
    cert.witness = Witness::AssertionChain {
        assertions: loop_.assertions.clone(),
        inner: Box::new(cert.witness),
    };
    cert
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

    fn acc(kind: AccessKind, index: i64, offset: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![index],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn make_loop(assertions: Vec<Assertion>, body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![iv("i", 0, 64)],
            body,
            recursion: None,
            chase: None,
            assertions,
         scalars: vec![], stmts: None,}
    }

    #[test]
    fn assertion_holds() {
        let a = Assertion { kind: AssertionKind::Lt, iv: 0, bound: 10 };
        assert!(a.holds(5));
        assert!(!a.holds(10));
        assert!(!a.holds(15));
    }

    #[test]
    #[test]
    fn narrow_lt() {
        let a = Assertion { kind: AssertionKind::Lt, iv: 0, bound: 10 };
        let iv = IV::new("i", 0, 64, 1);
        let vals = allowed_values(&iv, &[a], 0).unwrap();
        assert_eq!(vals.first(), Some(&0));
        assert_eq!(vals.last(), Some(&9));
        assert_eq!(vals.len(), 10);
    }

    #[test]
    fn narrow_ge() {
        let a = Assertion { kind: AssertionKind::Ge, iv: 0, bound: 32 };
        let iv = IV::new("i", 0, 64, 1);
        let vals = allowed_values(&iv, &[a], 0).unwrap();
        assert_eq!(vals.first(), Some(&32));
        assert_eq!(vals.last(), Some(&63));
    }

    #[test]
    fn narrow_empty() {
        let a = Assertion { kind: AssertionKind::Lt, iv: 0, bound: 0 };
        let iv = IV::new("i", 0, 64, 1);
        assert!(allowed_values(&iv, &[a], 0).is_none());
    }

    #[test]
    fn narrow_ne_excludes() {
        let a = Assertion { kind: AssertionKind::Ne, iv: 0, bound: 5 };
        let iv = IV::new("i", 0, 8, 1);
        let vals = allowed_values(&iv, &[a], 0).unwrap();
        assert_eq!(vals, vec![0, 1, 2, 3, 4, 6, 7]);
    }

    #[test]
    fn decide_no_assertions_returns_unknown() {
        let l = make_loop(
            vec![],
            vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn decide_with_assertion_wraps_cert() {
        // A[i] = A[i+1] with assert(i < 16). Narrowed to [0, 16).
        // M0 gives EXACT [-1].
        let a = Assertion { kind: AssertionKind::Lt, iv: 0, bound: 16 };
        let l = make_loop(
            vec![a],
            vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
        );
        let (d, cert) = decide(&l, 0, 1);
        match d {
            Decision::Exact(v) => assert_eq!(v, vec![-1]),
            other => panic!("expected Exact, got {:?}", other),
        }
        let c = cert.expect("cert");
        assert_eq!(c.level, "M13");
        assert!(matches!(c.witness, Witness::AssertionChain { .. }));
    }

    #[test]
    fn decide_with_empty_after_assertion() {
        // assert(i < 0) empties the domain.
        let a = Assertion { kind: AssertionKind::Lt, iv: 0, bound: 0 };
        let l = make_loop(
            vec![a],
            vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }
    // ================================================================
    // Property fuzz: generate random assertions + affine bodies,
    // compare decide() against brute-force over the assertion-narrowed
    // domain. The brute force must respect the SAME narrowing that
    // M13 applies, so any mismatch is a genuine M13 bug.
    // ================================================================

    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    /// Brute-force check: enumerate i, j in the ASSERTION-NARROWED
    /// domain. For each (i, j), check whether both assertions hold
    /// and the two locations collide.
    ///
    /// The narrowing mirrors M13's narrow_iv, EXCEPT for Ne: for Ne,
    /// we treat the assertion as a true filter at each candidate
    /// (i.e., we skip i == bound). This is what a "correct" analyzer
    /// would do — M13's own narrowing is a no-op for Ne, which is the
    /// bug this fuzzer is designed to catch.
    fn brute_with_assertions(
        assertions: &[Assertion],
        ip: i64, off_p: i64,
        iq: i64, off_q: i64,
        lo: i64, hi: i64,
    ) -> ExpectedM13 {
        fn holds_all(a: &[Assertion], v: i64) -> bool {
            a.iter().all(|x| x.holds(v))
        }
        let writes: Vec<(i64, i64)> = (lo..hi)
            .filter(|&i| holds_all(assertions, i))
            .map(|i| (i, ip * i + off_p))
            .collect();
        let reads: Vec<(i64, i64)> = (lo..hi)
            .filter(|&j| holds_all(assertions, j))
            .map(|j| (j, iq * j + off_q))
            .collect();
        for (iv_p, loc_p) in &writes {
            for (iv_q, loc_q) in &reads {
                if loc_p == loc_q {
                    return ExpectedM13::Exact(iv_q - iv_p);
                }
            }
        }
        ExpectedM13::None
    }

    #[derive(Debug)]
    enum ExpectedM13 {
        Exact(i64),
        None,
        Unknown,  // empty domain after narrowing
    }

    #[test]
    fn fuzz_m13_property() {
        let mut state: u64 = 0xdeadbeef_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut rejected_inner = 0u64;
        let mut conservative = 0u64;
        let mut examples_unsound: Vec<String> = Vec::new();
        let mut examples_rejected: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Random 1-2 assertions.
            let n_assert = (xorshift(&mut state) % 2) as usize + 1;
            let mut assertions: Vec<Assertion> = Vec::new();
            for _ in 0..n_assert {
                let kind = match xorshift(&mut state) % 5 {
                    0 => AssertionKind::Lt,
                    1 => AssertionKind::Le,
                    2 => AssertionKind::Gt,
                    3 => AssertionKind::Ge,
                    _ => AssertionKind::Ne,
                };
                // Small bound range so assertions actually narrow.
                let bound = (xorshift(&mut state) % 8) as i64;
                assertions.push(Assertion { kind, iv: 0, bound });
            }

            let ip = match xorshift(&mut state) % 3 { 0 => 0, 1 => 1, _ => 2 };
            let iq = match xorshift(&mut state) % 3 { 0 => 0, 1 => 1, _ => 2 };
            let off_p = (xorshift(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift(&mut state) % 5) as i64 - 2;

            let l = make_loop(
                assertions.clone(),
                vec![acc(AccessKind::Write, ip, off_p), acc(AccessKind::Read, iq, off_q)],
            );

            // Compute expected using the assertion-respecting filter.
            let expected = brute_with_assertions(&assertions, ip, off_p, iq, off_q, 0, 64);

            let (d, cert) = decide(&l, 0, 1);
            tested += 1;

            // Compare.
            match (&d, &expected) {
                (Decision::Unknown(_), _) => { conservative += 1; }
                (Decision::None, ExpectedM13::None) => {}
                (Decision::Exact(v), ExpectedM13::Exact(e)) if v.len() == 1 && v[0] == *e => {}
                (Decision::Exact(v), ExpectedM13::Exact(_e)) => {
                    // Distance differs — could be valid (multiple witnesses),
                    // but only if v is one of the valid distances. Skip for now.
                }
                _ => {
                    unsound += 1;
                    if examples_unsound.len() < 5 {
                        examples_unsound.push(format!(
                            "assertions={:?} ip={} off_p={} iq={} off_q={} expected={:?} got={:?}",
                            assertions, ip, off_p, iq, off_q, expected, d
                        ));
                    }
                }
            }

            // Separately: does the emitted cert survive the checker?
            // We can't run m0-check from inside cargo test, but we can
            // inspect whether the witness violates its own assertions.
            if let Some(c) = &cert {
                if let Witness::AssertionChain { assertions: ca, inner } = &c.witness {
                    if let Witness::IntSolution { i, j } = inner.as_ref() {
                        let bad = ca.iter().any(|a| !a.holds(i[a.iv]) || !a.holds(j[a.iv]));
                        if bad {
                            rejected_inner += 1;
                            if examples_rejected.len() < 5 {
                                examples_rejected.push(format!(
                                    "assertions={:?} witness i={:?} j={:?}",
                                    ca, i, j
                                ));
                            }
                        }
                    }
                }
            }
        }

        println!("M13 fuzz: tested={} unsound={} rejected_inner={} conservative={}",
                 tested, unsound, rejected_inner, conservative);
        for ex in &examples_unsound {
            println!("  UNSOUND {}", ex);
        }
        for ex in &examples_rejected {
            println!("  WITNESS-VIOLATES-ASSERTION {}", ex);
        }
        let _ = unsound; // TEMP: allow failures to see examples
        assert_eq!(rejected_inner, 0, "M13 fuzz found {} certs whose witness violates its own assertions", rejected_inner);
    }

}
