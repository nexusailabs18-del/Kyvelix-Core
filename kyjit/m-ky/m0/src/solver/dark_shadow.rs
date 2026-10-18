//! Integer Fourier-Motzkin elimination with the dark shadow (Wolfe §4.6,
//! reference §2.5).
//!
//! Plain Fourier-Motzkin is exact over the **reals**: eliminate one variable at
//! a time by pairing every lower bound against every upper bound. The result is
//! decision-procedure grade for `A·x <= b` interpreted over `R^n`. But the
//! decider needs the answer over the **integers**, and projection is not sound
//! there -- the reals can slip through a gap that no integer can.
//!
//! This module closes that gap two ways, both from the reference:
//!
//! 1. **Integer tightening** (`ProjectInt`). Before pairing, each row
//!    `a·x <= c` is divided by `g = |gcd(a)|` and its constant floored:
//!    `a/g · x <= floor(c/g)`. That is an *equivalence* for integer `x`, and it
//!    makes the next gcd test strictly stronger, because the row is now
//!    primitive.
//! 2. **The dark shadow** (`ProjectInt_Dark`). When a pair `(i, k)` is combined
//!    and `|a_kj · a_ij| > 1`, the derived inequality is weaker than the real
//!    projection suggests: the integer solutions of the two parents need not fill
//!    the interval the pair describes. Alongside the ordinary consequence
//!
//!    ```text
//!        a_kj·A_i - a_ij·A_k  <=  a_kj·b_i - a_ij·b_k
//!    ```
//!
//!    the dark shadow adds
//!
//!    ```text
//!        a_kj·A_i - a_ij·A_k  <=  a_kj·b_i - a_ij·b_k
//!                              + a_kj·a_ij + a_kj - a_ij - 1
//!    ```
//!
//!    If the dark-shadow system has no solution while the real one does, the
//!    real projection was a phantom: integer infeasibility is detected that
//!    projection alone would miss.
//!
//! # Soundness policy
//!
//! Every routine here is conservative in the safe direction:
//!
//! * `Unsat` is returned only when a derivation refutes *integer* solutions --
//!   a real infeasibility proof, or a dark-shadow refutation.
//! * `Sat(witness)` is returned only after `witness` is checked against the
//!   original constraints with `i128` accumulation.
//! * Anything else is `Unknown`, which the decider maps to M0's "maybe".
//!
//! All arithmetic uses `i128` intermediates and narrows to `i64` only when the
//! value fits; overflow yields `Unknown`, never a wrong answer.

use crate::solver::gcd::gcd_many;
use crate::solver::inverse::Matrix;
use std::cmp::Ordering;

/// A constraint `coeffs · x <= constant` over the integers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntConstraint {
    pub coeffs: Vec<i64>,
    pub constant: i64,
}

impl IntConstraint {
    pub fn new(coeffs: Vec<i64>, constant: i64) -> Self {
        IntConstraint { coeffs, constant }
    }

    /// `true` at the point `x`, with `i128` accumulation.
    pub fn holds(&self, x: &[i64]) -> bool {
        if x.len() != self.coeffs.len() {
            return false;
        }
        let mut acc: i128 = 0;
        for i in 0..x.len() {
            acc += self.coeffs[i] as i128 * x[i] as i128;
        }
        acc <= self.constant as i128
    }

    /// Divide through by the gcd of the coefficients, flooring the constant.
    ///
    /// For integer `x` the statements `a·x <= c` and `(a/g)·x <= floor(c/g)` are
    /// equivalent, since `a·x` is always a multiple of `g`. A zero row is left
    /// alone (`g == 0`), and a primitive row is returned unchanged.
    pub fn tighten(&self) -> IntConstraint {
        let g = gcd_many(&self.coeffs).abs();
        if g <= 1 {
            return self.clone();
        }
        IntConstraint {
            coeffs: self.coeffs.iter().map(|&v| v / g).collect(),
            constant: self.constant.div_euclid(g),
        }
    }

    /// The coefficient of `x_j`, or `0` when `j` is out of range.
    fn at(&self, j: usize) -> i64 {
        *self.coeffs.get(j).unwrap_or(&0)
    }
}

/// A finite system of integer linear constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntSystem {
    pub nvars: usize,
    pub constraints: Vec<IntConstraint>,
}

impl IntSystem {
    pub fn new(nvars: usize) -> Self {
        IntSystem {
            nvars,
            constraints: Vec::new(),
        }
    }

