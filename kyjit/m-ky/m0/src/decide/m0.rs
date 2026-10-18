use crate::certificate::*;
use crate::ir::*;
use crate::solver::{self, Constraint, SolveResult, System};
use super::{Decision, gcd_i64};

/// The decision procedure assumes a unit-step iteration space: every stage
/// reasons over the dense box `[lower, upper)`, which is the set of executions
/// only when `step == 1`.
///
/// That assumption is invisible on a step-1 loop but wrong on a stepped one. A
/// loop `for (i = 0; i < 2; i += 2)` runs only `i = 0`, yet the box `[0, 2)`
/// also contains `i = 1`; a witness drawn from the box can therefore name an
/// iteration the program never performs. The checker re-derives the lattice
/// itself and rejects such a witness (`access_executes_at`), so the failure
/// mode is a rejected certificate rather than a wrong answer -- but the level
/// is still wrong to emit it, and the same reasoning underlies its absence
/// claims.
///
/// So a stepped loop is normalised to an equivalent unit-step loop, decided
/// there, and the answer mapped back. See [`decide_stepped`].
pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    if loop_.recursion.is_none()
        && loop_.ivs.len() == loop_.depth
        && loop_.ivs.iter().any(|iv| iv.step != 1)
    {
        return decide_stepped(loop_, p, q);
    }
    decide_dense(loop_, p, q)
}

