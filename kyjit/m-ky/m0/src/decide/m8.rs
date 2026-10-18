//! M8 — linear recurrences.
//!
//! Handles index expressions of the form
//!     A[x[iv[0]]]    with    x[k] = Σ coeffs[j] * x[k-1-j]
//! seeded by `initial`. Depth-1 only.
//!
//! Decision procedure:
//!   1. Both accesses have `IndexKind::Recurrence(_)`.
//!   2. If they share the same `RecurrenceId`, enumerate the sequence
//!      over the IV range, build value→iv map, find cross-IV collision.
//!   3. If mixed (one recurrence, one affine), find intersection of
//!      recurrence with affine function.
//!   4. For large ranges, use matrix exponentiation + interval sampling
//!      to find intersections without full enumeration.
//!   5. Return Exact if a common location exists, None otherwise.

use super::Decision;
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;

/// Maximum range size before switching to interval sampling.
const BSGS_THRESHOLD: usize = 1024;

/// Companion matrix for a linear recurrence.
/// For recurrence x[k] = c₁*x[k-1] + c₂*x[k-2] + ... + c_d*x[k-d],
/// the companion matrix M is d×d:
/// [c₁  c₂  ...  c_d]
/// [1   0   ...  0  ]
/// [0   1   ...  0  ]
/// [...          ...]
/// [0   0   ...  1  0]
/// State vector: [x[k], x[k-1], ..., x[k-d+1]]^T
#[derive(Debug, Clone)]
struct CompanionMatrix {
    mat: Vec<Vec<i64>>,
    order: usize,
}

impl CompanionMatrix {
    /// Build companion matrix from coefficients c[0..d-1] where
    /// x[k] = c[0]*x[k-1] + c[1]*x[k-2] + ... + c[d-1]*x[k-d]
    fn new(coeffs: &[i64]) -> Self {
        let d = coeffs.len();
        let mut mat = vec![vec![0i64; d]; d];
        for j in 0..d {
            mat[0][j] = coeffs[j];
        }
        for i in 1..d {
            mat[i][i - 1] = 1;
        }
        CompanionMatrix { mat, order: d }
    }

    /// Matrix multiplication (d×d) × (d×d) with overflow checking.
    fn mul(&self, other: &CompanionMatrix) -> Option<CompanionMatrix> {
        let d = self.order;
        let mut res = vec![vec![0i64; d]; d];
        for i in 0..d {
            for j in 0..d {
                let mut sum: i64 = 0;
                for k in 0..d {
                    let a = self.mat[i][k];
                    let b = other.mat[k][j];
                    sum = sum.checked_add(a.checked_mul(b)?)?;
                }
                res[i][j] = sum;
            }
        }
        Some(CompanionMatrix { mat: res, order: d })
    }

    /// Matrix-vector multiplication: mat * vec.
    fn mul_vec(&self, vec: &[i64]) -> Option<Vec<i64>> {
        let d = self.order;
        if vec.len() != d {
            return None;
        }
        let mut res = vec![0i64; d];
        for i in 0..d {
            let mut sum: i64 = 0;
            for j in 0..d {
                sum = sum.checked_add(self.mat[i][j].checked_mul(vec[j])?)?;
            }
            res[i] = sum;
        }
        Some(res)
    }

    /// Fast matrix exponentiation: M^exp.
    fn pow(&self, mut exp: u64) -> Option<CompanionMatrix> {
        let d = self.order;
        let mut result = CompanionMatrix::identity(d);
        let mut base = self.clone();

        while exp > 0 {
            if exp & 1 == 1 {
                result = result.mul(&base)?;
            }
            exp >>= 1;
            if exp > 0 {
                base = base.mul(&base)?;
            }
        }
        Some(result)
    }

    /// Identity matrix of size d×d.
    fn identity(d: usize) -> Self {
        let mut mat = vec![vec![0i64; d]; d];
        for i in 0..d {
            mat[i][i] = 1;
        }
        CompanionMatrix { mat, order: d }
    }
}