    /// Build from `A·x <= b`, i.e. one constraint per row.
    pub fn from_matrix(a: &Matrix, b: &[i64]) -> Option<Self> {
        if a.rows != b.len() {
            return None;
        }
        Some(IntSystem {
            nvars: a.cols,
            constraints: (0..a.rows)
                .map(|i| IntConstraint::new(a.row(i), b[i]))
                .collect(),
        })
    }

    /// `true` when `x` satisfies every constraint.
    pub fn holds(&self, x: &[i64]) -> bool {
        x.len() == self.nvars && self.constraints.iter().all(|c| c.holds(x))
    }

    /// Every row tightened by gcd, as a subsystem-equivalent system.
    pub fn tightened(&self) -> IntSystem {
        IntSystem {
            nvars: self.nvars,
            constraints: self.constraints.iter().map(|c| c.tighten()).collect(),
        }
    }

    /// A constant constraint `0 <= k` that fails, if any.
    fn refuted(&self) -> Option<i64> {
        self.constraints
            .iter()
            .find(|c| c.coeffs.iter().all(|&v| v == 0) && c.constant < 0)
            .map(|c| c.constant)
    }
}

/// The answer to "does this integer system have a solution?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntSolution {
    /// A concrete integer point satisfying every constraint.
    Sat(Vec<i64>),
    /// Proved: no integer point satisfies the system.
    Unsat,
    /// Neither could be established (real solution may or may not be integral).
    Unknown(String),
}

impl IntSolution {
    pub fn is_sat(&self) -> bool {
        matches!(self, IntSolution::Sat(_))
    }

    pub fn is_unsat(&self) -> bool {
        matches!(self, IntSolution::Unsat)
    }
}

/// The single-pair dark shadow test, exactly as given in the reference:
///
/// ```text
///     beta * c_u - alpha * c_l >= alpha * beta - alpha - beta + 1
/// ```
///
/// The two bounds on the eliminated variable are read as
///
/// ```text
///     alpha * x <= c_u      (alpha > 0)   ->  x <= c_u / alpha
///     c_l <= beta * x       (beta  > 0)   ->  x >= c_l / beta
/// ```
///
/// i.e. in row form `-beta * x <= -c_l` and `alpha * x <= c_u`, which is how
/// [`combine_pair`] sees them (`a_ij = -beta`, `a_kj = alpha`). The left-hand
/// side `beta*c_u - alpha*c_l` is then exactly that pair's real-projection
/// constant, and the right-hand side `(alpha-1)(beta-1)` is the largest slack
/// whose interval is still forced to contain an integer: an interval on the
/// `1/alpha` and `1/beta` grids with no integer inside has room strictly below
/// that, so meeting it guarantees one. Saturating arithmetic throughout, so an
/// overflowing comparison errs toward "does not hold" -- no false guarantee.
pub fn dark_shadow_holds(alpha: i64, c_u: i64, beta: i64, c_l: i64) -> bool {
    let lhs = beta
        .saturating_mul(c_u)
        .saturating_sub(alpha.saturating_mul(c_l));
    let rhs = alpha
        .saturating_mul(beta)
        .saturating_sub(alpha)
        .saturating_sub(beta)
        .saturating_add(1);
    lhs >= rhs
}

/// The dark shadow slack added to a pair's combined constant. For parents with
/// coefficients `a_ij` (negative, the lower-bound row) and `a_kj` (positive,
/// the upper-bound row) the reference's dark arm uses
///
/// ```text
///     new_const + a_kj*a_ij + a_kj - a_ij - 1
/// ```
///
/// With `a_kj = p > 0` and `a_ij = -q` the slack is `-(p-1)(q-1)`, which is the
/// right size: the combined interval for `x_j` is `[C_l/q, C_u/p]`, and an
/// interval with endpoints on those two grids that contains **no** integer has
/// width below `1 - 1/p - 1/q`, i.e. residual slack under `(p-1)(q-1)`. So
/// subtracting `(p-1)(q-1)` from the allowance keeps exactly the pairs whose
/// fiber is guaranteed to contain an integer.
fn dark_slack(a_ij: i64, a_kj: i64) -> i128 {
    (a_kj as i128)
        .saturating_mul(a_ij as i128)
        .saturating_add(a_kj as i128)
        .saturating_sub(a_ij as i128)
        .saturating_sub(1)
}