/// The dense, unit-step decision procedure. Callers must pass a loop whose IVs
/// step by 1; [`decide`] normalises anything else first.
fn decide_dense(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];


    // ---------- Stage 1: same base? ----------
    // Two accesses with different base pointers cannot alias.
    // Return NONE immediately, no analysis needed.
    if ap.base != aq.base {
        return (Decision::None, None);
    }

    // ---------- Stage 2: both reads? ----------
    // Two reads of the same location do not conflict. No dependence.
    if ap.kind == AccessKind::Read && aq.kind == AccessKind::Read {
        return (Decision::None, None);
    }
    // ---------- Stage 3: build the equality system ----------
    // Variables: i[0..d] and j[0..d], total n = 2*d.
    // Equality: ap.index·i + ap.offset = aq.index·j + aq.offset
    // Encoded as two inequalities:
    //   ap.index·i - aq.index·j <= aq.offset - ap.offset
    //   -ap.index·i + aq.index·j <= ap.offset - aq.offset
    // Plus bounds for every IV:
    //   lo_k <= i_k < hi_k    and    lo_k <= j_k < hi_k

    // Offsets must be concrete for the constraint system; symbolic offsets
    // make the problem parametric and require a different decision procedure.
    let ap_off = match ap.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("symbolic offset".into()), None),
    };
    let aq_off = match aq.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("symbolic offset".into()), None),
    };

    let d = loop_.depth;
    let n = 2 * d;
    let mut constraints = vec![];

    // ap.index . i - aq.index . j <= aq.offset - ap.offset
    let mut c1 = vec![0i64; n];
    for k in 0..d {
        c1[k] = ap.index[k];
        c1[d + k] = -aq.index[k];
    }
    constraints.push(Constraint {
        coeffs: c1.clone(),
        constant: aq_off - ap_off,
    });

    // -(ap.index . i) + aq.index . j <= ap.offset - aq.offset
    let mut c2 = vec![0i64; n];
    for k in 0..d {
        c2[k] = -ap.index[k];
        c2[d + k] = aq.index[k];
    }
    constraints.push(Constraint {
        coeffs: c2.clone(),
        constant: ap_off - aq_off,
    });

    // Iteration bounds: lo <= i[k] < hi
    for k in 0..d {
        // i[k] <= hi - 1
        let mut c = vec![0i64; n];
        c[k] = 1;
        constraints.push(Constraint {
            coeffs: c.clone(),
            constant: loop_.ivs[k].upper - 1,
        });
        // -i[k] <= -lo
        let mut c = vec![0i64; n];
        c[k] = -1;
        constraints.push(Constraint {
            coeffs: c,
            constant: -loop_.ivs[k].lower,
        });
        // same for j
        let mut c = vec![0i64; n];
        c[d + k] = 1;
        constraints.push(Constraint {
            coeffs: c.clone(),
            constant: loop_.ivs[k].upper - 1,
        });
        let mut c = vec![0i64; n];
        c[d + k] = -1;
        constraints.push(Constraint {
            coeffs: c,
            constant: -loop_.ivs[k].lower,
        });
    }

    // ---------- Stage 4: GCD divisibility test ----------
    // The equality ap.index·i - aq.index·j = aq.offset - ap.offset
    // has an integer solution iff gcd(all coefficients) divides the
    // constant on the right. Pure arithmetic, no FM needed.
    let mut g: i64 = 0;
    for k in 0..d {
        g = gcd_i64(g, ap.index[k]);
        g = gcd_i64(g, aq.index[k]);
    }
    let rhs = aq_off - ap_off;
    if g > 1 && rhs % g != 0 {
        return (Decision::None, None);
    }

    // ---------- Stage 4b: exact Banerjee-Wolfe bounds test ----------
    // For each candidate dependence level, the ordering constraint on
    // (i, j) carves the box into a triangle whose extremes mu/nu are exact
    // (Wolfe 4.7). Every admissible pair (i, j) belongs to exactly one
    // arm: equal everywhere (level d+1), or differing first at some level
    // L with i_L > j_L (backward) or j_L > i_L (forward). So if the
    // required offset difference rhs = b0 - a0 misses every arm's span,
    // no colliding pair exists -- a *proof*, not a relaxation heuristic.
    //
    // Only applied when both offsets are constant; a symbolic offset has
    // no concrete value to compare against, and guessing one is exactly
    // the unsoundness this stage is meant to remove.
    if let (Some(a0), Some(b0)) = (ap.offset.as_const(), aq.offset.as_const()) {
        let p: Vec<i64> = loop_.ivs.iter().map(|iv| iv.lower).collect();
        let q: Vec<i64> = loop_.ivs.iter().map(|iv| iv.upper - 1).collect();
        let mut possible = false;
        for level in 1..=(d + 1) {
            if crate::solver::extreme::bounds_test_holds(
                &ap.index, a0, &aq.index, b0, &p, &q, level,
            ) {
                possible = true;
                break;
            }
        }
        if !possible {
            return (Decision::None, None);
        }
    }

    // ---------- Stage 5: Banerjee bounds (same-stride fast path) ----------
    // For 1D loops where both accesses have the same stride, the
    // distance is directly computable. No FM needed.
    if d == 1 && ap.index[0] == aq.index[0] && ap.index[0] != 0 {
        let c = ap.index[0];
        let diff = match aq_off.checked_sub(ap_off) {
            Some(v) => v,
            None => return (Decision::None, None),
        };

        if diff % c != 0 {
            return (Decision::None, None);
        }
        let shift = diff / c; // i - j = shift
        let dist = match shift.checked_neg() {
            Some(v) => v,
            None => return (Decision::None, None),
        };    // distance = j - i

        let lo = loop_.ivs[0].lower;
        let hi = loop_.ivs[0].upper;

        // Need i, j in [lo, hi) with i - j = shift.
        // i = j + shift, so j must satisfy:
        //   lo <= j < hi    and    lo <= j + shift < hi
        let j_lo = lo.max(match lo.checked_sub(shift) {
            Some(v) => v,
            None => return (Decision::None, None),
        });
        let j_hi = hi.min(match hi.checked_sub(shift) {
            Some(v) => v,
            None => return (Decision::None, None),
        });
        if j_lo >= j_hi {
            return (Decision::None, None);
        }

        let j = j_lo;
        let i = match j.checked_add(shift) {
            Some(v) => v,
            None => return (Decision::None, None),
        };
        let dist_vec = vec![dist];
        let cert = Certificate {
            level: "M0".into(),
            loop_hash: loop_.hash(),
            pair: (p, q),
            claim: Claim::Exact(dist_vec.clone()),
            witness: Witness::IntSolution {
                i: vec![i],
                j: vec![j],
            },
            checker: "m0-check-v1".into(),
        };
        return (Decision::Exact(dist_vec), Some(cert));
    }

    // ---------- Stage 5b: exhaustive enumeration for depth-1 ----------
    // For depth-1 loops with bounded IV range, enumerate i in [lo, hi),
    // compute the write location, and solve aq_i * j + aq_off = write_loc
    // for j. If j is in range, emit Exact(j - i) with witness (i, j);
    // otherwise return None. This closes the completeness gap on
    // opposite-sign strides (e.g. A[-3*i + op] vs A[2*i + oq]) that
    // Stage 5's same-stride fast path and FM both decline.
    //
    // Sound and complete for depth-1, range <= 4096. Larger ranges fall
    // through to Fourier-Motzkin.
    if d == 1 {
        let lo = loop_.ivs[0].lower;
        let hi = loop_.ivs[0].upper;
        let range = match hi.checked_sub(lo) {
            Some(v) => v.max(0) as usize,
            None => return (Decision::None, None),
        };

        if range > 0 && range <= 4096 {
            let ap_i = ap.index[0];
            let aq_i = aq.index[0];
            // Use the outer ap_off and aq_off constants (already checked for const)

            if aq_i == 0 {
                // Read hits a single location regardless of j.
                let read_loc = aq_off;
                for i in lo..hi {
                    let write_loc = match ap_i.checked_mul(i).and_then(|v| v.checked_add(ap_off)) {
                        Some(v) => v,
                        None => continue,
                    };
                    if write_loc == read_loc {
                        let j = lo;
                        let dist = match j.checked_sub(i) {
                            Some(v) => v,
                            None => continue,
                        };
                        let cert = Certificate {
                            level: "M0".into(),
                            loop_hash: loop_.hash(),
                            pair: (p, q),
                            claim: Claim::Exact(vec![dist]),
                            witness: Witness::IntSolution { i: vec![i], j: vec![j] },
                            checker: "m0-check-v1".into(),
                        };
                        return (Decision::Exact(vec![dist]), Some(cert));
                    }
                }
                return (Decision::None, None);
            }

            for i in lo..hi {
                let write_loc = match ap_i.checked_mul(i).and_then(|v| v.checked_add(ap_off)) {
                    Some(v) => v,
                    None => continue,
                };
                let num = match write_loc.checked_sub(aq_off) {
                    Some(v) => v,
                    None => continue,
                };
                if num % aq_i != 0 {
                    continue;
                }
                let j = num / aq_i;
                if j < lo || j >= hi {
                    continue;
                }
                let dist = match j.checked_sub(i) {
                    Some(v) => v,
                    None => continue,
                };
                let cert = Certificate {
                    level: "M0".into(),
                    loop_hash: loop_.hash(),
                    pair: (p, q),
                    claim: Claim::Exact(vec![dist]),
                    witness: Witness::IntSolution { i: vec![i], j: vec![j] },
                    checker: "m0-check-v1".into(),
                };
                return (Decision::Exact(vec![dist]), Some(cert));
            }
            return (Decision::None, None);
        }
    }


    // ---------- Stage 6: fast 2D same-stride witness ----------
    // For 2D loops with the same strides in each dimension, find
    // (di, dj) such that c0·di + c1·dj = diff = aq.offset - ap.offset.
    // Then translate to concrete (i, j) coordinates in range.
    if d == 2
        && ap.index[0] == aq.index[0]
        && ap.index[1] == aq.index[1]
        && ap.index[0] != 0
        && ap.index[1] != 0
    {
        let c0 = ap.index[0];
        let c1 = ap.index[1];
        let diff = match aq_off.checked_sub(ap_off) {
            Some(v) => v,
            None => return (Decision::None, None),
        };

        let lo0 = loop_.ivs[0].lower;
        let hi0 = loop_.ivs[0].upper;
        let lo1 = loop_.ivs[1].lower;
        let hi1 = loop_.ivs[1].upper;

        let span0 = match hi0.checked_sub(lo0) {
            Some(v) => v,
            None => return (Decision::None, None),
        };

        let mut found: Option<(i64, i64, i64, i64)> = None;
        let mut di: i64 = 0;
        let di_max = span0.max(1);
        while di <= di_max {
            let candidates: [i64; 2] = if di == 0 { [0, 0] } else { [di, -di] };
            for &d_i in &candidates {
                let c0_di = match c0.checked_mul(d_i) {
                    Some(v) => v,
                    None => continue,
                };
                let rem = match diff.checked_sub(c0_di) {
                    Some(v) => v,
                    None => continue,
                };
                if rem % c1 == 0 {
                    let d_j = rem / c1;
                    let i_dst_lo = lo0.max(match lo0.checked_sub(d_i) {
                        Some(v) => v,
                        None => continue,
                    });
                    let i_dst_hi = hi0.min(match hi0.checked_sub(d_i) {
                        Some(v) => v,
                        None => continue,
                    });
                    let j_dst_lo = lo1.max(match lo1.checked_sub(d_j) {
                        Some(v) => v,
                        None => continue,
                    });
                    let j_dst_hi = hi1.min(match hi1.checked_sub(d_j) {
                        Some(v) => v,
                        None => continue,
                    });
                    if i_dst_lo < i_dst_hi && j_dst_lo < j_dst_hi {
                        let i_dst = i_dst_lo;
                        let j_dst = j_dst_lo;
                        let i_src = match i_dst.checked_add(d_i) {
                            Some(v) => v,
                            None => continue,
                        };
                        let j_src = match j_dst.checked_add(d_j) {
                            Some(v) => v,
                            None => continue,
                        };
                        found = Some((i_src, j_src, i_dst, j_dst));
                        break;
                    }
                }
            }
            if found.is_some() { break; }
            di = match di.checked_add(1) {
                Some(v) => v,
                None => break,
            };
        }

        if let Some((i_src, j_src, i_dst, j_dst)) = found {
            let dist0 = match i_dst.checked_sub(i_src) {
                Some(v) => v,
                None => return (Decision::None, None),
            };
            let dist1 = match j_dst.checked_sub(j_src) {
                Some(v) => v,
                None => return (Decision::None, None),
            };
            let dist = vec![dist0, dist1];
            let cert = Certificate {
                level: "M0".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(dist.clone()),
                witness: Witness::IntSolution {
                    i: vec![i_src, j_src],
                    j: vec![i_dst, j_dst],
                },
                checker: "m0-check-v1".into(),
            };
            return (Decision::Exact(dist), Some(cert));
        }
    }

    // ---------- Stage 7: full FM (slow path) ----------
    // Reached only if Stages 5 and 6 did not fire. Handles:
    //   - Different strides per dimension
    //   - 3+ dimensional loops
    //   - Any case where the fast witness construction fails
    // Build the System, call the FM solver, and extract the witness.

    let mut ranges = vec![(0i64, 0i64); n];
    for k in 0..d {
        let lower = loop_.ivs[k].lower;
        let upper = match loop_.ivs[k].upper.checked_sub(1) {
            Some(v) => v,
            None => return (Decision::Unknown("M0: range overflow".into()), None),
        };
        ranges[k] = (lower, upper);
        ranges[d + k] = (lower, upper);
    }

    let sys = System {
        nvars: n,
        constraints,
        ranges,
    };
    let res = solver::solve(&sys);
    match res {
        // ---------- Stage 8: reverse-FM witness extraction ----------
        // FM returned a feasible witness. Split it into i[0..d] and j[0..d],
        // compute the distance vector, and build the certificate.

        SolveResult::Sat(w) => {
            // ---------- Stage 9: certificate emission ----------
            // Build and return the certificate for the FM-found witness.
            let i: Vec<i64> = w[..d].to_vec();
            let j: Vec<i64> = w[d..].to_vec();
            let mut dist = vec![0i64; d];
            for k in 0..d {
                dist[k] = match j[k].checked_sub(i[k]) {
                    Some(v) => v,
                    None => return (Decision::Unknown("M0: distance overflow".into()), None),
                };
            }
            let cert = Certificate {
                level: "M0".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(dist.clone()),
                witness: Witness::IntSolution {
                    i: i.clone(),
                    j: j.clone(),
                },
                checker: "m0-check-v1".into(),
            };
            (Decision::Exact(dist), Some(cert))
        }
        SolveResult::Unsat(proof) => {
            let cert = Certificate {
                level: "M0".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::None,
                witness: Witness::UnsatProof {
                    constraints: proof
                        .into_iter()
                        .map(|c| CConstraint {
                            coeffs: c.coeffs,
                            constant: c.constant,
                        })
                        .collect(),
                },
                checker: "m0-check-v1".into(),
            };
            (Decision::None, Some(cert))
        }
        SolveResult::Unknown(reason) => (Decision::Unknown(reason), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_acc(kind: AccessKind, base: &str, index: i64, offset: i64) -> Access {
        Access {
            kind,
            base: base.into(),
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

    fn mk_loop(lo: i64, hi: i64, body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", lo, hi, 1)],
            body,
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    /// A depth-1 loop with an explicit step. The IV runs `lo, lo + step, ...`
    /// while below `hi` -- *not* every integer in `[lo, hi)`, which is the
    /// distinction `decide` has to respect.
    fn mk_loop_step(lo: i64, hi: i64, step: i64, body: Vec<Access>) -> Loop {
        Loop {
            ivs: vec![IV::new("i", lo, hi, step)],
            scalars: vec![],
            stmts: None,
            ..mk_loop(lo, hi, body)
            }
    }

    fn mk_acc2d(kind: AccessKind, c: [i64; 2], offset: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: c.to_vec(),
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    /// The values a depth-1 loop's IV actually takes, walked directly rather
    /// than via any normalisation the implementation performs.
    fn ran_values(lo: i64, hi: i64, step: i64) -> Vec<i64> {
        let mut v = Vec::new();
        if step <= 0 {
            return v;
        }
        let mut p = lo;
        while p < hi {
            v.push(p);
            p += step;
        }
        v
    }

    fn xorshift0(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    #[test]
    fn a_stepped_loop_never_witnesses_an_iteration_it_skips() {
        // `for (i = 0; i < 2; i += 2)` runs only i = 0. The dense box [0, 2)
        // also holds i = 1, and a witness drawn from that box -- as every
        // stage before the normalisation did -- names an execution the program
        // never performs.
        let l = mk_loop_step(0, 2, 2, vec![
            mk_acc(AccessKind::Write, "A", -2, -3),
            mk_acc(AccessKind::Read,  "A", -2, -1),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        // No two *executed* iterations collide, so absence is the answer.
        // (The dense stages routinely refute without emitting a certificate;
        // what matters here is that the *dense box* did not produce a witness.)
        assert!(matches!(d, Decision::None), "got {:?}", d);
        assert!(cert.is_none(), "no collision to witness: {:?}", cert);

        // And when a collision does exist, the witness lies on the lattice.
        // The write at i = 0 and the read at j = 2 both name location 0; the
        // dense box also holds i = 1, and a witness drawn from it would be
        // distance 1 rather than the true 2.
        let l2 = mk_loop_step(0, 4, 2, vec![
            mk_acc(AccessKind::Write, "A", 1, 0),
            mk_acc(AccessKind::Read,  "A", 1, -2),
        ]);
        let (d2, cert2) = decide(&l2, 0, 1);
        assert!(matches!(d2, Decision::Exact(ref v) if v == &vec![2]), "got {:?}", d2);
        match cert2.unwrap().witness {
            Witness::IntSolution { i, j } => {
                assert_eq!((i[0], j[0]), (0, 2), "witness must be the real pair");
            }
            other => panic!("unexpected witness {:?}", other),
        }
    }

    #[test]
    fn the_distance_is_rescaled_by_the_step() {
        // Same stride on both sides: the location gap is 2 elements, reached by
        // one step of 2. On the normalised unit-step loop the gap is a single
        // iteration, so a distance reported without rescaling is off by the step.
        let l = mk_loop_step(0, 8, 2, vec![
            mk_acc(AccessKind::Write, "A", 1, 0),
            mk_acc(AccessKind::Read,  "A", 1, 2),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![-2]), "got {:?}", d);
        let c = cert.unwrap();
        assert!(matches!(c.claim, Claim::Exact(ref v) if v == &vec![-2]), "claim {:?}", c.claim);
        match c.witness {
            Witness::IntSolution { i, j } => {
                assert_eq!(i[0] - j[0], 2);
                assert_eq!(i[0] % 2, 0, "witness i={} not on the lattice", i[0]);
                assert_eq!(j[0] % 2, 0, "witness j={} not on the lattice", j[0]);
            }
            other => panic!("unexpected witness {:?}", other),
        }
    }

    #[test]
    fn a_step_that_skips_the_only_collision_refutes_it() {
        // Table-free analogue of the M2a.5 phantom-iteration bug: the write
        // lands on the read's location at i = 1 and i = 3, but the loop steps
        // by 2 from 0, so it runs neither.
        let body = || vec![
            mk_acc(AccessKind::Write, "A", 2, 0),
            mk_acc(AccessKind::Read,  "A", 1, 1),
        ];
        assert!(matches!(decide(&mk_loop_step(0, 4, 2, body()), 0, 1).0, Decision::None),
                "step=2 skips both colliding iterations");
        assert!(matches!(decide(&mk_loop_step(0, 4, 1, body()), 0, 1).0, Decision::Exact(_)),
                "step=1 reaches them");
    }

    #[test]
    fn a_non_positive_step_is_not_silently_treated_as_one() {
        let l = mk_loop_step(0, 8, 0, vec![
            mk_acc(AccessKind::Write, "A", 1, 0),
            mk_acc(AccessKind::Read,  "A", 1, 0),
        ]);
        assert!(matches!(decide(&l, 0, 1).0, Decision::Unknown(_)));
    }

    #[test]
    fn a_two_dimensional_stepped_loop_stays_on_its_lattice() {
        let l = Loop {
            depth: 2,
            ivs: vec![
                IV::new("i", 0, 6, 2),
                IV::new("j", 0, 6, 3),
            ],
            body: vec![
                mk_acc2d(AccessKind::Write, [1, 1], 0),
                mk_acc2d(AccessKind::Read,  [1, 1], 1),
            ],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "got {:?}", d);
        match cert.unwrap().witness {
            Witness::IntSolution { i, j } => {
                for v in i.iter().chain(j.iter()).copied() {
                    assert!(v >= 0 && v < 6);
                }
                assert_eq!(i[0] % 2, 0, "i={:?} off the level-0 lattice", i);
                assert_eq!(j[0] % 2, 0, "j={:?} off the level-0 lattice", j);
                assert_eq!(i[1] % 3, 0, "i={:?} off the level-1 lattice", i);
                assert_eq!(j[1] % 3, 0, "j={:?} off the level-1 lattice", j);
            }
            other => panic!("unexpected witness {:?}", other),
        }
    }

    #[test]
    fn fuzz_m0_property() {
        let mut state: u64 = 0x0_01234567_89abcdef;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        let lo = 0i64;
        let hi = 16i64;

        for _ in 0..5000 {
            let ip = (xorshift0(&mut state) % 5) as i64 - 2;
            let iq = (xorshift0(&mut state) % 5) as i64 - 2;
            let op_ = (xorshift0(&mut state) % 9) as i64 - 4;
            let oq = (xorshift0(&mut state) % 9) as i64 - 4;
            // Steps are sampled, and often bigger than 1. Before this the shape
            // was hardcoded to 1, which is exactly why the dense stages could
            // ignore `iv.step` without any test noticing.
            let step = (xorshift0(&mut state) % 4) as i64 + 1;

            let l = mk_loop_step(lo, hi, step, vec![
                mk_acc(AccessKind::Write, "A", ip, op_),
                mk_acc(AccessKind::Read,  "A", iq, oq),
            ]);

            // Brute-force ground truth over the values the IV really takes.
            let ivs = ran_values(lo, hi, step);
            let mut wp = std::collections::HashSet::new();
            for &i in &ivs { wp.insert(ip * i + op_); }
            let mut expected_exact = false;
            for &j in &ivs {
                if wp.contains(&(iq * j + oq)) { expected_exact = true; break; }
            }

            let (d, cert) = decide(&l, 0, 1);
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
                        "ip={} op={} iq={} oq={} step={} expected_exact={} got={:?}",
                        ip, op_, iq, oq, step, expected_exact, d
                    ));
                }
                continue;
            }

            // An Exact verdict must be witnessed by iterations the loop really
            // performs, and the claim must equal `j - i` there.
            if let (true, Decision::Exact(claim)) = (expected_exact, &d) {
                let c = cert.as_ref().expect("Exact must carry a certificate");
                let (wi, wj) = match &c.witness {
                    Witness::IntSolution { i, j } => (i[0], j[0]),
                    other => panic!("M0 Exact used a non-IntSolution witness: {:?}", other),
                };
                let bad = if !ivs.contains(&wi) || !ivs.contains(&wj) {
                    Some("witness on an iteration the loop never runs")
                } else if ip * wi + op_ != iq * wj + oq {
                    Some("witness locations do not collide")
                } else if !matches!(&c.claim, Claim::Exact(v) if v == &vec![wj - wi]) {
                    Some("claim does not match the witness distance")
                } else if claim != &vec![wj - wi] {
                    Some("decision and certificate disagree")
                } else {
                    None
                };
                if let Some(why) = bad {
                    unsound += 1;
                    if examples.len() < 10 {
                        examples.push(format!(
                            "witness i={} j={} {}: ip={} op={} iq={} oq={} step={}",
                            wi, wj, why, ip, op_, iq, oq, step
                        ));
                    }
                }
            }
        }

        println!("M0 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M0 fuzz found {} unsound", unsound);
    }
}
// ============================================================================
// Stepped loops
// ============================================================================

/// Decide a loop whose IVs step by more than 1, by normalising it to an
/// equivalent unit-step loop and mapping the answer back.
///
/// The normalisation is M3's: level `k` of the original runs
/// `lower, lower + s, ..., lower + (count - 1) * s` with `count` the number of
/// steps below `upper`, so the substitution `iv[k] = lower[k] + s[k] * k'` over
/// `k' in [0, count)` covers exactly the same executions. Under it each access
/// becomes `sum(index[k] * s[k] * k') + (offset + sum(index[k] * lower[k]))`,
/// so every coefficient is scaled by the step and the loop's own origin is
/// folded into the constant.
///
/// The substitution is affine and injective, so it maps a decision on the
/// normalised loop back to the original with no loss: `g(i, j) = i - j` is
/// unchanged, which is why the distance carries over as-is, and a witness
/// `(i', j')` maps back pointwise. A certificate is emitted on the **original**
/// loop, that being what the checker was handed, with the witness rewritten and
/// wrapped in `WithLoop` for the same reason M2a and M4 wrap theirs.
fn decide_stepped(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    // Every step must be positive. `validate` enforces this for a parsed loop,
    // but a step this level cannot normalise must not be silently treated as 1.
    if loop_.ivs.iter().any(|iv| iv.step <= 0) {
        return (
            Decision::Unknown("M0: IV step must be positive".into()),
            None,
        );
    }

    let mut ivs = Vec::with_capacity(loop_.depth);
    let mut scales = Vec::with_capacity(loop_.depth);
    for iv in &loop_.ivs {
        let count = match iv.upper.checked_sub(iv.lower) {
            Some(span) => match span.checked_add(iv.step - 1) {
                Some(v) => v / iv.step,
                None => return (Decision::Unknown("M0: IV range overflow".into()), None),
            },
            None => return (Decision::Unknown("M0: IV range overflow".into()), None),
        };
        ivs.push(IV::new(iv.name.clone(), 0, count.max(0), 1));
        scales.push(iv.step);
    }

    let mut body = loop_.body.clone();
    for a in &mut body {
        let mut base_shift: i64 = 0;
        for (k, c) in a.index.iter_mut().enumerate() {
            base_shift = match base_shift.checked_add(match c.checked_mul(loop_.ivs[k].lower) {
                Some(v) => v,
                None => return (Decision::Unknown("M0: index overflow".into()), None),
            }) {
                Some(v) => v,
                None => return (Decision::Unknown("M0: index overflow".into()), None),
            };
            *c = match c.checked_mul(scales[k]) {
                Some(v) => v,
                None => return (Decision::Unknown("M0: index overflow".into()), None),
            };
        }
        // A parametric offset has no concrete value to shift, so the
        // normalisation cannot be carried out and the level abstains rather
        // than dropping the shift.
        let off = match a.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M0: parametric offset".into()), None),
        };
        a.offset = match off.checked_add(base_shift) {
            Some(v) => OffsetValue::Const(v),
            None => return (Decision::Unknown("M0: offset overflow".into()), None),
        };
    }

    let normalized = Loop {
        depth: loop_.depth,
        ivs,
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,};

    // A chase loop is M5's to expand; its accesses are pointers into a chain,
    // not affine subscripts, so the normalisation above would be meaningless.
    if loop_.chase.is_some() {
        return (Decision::Unknown("M0: chase metadata".into()), None);
    }

    let (d, cert) = decide_dense(&normalized, p, q);
    let cert = cert.map(|mut c| {
        c.level = "M0".into();
        c.loop_hash = loop_.hash();
        c.witness = map_witness_back(&c.witness, &normalized, loop_);
        c.claim = map_claim_back(&c.claim, &scales);
        c
    });
    (map_decision_back(d, &scales), cert)
}

/// Scale a normalised-loop distance vector back to the original loop's step.
///
/// Under `iv = lower + step * iv'` the physical gap between two iterations is
/// the step times the normalised gap, so `j - i` on the original is
/// `step * (j' - i')`. Only `Exact` carries a concrete vector; every other
/// claim is step-independent and is returned untouched.
fn map_claim_back(claim: &Claim, scales: &[i64]) -> Claim {
    match claim {
        Claim::Exact(d) => Claim::Exact(
            d.iter()
                .enumerate()
                .map(|(k, &v)| v.checked_mul(scales[k]).unwrap_or(i64::MAX))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The same scaling applied to a bare [`Decision`].
fn map_decision_back(d: Decision, scales: &[i64]) -> Decision {
    match d {
        Decision::Exact(v) => Decision::Exact(
            v.iter()
                .enumerate()
                .map(|(k, &x)| x.checked_mul(scales[k]).unwrap_or(i64::MAX))
                .collect(),
        ),
        other => other,
    }
}

/// Translate a witness on the normalised unit-step loop back to the original
/// stepped one, level by level: `iv = lower + step * iv'`.
///
/// `WithLoop` and `UnsatProof` are carried through unchanged, since neither
/// names an iteration of the outer loop: the former is interpreted against the
/// loop it stores, and the latter asserts the absence of a collision rather
/// than exhibiting one.
fn map_witness_back(w: &Witness, normalized: &Loop, original: &Loop) -> Witness {
    let map = |values: &[i64]| -> Vec<i64> {
        values
            .iter()
            .enumerate()
            .map(|(k, &v)| {
                let step_v = original.ivs[k].step.checked_mul(v).unwrap_or(i64::MAX);
                original.ivs[k].lower.checked_add(step_v).unwrap_or(i64::MAX)
            })
            .collect()
    };
    match w {
        Witness::IntSolution { i, j } => Witness::IntSolution {
            i: map(i),
            j: map(j),
        },
        Witness::WithLoop { loop_, inner } => Witness::WithLoop {
            loop_: loop_.clone(),
            inner: Box::new(map_witness_back(inner, normalized, original)),
        },
        other => other.clone(),
    }
}