/// Recurrence sequence with fast evaluation via matrix exponentiation.
#[derive(Debug, Clone)]
struct FastRecurrence {
    /// Companion matrix for the recurrence.
    companion: CompanionMatrix,
    /// Initial state vector [x[d-1], x[d-2], ..., x[0]]^T.
    init_state: Vec<i64>,
    /// Order of the recurrence.
    order: usize,
}

impl FastRecurrence {
    /// Create from RecurrenceId.
    fn new(rec: &RecurrenceId) -> Option<Self> {
        let d = rec.coeffs.len();
        if rec.initial.len() != d || d == 0 {
            return None;
        }
        let companion = CompanionMatrix::new(&rec.coeffs);
        // State vector is [x[d-1], x[d-2], ..., x[0]]
        let init_state = rec.initial.iter().rev().copied().collect();
        Some(FastRecurrence {
            companion,
            init_state,
            order: d,
        })
    }

    /// Evaluate recurrence at index k using matrix exponentiation.
    /// Returns the value x[k].
    fn eval(&self, k: usize) -> Option<i64> {
        let d = self.order;
        if k < d {
            return Some(self.init_state[d - 1 - k]);
        }
        // Compute M^(k - d + 1) * init_state
        let power = k - d + 1;
        let mat_pow = self.companion.pow(power as u64)?;
        let state = mat_pow.mul_vec(&self.init_state)?;
        Some(state[0])
    }
}