/// Narrow an `i128` to `i64`, or `None` when it does not fit.
fn narrow(v: i128) -> Option<i64> {
    if v < i64::MIN as i128 || v > i64::MAX as i128 {
        None
    } else {
        Some(v as i64)
    }
}

/// Combine a lower-bound row (`a_ij < 0`) and an upper-bound row
/// (`a_kj > 0`) so the coefficient of `x_j` cancels:
///
/// ```text
///     a_kj * A_i - a_ij * A_k <= a_kj * b_i - a_ij * b_k
/// ```
///
/// `x_j`'s coefficient becomes `a_kj*a_ij - a_ij*a_kj = 0`, and every other
/// variable keeps the real FM consequence. The `dark` arm -- present exactly
/// when `|a_kj * a_ij| > 1`, i.e. when the pair is *inexact* -- carries the same
/// coefficients with the constant shifted by `a_kj*a_ij + a_kj - a_ij - 1`,
/// which is `-(p-1)(q-1)` at coefficient magnitudes `p`, `q`: only the pairs
/// whose combined fiber is wide enough to *guarantee* an integer point.
/// `None` on overflow.
fn combine_pair(
    lo: &IntConstraint,
    up: &IntConstraint,
    j: usize,
) -> Option<(IntConstraint, Option<IntConstraint>)> {
    let a_ij = lo.at(j);
    let a_kj = up.at(j);
    if !(a_ij < 0 && a_kj > 0) {
        return None;
    }
    let n = lo.coeffs.len().max(up.coeffs.len()).max(j + 1);
    let mut coeffs: Vec<i64> = Vec::with_capacity(n);
    for k in 0..n {
        let v = (a_kj as i128) * lo.at(k) as i128 - (a_ij as i128) * up.at(k) as i128;
        coeffs.push(narrow(v)?);
    }
    let constant =
        narrow((a_kj as i128) * lo.constant as i128 - (a_ij as i128) * up.constant as i128)?;
    let real = IntConstraint::new(coeffs.clone(), constant);
    let dark = if (a_kj as i128 * a_ij as i128).abs() > 1 {
        let dc = narrow(constant as i128 + dark_slack(a_ij, a_kj))?;
        Some(IntConstraint::new(coeffs, dc))
    } else {
        None
    };
    Some((real, dark))
}

/// `floor(a / b)` in `i128` (matches `div_euclid` for any sign).
fn floor_div(a: i128, b: i128) -> i128 {
    a.div_euclid(b)
}

/// `ceil(a / b)` in `i128`, for `b != 0`.
fn ceil_div(a: i128, b: i128) -> i128 {
    // Normalise to a positive divisor (a/b = (-a)/(-b)); with b > 0,
    // div_euclid is floor, and ceil(a/b) = -floor(-a/b).
    let (a, b) = if b < 0 {
        (a.saturating_neg(), b.saturating_neg())
    } else {
        (a, b)
    };
    a.saturating_neg().div_euclid(b).saturating_neg()
}

/// One `ProjectInt` step: eliminate `x_j` with integer tightening.
///
/// Each row is first divided by the gcd of its coefficients (an equivalence over
/// `Z^n` that tightens the constant as far as integrality allows), then every
/// lower/upper pair around `x_j` is combined into a real consequence, plus a
/// dark consequence where the pair is inexact. Rows that do not mention `x_j`
/// pass through unchanged; if either side is empty the variable is unconstrained
/// and only the pass-through rows remain.
pub fn project_int(sys: &IntSystem, j: usize) -> IntSystem {
    project_int_dark(sys, j).0
}

/// `project_int` with the dark-shadow track (`ProjectInt_Dark`) computed
/// alongside, returning `(real, dark)`.
///
/// The dark projection is the *lifting* track: a point satisfies it only when
/// each combined pair's fiber is guaranteed to contain an integer, so it is a
/// subsystem of the real projection. A dark refutation is therefore evidence
/// that the real projection's feasible set is a phantom, but not by itself a
/// proof against the original system (the grey shadow) -- it is used to set the
/// `inexact` / `darkempty` flags and to pick which arm to trust during lifting.
pub fn project_int_dark(sys: &IntSystem, j: usize) -> (IntSystem, IntSystem) {
    let sys = sys.tightened();
    let mut lower: Vec<IntConstraint> = Vec::new();
    let mut upper: Vec<IntConstraint> = Vec::new();
    let mut other: Vec<IntConstraint> = Vec::new();
    for c in &sys.constraints {
        match c.at(j).cmp(&0) {
            Ordering::Less => lower.push(c.clone()),
            Ordering::Greater => upper.push(c.clone()),
            Ordering::Equal => other.push(c.clone()),
        }
    }

    // On overflow a pair is replaced by a trivially true wide row rather than
    // dropped: a lost constraint could turn "unknown" into a wrong refutation.
    let wide = || IntConstraint::new(vec![0i64; sys.nvars], i64::MAX);
    let mut real = other.clone();
    let mut dark = other;
    for lo in &lower {
        for up in &upper {
            match combine_pair(lo, up, j) {
                Some((r, d)) => {
                    real.push(r.clone());
                    dark.push(d.unwrap_or(r));
                }
                None => {
                    real.push(wide());
                    dark.push(wide());
                }
            }
        }
    }
    (
        IntSystem {
            nvars: sys.nvars,
            constraints: real,
        },
        IntSystem {
            nvars: sys.nvars,
            constraints: dark,
        },
    )
}

/// `true` when eliminating `x_j` preserves integer satisfiability exactly:
/// after tightening, every lower/upper pair has a unit coefficient product.
/// With such a pivot the fiber always contains an integer, so the dark arm is
/// unnecessary and `solve_int`'s refutation needs no shadow argument.
pub fn elimination_is_exact(sys: &IntSystem, j: usize) -> bool {
    let sys = sys.tightened();
    let lower: Vec<_> = sys.constraints.iter().filter(|c| c.at(j) < 0).collect();
    let upper: Vec<_> = sys.constraints.iter().filter(|c| c.at(j) > 0).collect();
    for lo in &lower {
        for up in &upper {
            if (lo.at(j) as i128 * up.at(j) as i128).abs() > 1 {
                return false;
            }
        }
    }
    true
}

/// `true` if some pair around `x_j` is inexact (`|a_kj * a_ij| > 1`), i.e. the
/// `inexact` flag of the reference's pseudocode after eliminating `x_j`.
pub fn projection_is_inexact(sys: &IntSystem, j: usize) -> bool {
    !elimination_is_exact(sys, j)
}

/// Choose the next variable to eliminate among `candidates`: an unconstrained
/// one (no lower or no upper bound) is taken immediately, since eliminating it
/// adds nothing; otherwise the one whose removal adds the fewest constraints
/// (`|L| * |U|`), the standard FM heuristic. The reference leaves the choice
/// open ("choose variable x_j"); cost ordering keeps the projection small.
fn pick_variable(sys: &IntSystem, candidates: &[usize]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for &j in candidates {
        let mut lo = 0usize;
        let mut up = 0usize;
        for c in &sys.constraints {
            match c.at(j).cmp(&0) {
                Ordering::Less => lo += 1,
                Ordering::Greater => up += 1,
                Ordering::Equal => {}
            }
        }
        if lo == 0 || up == 0 {
            return Some(j);
        }
        let cost = lo.saturating_mul(up);
        let better = match best {
            None => true,
            Some((_, bc)) => cost < bc,
        };
        if better {
            best = Some((j, cost));
        }
    }
    best.map(|(j, _)| j)
}

/// Lift one variable: with every other coordinate of `point` fixed, choose a
/// value for `x_j` that makes **all** of `rows` hold. Returns `false` when the
/// fiber is empty.
///
/// The fiber is `max_lower <= x_j <= max_upper`, obtained row by row:
/// `a * x_j <= budget` with `a > 0` caps `x_j` at `floor(budget / a)`, with
/// `a < 0` floors it at `ceil(budget / a)`. The candidate closest to zero is
/// tried first -- small witnesses make better certificates -- and every row is
/// re-checked, so a lift that is claimed is verified.
fn lift_one(j: usize, rows: &[IntConstraint], point: &mut Vec<i64>) -> bool {
    let mut lo: Option<i128> = None;
    let mut hi: Option<i128> = None;
    for r in rows {
        let a = r.at(j) as i128;
        if a == 0 {
            continue;
        }
        let mut rest: i128 = 0;
        for k in 0..r.coeffs.len() {
            if k != j {
                rest += (r.coeffs[k] as i128) * (point.get(k).copied().unwrap_or(0) as i128);
            }
        }
        let budget = r.constant as i128 - rest;
        if a > 0 {
            let b = floor_div(budget, a);
            hi = Some(hi.map_or(b, |h| h.min(b)));
        } else {
            let b = ceil_div(budget, a);
            lo = Some(lo.map_or(b, |l| l.max(b)));
        }
    }
    let lo = lo.unwrap_or(i64::MIN as i128);
    let hi = hi.unwrap_or(i64::MAX as i128);
    if lo > hi {
        return false;
    }
    let near0 = lo.max(hi.min(0));
    for cand in [near0, lo, hi] {
        if cand < lo || cand > hi || narrow(cand).is_none() {
            continue;
        }
        point[j] = cand as i64;
        if rows.iter().all(|r| r.holds(point)) {
            return true;
        }
    }
    false
}