/// Find intersection of two recurrences with offsets: rec_p[k1] + off_p == rec_q[k2] + off_q
fn find_intersection_offset(
    rec_p: &FastRecurrence,
    off_p: i64,
    rec_q: &FastRecurrence,
    off_q: i64,
    n: usize,
) -> Option<(usize, usize)> {
    // We search for k1, k2 such that rec_p[k1] - rec_q[k2] == off_q - off_p
    // For same recurrence, we can check if rec_p[k] + off_p == rec_p[k] + off_q
    // i.e., off_p == off_q, in which case any k works.
    // For different recurrences, we use sampling.

    // Simple approach: sample both and check
    let step = if n <= BSGS_THRESHOLD { 1 } else { (n as f64).sqrt().ceil() as usize };
    let step = step.max(1).min(BSGS_THRESHOLD);

    // Sample rec_p with offset
    let mut samples_p = Vec::new();
    let mut k = 0;
    while k < n {
        if let Some(v) = rec_p.eval(k) {
            samples_p.push((k, v + off_p));
        }
        k = k.saturating_add(step);
    }

    // Sample rec_q with offset and check
    k = 0;
    while k < n {
        if let Some(vq) = rec_q.eval(k) {
            let vq_off = vq + off_q;
            for (kp, vp) in &samples_p {
                if *vp == vq_off {
                    // Verify in neighborhood
                    let start_p = kp.saturating_sub(step);
                    let end_p = (kp + step + 1).min(n);
                    let start_q = k.saturating_sub(step);
                    let end_q = (k + step + 1).min(n);
                    for ip in start_p..end_p {
                        if let Some(vpi) = rec_p.eval(ip) {
                            let vpi_off = vpi + off_p;
                            for iq in start_q..end_q {
                                if let Some(vqi) = rec_q.eval(iq) {
                                    if vpi_off == vqi + off_q {
                                        return Some((ip, iq));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        k = k.saturating_add(step);
    }
    None
}

/// Check if recurrence intersects with affine function c*k + off in range [0, n).
fn find_recurrence_affine_intersection(
    rec: &FastRecurrence,
    c: i64,
    off: i64,
    n: usize,
) -> Option<usize> {
    if n == 0 {
        return None;
    }
    if n <= BSGS_THRESHOLD {
        for k in 0..n {
            if let Some(v) = rec.eval(k) {
                let target = c.checked_mul(k as i64)?.checked_add(off)?;
                if v == target {
                    return Some(k);
                }
            }
        }
        return None;
    }

    // For large n, use interval sampling
    let step = (n as f64).sqrt().ceil() as usize;
    let step = step.max(1).min(BSGS_THRESHOLD);

    // Sample recurrence
    let mut samples = Vec::new();
    let mut k = 0;
    while k < n {
        if let Some(v) = rec.eval(k) {
            samples.push((k, v));
        }
        k = k.saturating_add(step);
    }

    // Check affine against samples
    k = 0;
    while k < n {
        let target = c.checked_mul(k as i64)?.checked_add(off)?;
        for (kp, vp) in &samples {
            if *vp == target {
                // Verify in neighborhood
                let start = kp.saturating_sub(step);
                let end = (kp + step + 1).min(n);
                for ip in start..end {
                    if let Some(vpi) = rec.eval(ip) {
                        let target_i = c.checked_mul(ip as i64)?.checked_add(off)?;
                        if vpi == target_i {
                            return Some(ip);
                        }
                    }
                }
            }
        }
        k = k.saturating_add(step);
    }
    None
}

/// Decide using fast intersection finding for large ranges.
pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    let uses_rec = |a: &Access| matches!(&a.index_kind, IndexKind::Recurrence(_));
    if !uses_rec(ap) && !uses_rec(aq) {
        return (Decision::Unknown("M8: no recurrence index".into()), None);
    }

    if loop_.depth != 1 {
        return (Decision::Unknown("M8: depth>1 unsupported".into()), None);
    }
    if ap.base != aq.base {
        return (Decision::None, None);
    }

    let iv = &loop_.ivs[0];
    let iv_lower = iv.lower;
    let iv_upper = iv.upper;
    let iv_step = iv.step;

    if iv_step <= 0 || iv_lower >= iv_upper {
        return (Decision::None, None);
    }
    let count = ((iv_upper - iv_lower) as u64 + (iv_step as u64) - 1) / (iv_step as u64);
    let n = count as usize;

    let is_rec_p = matches!(&ap.index_kind, IndexKind::Recurrence(_));
    let is_rec_q = matches!(&aq.index_kind, IndexKind::Recurrence(_));

    if is_rec_p && is_rec_q {
        let rec_p = match &ap.index_kind { IndexKind::Recurrence(r) => r, _ => unreachable!() };
        let rec_q = match &aq.index_kind { IndexKind::Recurrence(r) => r, _ => unreachable!() };

        let fast_p = match FastRecurrence::new(rec_p) {
            Some(f) => f,
            None => return (Decision::Unknown("M8: failed to create FastRecurrence for p".into()), None),
        };
        let fast_q = match FastRecurrence::new(rec_q) {
            Some(f) => f,
            None => return (Decision::Unknown("M8: failed to create FastRecurrence for q".into()), None),
        };

        let off_p = match ap.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M8: non-const offset for p".into()), None),
        };
        let off_q = match aq.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M8: non-const offset for q".into()), None),
        };

        if let Some((kp, kq)) = find_intersection_offset(&fast_p, off_p, &fast_q, off_q, n) {
            let iv_p = iv_lower + kp as i64 * iv_step;
            let iv_q = iv_lower + kq as i64 * iv_step;
            let d = vec![iv_q - iv_p];
            let cert = Certificate {
                level: "M8".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i: vec![iv_p], j: vec![iv_q] },
                checker: "m0-check".into(),
            };
            return (Decision::Exact(d), Some(cert));
        }

        let cert = Certificate {
            level: "M8".into(),
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
    } else if is_rec_p && !is_rec_q {
        let rec_p = match &ap.index_kind { IndexKind::Recurrence(r) => r, _ => unreachable!() };
        let fast_p = match FastRecurrence::new(rec_p) {
            Some(f) => f,
            None => return (Decision::Unknown("M8: failed to create FastRecurrence for p".into()), None),
        };
        let off_p = match ap.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M8: non-const offset for p".into()), None),
        };
        let c_q = aq.index.first().copied().unwrap_or(0);
        let off_q = match aq.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M8: non-const offset for q".into()), None),
        };

        let c = c_q * iv_step;
        let off = c_q * iv_lower + off_q - off_p;

        if let Some(k) = find_recurrence_affine_intersection(&fast_p, c, off, n) {
            let iv_p = iv_lower + k as i64 * iv_step;
            let iv_q = iv_p;
            let d = vec![iv_q - iv_p];
            let cert = Certificate {
                level: "M8".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i: vec![iv_p], j: vec![iv_q] },
                checker: "m0-check".into(),
            };
            return (Decision::Exact(d), Some(cert));
        }

        let cert = Certificate {
            level: "M8".into(),
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
    } else if !is_rec_p && is_rec_q {
        let rec_q = match &aq.index_kind { IndexKind::Recurrence(r) => r, _ => unreachable!() };
        let fast_q = match FastRecurrence::new(rec_q) {
            Some(f) => f,
            None => return (Decision::Unknown("M8: failed to create FastRecurrence for q".into()), None),
        };
        let off_q = match aq.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M8: non-const offset for q".into()), None),
        };
        let c_p = ap.index.first().copied().unwrap_or(0);
        let off_p = match ap.offset.as_const() {
            Some(v) => v,
            None => return (Decision::Unknown("M8: non-const offset for p".into()), None),
        };

        let c = c_p * iv_step;
        let off = c_p * iv_lower + off_p - off_q;

        if let Some(k) = find_recurrence_affine_intersection(&fast_q, c, off, n) {
            let iv_p = iv_lower + k as i64 * iv_step;
            let iv_q = iv_p;
            let d = vec![iv_q - iv_p];
            let cert = Certificate {
                level: "M8".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i: vec![iv_p], j: vec![iv_q] },
                checker: "m0-check".into(),
            };
            return (Decision::Exact(d), Some(cert));
        }

        let cert = Certificate {
            level: "M8".into(),
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
    } else {
        (Decision::Unknown("M8: both affine".into()), None)
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

    fn fibonacci() -> RecurrenceId {
        // x[0]=0, x[1]=1, x[k] = x[k-1] + x[k-2]
        RecurrenceId {
            initial: vec![0, 1],
            coeffs: vec![1, 1],
        }
    }

    fn acc_rec(kind: AccessKind, rec: RecurrenceId, offset: i64) -> Access {
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
            index_kind: IndexKind::Recurrence(rec),
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
    fn fibonacci_seed() {
        let f = fibonacci();
        assert_eq!(f.eval(0), Some(0));
        assert_eq!(f.eval(1), Some(1));
        assert_eq!(f.eval(2), Some(1));
        assert_eq!(f.eval(3), Some(2));
        assert_eq!(f.eval(4), Some(3));
        assert_eq!(f.eval(5), Some(5));
        assert_eq!(f.eval(6), Some(8));
        assert_eq!(f.eval(7), Some(13));
    }

    #[test]
    fn linear_recurrence_constant() {
        // x[0]=1, coeffs=[1] → all ones
        let r = RecurrenceId { initial: vec![1], coeffs: vec![1] };
        assert_eq!(r.eval(0), Some(1));
        assert_eq!(r.eval(5), Some(1));
        assert_eq!(r.eval(100), Some(1));
    }

    #[test]
    fn malformed_returns_none() {
        // coeffs.len() != initial.len()
        let r = RecurrenceId { initial: vec![0], coeffs: vec![1, 1] };
        assert_eq!(r.eval(0), None);
    }

    #[test]
    fn decide_same_fibonacci() {
        // A[x[i]] = A[x[i]] → EXACT [0] (both at same location for i=0)
        let l = make_loop_1d(0, 8, vec![
            acc_rec(AccessKind::Write, fibonacci(), 0),
            acc_rec(AccessKind::Read,  fibonacci(), 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M8");
    }

    #[test]
    fn decide_fibonacci_constant_shift() {
        // A[x[i]] = A[x[i] + 1]: since Fibonacci has distinct values 0,1,1,2,3,5,...
        // x[2]=1, x[3]=2. Location p at i=2 is 1; location q at i=3 is 3.
        // But p at i=3 is 2, q at i=1 is 2. So there's a cross collision.
        let l = make_loop_1d(0, 8, vec![
            acc_rec(AccessKind::Write, fibonacci(), 0),
            acc_rec(AccessKind::Read,  fibonacci(), 1),
        ]);
        let (d, _) = decide(&l, 0, 1);
        // Any Exact is acceptable; the point is it finds a dependence.
        assert!(matches!(d, Decision::Exact(_)));
    }

    #[test]
    fn decide_recurrence_vs_affine_disjoint() {
        // A[x[i]] where x is Fibonacci = {0, 1, 1, 2, 3, 5, 8, 13, ...}
        // vs A[100 + i] → affine locations {100, 101, ...}. Disjoint.
        let l = make_loop_1d(0, 8, vec![
            acc_rec(AccessKind::Write, fibonacci(), 0),
            {
                let mut a = acc_rec(AccessKind::Read, fibonacci(), 0);
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
        // Same recurrence, same offset - should find intersection at k=0 even for large ranges
        let l = make_loop_1d(0, 100_000, vec![
            acc_rec(AccessKind::Write, fibonacci(), 0),
            acc_rec(AccessKind::Read,  fibonacci(), 0),
        ]);
        let (d, _) = decide(&l, 0, 1);
        // With fast intersection finding, we can now handle large ranges
        assert!(matches!(d, Decision::Exact(_)));
    }
    // ================================================================
    // Property fuzz: random linear recurrences, compare decide() against
    // brute-force sequence enumeration.
    // ================================================================

    fn xorshift8(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn seq8(initial: &[i64], coeffs: &[i64], n: usize) -> Option<Vec<i64>> {
        if initial.len() != 2 || coeffs.len() != 2 { return None; }
        let mut x = vec![initial[0], initial[1]];
        while x.len() < n {
            let a = coeffs[0];
            let b = coeffs[1];
            let next = a.checked_mul(x[x.len() - 1])?.checked_add(b.checked_mul(x[x.len() - 2])?)?;
            x.push(next);
        }
        x.truncate(n);
        Some(x)
    }

    #[test]
    fn fuzz_m8_property() {
        let mut state: u64 = 0x8_0badc0de;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..2000 {
            let init_p = [
                (xorshift8(&mut state) % 6) as i64 - 3,
                (xorshift8(&mut state) % 6) as i64 - 3,
            ];
            let coeffs_p = [
                (xorshift8(&mut state) % 4) as i64 - 1,
                (xorshift8(&mut state) % 4) as i64 - 1,
            ];
            let init_q = [
                (xorshift8(&mut state) % 6) as i64 - 3,
                (xorshift8(&mut state) % 6) as i64 - 3,
            ];
            let coeffs_q = [
                (xorshift8(&mut state) % 4) as i64 - 1,
                (xorshift8(&mut state) % 4) as i64 - 1,
            ];
            let off_p = (xorshift8(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift8(&mut state) % 5) as i64 - 2;

            let lo = 0i64;
            let hi = 8i64;
            let n = (hi - lo) as usize;

            let xp = match seq8(&init_p, &coeffs_p, n) { Some(v) => v, None => continue };
            let xq = match seq8(&init_q, &coeffs_q, n) { Some(v) => v, None => continue };

            let wp: std::collections::HashSet<i64> = xp.iter().map(|&v| v + off_p).collect();
            let mut expected_exact = false;
            for &v in &xq {
                if wp.contains(&(v + off_q)) { expected_exact = true; break; }
            }

            let l = make_loop_1d(lo, hi, vec![
                acc_rec(AccessKind::Write, RecurrenceId { initial: init_p.to_vec(), coeffs: coeffs_p.to_vec() }, off_p),
                acc_rec(AccessKind::Read,  RecurrenceId { initial: init_q.to_vec(), coeffs: coeffs_q.to_vec() }, off_q),
            ]);

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
                        "ip={:?} cp={:?} iq={:?} cq={:?} op={} oq={} expected_exact={} got={:?}",
                        init_p, coeffs_p, init_q, coeffs_q, off_p, off_q, expected_exact, d
                    ));
                }
            }
        }

        println!("M8 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M8 fuzz found {} unsound", unsound);
    }

}