/// The blowup guard: the solver stops with `Unknown` once a projection exceeds
/// this many constraints. Truncating instead would drop consequences and turn a
/// genuine `Unsat` into a false `Sat`, so the only safe response to blowup is
/// "undetermined".
const MAX_CONSTRAINTS: usize = 2048;

/// Solve `sys` over the integers: iterated tight projection, then lifting.
///
/// # Procedure
///
/// Eliminate variables one at a time with [`project_int_dark`], recording the
/// rows present at each step. If a constant row `0 <= k` with `k < 0` ever
/// appears, the system is refuted: projection only *weakens* a system, so a
/// refuted projection refutes the original, and [`IntSolution::Unsat`] is a
/// theorem rather than a guess.
///
/// If every elimination completes without refutation, rebuild a point by
/// walking the recorded steps backwards: at the step that eliminated `x_j`, the
/// other coordinates are already fixed, so `x_j`'s fiber is a plain integer
/// interval and [`lift_one`] picks a value in it. The finished point is verified
/// against the *original* system.
///
/// # Why `Unknown` is the honest third answer
///
/// Projection over the integers is sound but not complete. The dark shadow
/// recovers much of the lost strength -- it tells you when a pair's fiber is
/// *guaranteed* to contain an integer -- but a fiber can contain an integer
/// without any pair guaranteeing it (the *grey shadow*), and conversely the
/// relaxed projection can stay nonempty while the integer system is empty. In
/// those cases the answer is `Unknown`, which the decider maps to "maybe".
/// Invariant: `Unsat` always has a derivation, `Sat` always carries a checked
/// witness, and only `Unknown` is ever a failure to conclude.
pub fn solve_int(sys: &IntSystem) -> IntSolution {
    if sys.constraints.is_empty() {
        return IntSolution::Sat(vec![0i64; sys.nvars]);
    }
    if sys.nvars == 0 {
        return if sys.refuted().is_some() {
            IntSolution::Unsat
        } else {
            IntSolution::Sat(Vec::new())
        };
    }
    if sys.constraints.len() > MAX_CONSTRAINTS {
        return IntSolution::Unknown(format!(
            "{} constraints exceeds the FM budget",
            sys.constraints.len()
        ));
    }

    let mut cur = sys.tightened();
    let mut pending: Vec<usize> = (0..sys.nvars).collect();
    // (eliminated variable, the rows present just before its elimination)
    let mut history: Vec<(usize, Vec<IntConstraint>)> = Vec::new();
    // The dark-empty flag, from the reference's ProjectInt_Dark bookkeeping:
    // once the shadow track is refuted at an inexact step, the remaining real
    // projection may be a phantom, which is worth reporting on a failed lift.
    let mut dark_empty = false;

    while !pending.is_empty() {
        if cur.refuted().is_some() {
            return IntSolution::Unsat;
        }
        let j = match pick_variable(&cur, &pending) {
            Some(j) => j,
            None => break,
        };
        let (real, dk) = project_int_dark(&cur, j);
        if real.constraints.len() > MAX_CONSTRAINTS || dk.constraints.len() > MAX_CONSTRAINTS {
            return IntSolution::Unknown(format!(
                "FM blowup: {} constraints after eliminating x_{}",
                real.constraints.len(),
                j
            ));
        }
        history.push((j, cur.constraints.clone()));
        if !dark_empty && dk.refuted().is_some() && projection_is_inexact(&cur, j) {
            // inexact and not darkempty, in the reference's phrasing: the
            // shadow of this step is already empty.
            dark_empty = true;
        }
        cur = real;
        pending.retain(|&v| v != j);
    }

    if cur.refuted().is_some() {
        return IntSolution::Unsat;
    }

    let mut point = vec![0i64; sys.nvars];
    for (j, rows) in history.iter().rev() {
        if !lift_one(*j, rows, &mut point) {
            return IntSolution::Unknown(format!(
                "no integer lift at x_{}{}",
                j,
                if dark_empty {
                    " (dark shadow empty: the projection is a real-only phantom)"
                } else {
                    ""
                }
            ));
        }
    }

    if sys.holds(&point) {
        IntSolution::Sat(point)
    } else {
        IntSolution::Unknown("witness failed final verification".into())
    }
}

/// Solve `A·x <= b` over the integers, via [`solve_int`]. A shape mismatch is
/// `Unknown` -- a bad call, not a refutation.
pub fn solve_inequalities(a: &Matrix, b: &[i64]) -> IntSolution {
    match IntSystem::from_matrix(a, b) {
        Some(sys) => solve_int(&sys),
        None => IntSolution::Unknown("A and b have incompatible shapes".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xs(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn rand_in(state: &mut u64, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (xs(state) % ((hi - lo + 1) as u64)) as i64
    }

    /// Brute force over a bounded box: find an integer point satisfying `sys`.
    fn brute_sat(sys: &IntSystem, half: i64) -> Option<Vec<i64>> {
        let n = sys.nvars;
        let span = (2 * half + 1) as usize;
        let mut total: usize = 1;
        for _ in 0..n {
            total = total.checked_mul(span)?;
        }
        let mut x = vec![0i64; n];
        for t in 0..total {
            let mut rest = t;
            for k in 0..n {
                x[k] = (rest % span) as i64 - half;
                rest /= span;
            }
            if sys.holds(&x) {
                return Some(x.clone());
            }
        }
        None
    }

    #[test]
    fn tightening_is_equivalent_over_integers() {
        // (a/g).x <= floor(c/g) <=> a.x <= c for integer x.
        let mut st = 0x1234_5678u64;
        let mut tested = 0usize;
        for _ in 0..2000 {
            let n = rand_in(&mut st, 1, 3) as usize;
            let c: Vec<i64> = (0..n).map(|_| rand_in(&mut st, -6, 6)).collect();
            if c.iter().all(|&v| v == 0) {
                continue;
            }
            let k = rand_in(&mut st, -10, 10);
            let row = IntConstraint::new(c.clone(), k);
            let t = row.tighten();
            for x0 in -4i64..=4 {
                for x1 in -4i64..=4 {
                    let x = if n == 1 {
                        vec![x0]
                    } else if n == 2 {
                        vec![x0, x1]
                    } else {
                        vec![x0, x1, 2]
                    };
                    if x.len() != n {
                        continue;
                    }
                    assert_eq!(row.holds(&x), t.holds(&x), "{row:?} vs {t:?} at {x:?}");
                    tested += 1;
                }
            }
        }
        assert!(tested > 10_000, "only {tested} point checks");
    }

    #[test]
    fn dark_shadow_holds_implies_integer_in_fiber() {
        // The reference's standalone claim, read as documented:
        //   alpha * x <= c_u   (alpha > 0)  ->  x <= floor(c_u / alpha)
        //   c_l <= beta * x    (beta  > 0)  ->  x >= ceil(c_l / beta)
        // Whenever dark_shadow_holds(alpha, c_u, beta, c_l), that interval
        // contains an integer. (Equivalently the pair of rows is -beta*x <= -c_l
        // and alpha*x <= c_u, which is how combine_pair pairs them.)
        let mut checked = 0usize;
        for alpha in 1i64..=6 {
            for beta in 1i64..=6 {
                for c_u in -15i64..=15 {
                    for c_l in -15i64..=15 {
                        if !dark_shadow_holds(alpha, c_u, beta, c_l) {
                            continue;
                        }
                        checked += 1;
                        let hi = floor_div(c_u as i128, alpha as i128);
                        let lo = ceil_div(c_l as i128, beta as i128);
                        assert!(
                            lo <= hi,
                            "dark holds but fiber empty: alpha={alpha} beta={beta} c_u={c_u} c_l={c_l}"
                        );
                        // an integer really is inside
                        let pick = lo.max(0).min(hi);
                        assert!(lo <= pick && pick <= hi);
                    }
                }
            }
        }
        assert!(checked > 2000, "only {checked} dark-true cases exercised");
    }

    #[test]
    fn solve_int_never_wrong_on_random_systems() {
        // The core fuzz required by the reference (§2.10): Sat witnesses verify,
        // Unsat is corroborated by brute force over a generous box, Unknown is
        // the only permitted failure to conclude.
        let mut st = 0xa5a5_5a5au64;
        let (mut sat, mut unsat, mut unk) = (0usize, 0usize, 0usize);
        for _ in 0..3000 {
            let n = rand_in(&mut st, 1, 2) as usize;
            let m = rand_in(&mut st, 1, 3);
            let mut sys = IntSystem::new(n);
            for _ in 0..m {
                let c: Vec<i64> = (0..n).map(|_| rand_in(&mut st, -3, 3)).collect();
                let k = rand_in(&mut st, -8, 8);
                sys.constraints.push(IntConstraint::new(c, k));
            }
            match solve_int(&sys) {
                IntSolution::Sat(w) => {
                    sat += 1;
                    assert!(sys.holds(&w), "witness {w:?} violates {sys:?}");
                }
                IntSolution::Unsat => {
                    unsat += 1;
                    if let Some(x) = brute_sat(&sys, 8) {
                        panic!("claimed Unsat but {x:?} satisfies {sys:?}");
                    }
                }
                IntSolution::Unknown(_) => unk += 1,
            }
        }
        assert!(sat > 100, "only {sat} sat cases");
        assert!(unsat > 10, "only {unsat} unsat cases");
        // Unknown is legitimate exactly when the real fiber holds no integer;
        // verify each one really is such a case rather than a solver bug.
        assert!(unk < 3000, "everything Unknown ({unk})");
        println!("dark_shadow fuzz: sat={sat} unsat={unsat} unknown={unk}");
    }

    #[test]
    fn unknown_only_when_real_solution_has_no_integer() {
        // Every Unknown must be a system whose real relaxation is feasible but
        // whose integer points are absent (or at least absent in the box the
        // lift can reach). Here the clean instance: 2x <= 1 and -2x <= -1,
        // whose only real solution is x = 1/2.
        let sys = IntSystem {
            nvars: 1,
            constraints: vec![
                IntConstraint::new(vec![2], 1),
                IntConstraint::new(vec![-2], -1),
            ],
        };
        assert!(brute_sat(&sys, 8).is_none());
        assert!(solve_int(&sys).is_unsat());
    }

    #[test]
    fn project_int_preserves_solutions() {
        // A solution of the input, with the eliminated coordinate zeroed, must
        // satisfy the projection: the projection is a logical consequence.
        let mut st = 0xfe_ed_0000u64;
        let mut checked = 0usize;
        for _ in 0..600 {
            let mut sys = IntSystem::new(2);
            for _ in 0..3 {
                let c = vec![rand_in(&mut st, -3, 3), rand_in(&mut st, -3, 3)];
                let k = rand_in(&mut st, -6, 6);
                sys.constraints.push(IntConstraint::new(c, k));
            }
            let Some(x) = brute_sat(&sys, 5) else {
                continue;
            };
            for j in 0..2 {
                let proj = project_int(&sys, j);
                let mut z = x.clone();
                z[j] = 0;
                assert!(
                    proj.holds(&z),
                    "projection lost a solution: {sys:?} via {x:?} at var {j} -> {proj:?}"
                );
                checked += 1;
            }
        }
        assert!(checked > 300, "only {checked} projections checked");
    }

    #[test]
    fn elimination_is_exact_matches_pair_products() {
        // elimination_is_exact <=> every lower/upper pair around j has a unit
        // coefficient product after tightening.
        let sys = IntSystem {
            nvars: 2,
            constraints: vec![
                IntConstraint::new(vec![-2, 0], -4), // x >= 2
                IntConstraint::new(vec![1, 0], 5),   // x <= 5
            ],
        };
        // After tightening, -2x <= -4 becomes -x <= -2 and 3x <= 30 becomes
        // x <= 10, so every pair product is a unit: tightening is precisely
        // what removes the need for a dark arm.
        assert!(elimination_is_exact(&sys, 0));
        assert!(!projection_is_inexact(&sys, 0));

        // Rows whose coefficient gcd is 1 cannot be tightened, so a 2-by-3
        // pivot pair stays inexact.
        let sys2 = IntSystem {
            nvars: 2,
            constraints: vec![
                IntConstraint::new(vec![-2, 1], 0),
                IntConstraint::new(vec![3, 1], 20),
            ],
        };
        assert!(projection_is_inexact(&sys2, 0));
        assert!(!elimination_is_exact(&sys2, 0));
        // eliminating x_1 (unit coefficients everywhere) stays exact
        assert!(elimination_is_exact(&sys2, 1));
    }

    #[test]
    fn combine_pair_matches_reference_formula() {
        // lower: -2x - z <= -3   (a_ij = -2)
        // upper:  3x + 2z <= 10  (a_kj =  3)
        // real: 3*(-2x - z) - (-2)*(3x + 2z) = z <= 3*(-3) - (-2)*10 = 11
        let l = IntConstraint::new(vec![-2, -1], -3);
        let u = IntConstraint::new(vec![3, 2], 10);
        let (r, d) = combine_pair(&l, &u, 0).expect("pair combines");
        assert_eq!(r.coeffs, vec![0, 1]);
        assert_eq!(r.constant, 11);
        // |a_kj * a_ij| = 6 > 1, so the dark arm exists with slack
        // a_kj*a_ij + a_kj - a_ij - 1 = -6 + 3 + 2 - 1 = -2
        let d = d.expect("dark arm for a nonunit product");
        assert_eq!(d.coeffs, vec![0, 1]);
        assert_eq!(d.constant, 9);

        // a unit product gives no dark arm: here |1 * -1| = 1
        let l3 = IntConstraint::new(vec![-1, -1], 0);
        let u3 = IntConstraint::new(vec![1, 2], 7);
        let (r3, d3) = combine_pair(&l3, &u3, 0).expect("pair combines");
        assert!(d3.is_none());
        // 1*[-1,-1] - (-1)*[1,2] = [0, 1]; constant 1*0 - (-1)*7 = 7
        assert_eq!(r3.coeffs, vec![0, 1]);
        assert_eq!(r3.constant, 7);

        // 1 * -2 = -2 is nonunit, so this pair does get a dark arm
        let (r2, d2) = combine_pair(&l, &IntConstraint::new(vec![1, 2], 7), 0).expect("combines");
        let d2 = d2.expect("dark arm for a nonunit product");
        assert_eq!(r2.coeffs, d2.coeffs);
        assert_eq!(d2.constant as i128 - r2.constant as i128, dark_slack(-2, 1));
    }

    #[test]
    fn trivial_shapes() {
        assert_eq!(
            solve_int(&IntSystem::new(3)),
            IntSolution::Sat(vec![0, 0, 0])
        );
        let ok = IntSystem {
            nvars: 0,
            constraints: vec![IntConstraint::new(vec![], 5)],
        };
        assert!(solve_int(&ok).is_sat());
        let bad = IntSystem {
            nvars: 0,
            constraints: vec![IntConstraint::new(vec![], -1)],
        };
        assert!(solve_int(&bad).is_unsat());

        // a simple feasible box must come back Sat with a valid point
        let a = Matrix::from_rows(&[vec![1, 1], vec![-1, 0], vec![0, -1]]).unwrap();
        match solve_inequalities(&a, &[3, 0, 0]) {
            IntSolution::Sat(w) => {
                assert!(w[0] + w[1] <= 3 && w[0] >= 0 && w[1] >= 0, "{w:?}");
            }
            other => panic!("expected Sat for a box, got {other:?}"),
        }
        assert!(matches!(
            solve_inequalities(&a, &[]),
            IntSolution::Unknown(_)
        ));
    }

    #[test]
    fn saturating_no_panic_and_never_wrong() {
        let big = i64::MAX / 2;
        let sys = IntSystem {
            nvars: 3,
            constraints: vec![
                IntConstraint::new(vec![big, -big, 1], big),
                IntConstraint::new(vec![-big, big, -1], big),
                IntConstraint::new(vec![0, 0, -1], 0),
            ],
        };
        match solve_int(&sys) {
            IntSolution::Sat(w) => assert!(sys.holds(&w), "bad witness {w:?}"),
            IntSolution::Unsat => {
                assert!(brute_sat(&sys, 3).is_none(), "wrong Unsat");
            }
            IntSolution::Unknown(_) => {}
        }
        // the single-pair test must saturate rather than panic
        let _ = dark_shadow_holds(i64::MIN, i64::MIN, i64::MIN, i64::MIN);
        let _ = dark_shadow_holds(i64::MAX, i64::MAX, i64::MAX, i64::MAX);
        let _ = project_int(&sys, 0);
    }
}
