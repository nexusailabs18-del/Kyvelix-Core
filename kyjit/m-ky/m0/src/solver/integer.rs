//! General integer solver (Wolfe §4.3, §4.4): solve `A·x = b` over `Z`.
//!
//! This is the module that turns the Hermite machinery of [`crate::solver::hnf`]
//! into answers the decision levels can use. The route is the classical one:
//!
//! 1. **Feasibility over `Q`.** `A·x = b` has a rational solution iff
//!    `rank(A) == rank(A|b)`. A strict inequality here is a *proof* of
//!    `NoSolution` -- there is no integer solution either, and no guessing
//!    was involved.
//! 2. **Full row rank.** Dependent rows are dropped, leaving `A'` with
//!    `rank(A') == rank(A)`. Dropping them is only legitimate because step 1
//!    already established that every dropped equation is implied by the
//!    retained ones.
//! 3. **Hermite.** `A'·U = H` with `H` lower triangular and positive
//!    diagonal, so `A'·x = b'` becomes `H·y = b'` under `x = U·y`. Forward
//!    substitution divides exactly or fails, and *failure is decisive*: `H`
//!    is a canonical basis of the lattice `L(A')`, so `b'` misses the lattice
//!    iff some division misses. That is the gcd test of [`crate::solver::gcd`]
//!    generalised from one equation to `n`.
//! 4. **Back out.** `x_0 = U·y_0` is a particular solution and the columns of
//!    `U` past the last pivot are a `Z`-basis of the nullspace, so the answer
//!    is the complete family `x_0 + Σ t_i·basis_i`, not a lone sample.
//!
//! Every arithmetic step is exact: `i128` intermediates, range checks, and
//! `Unknown` (never a wrong answer) when a value will not fit in `i64`.
//!
//! # Inequalities
//!
//! [`solve_integer_inequalities`] delegates to
//! [`crate::solver::dark_shadow`], the integer Fourier-Motzkin engine, because
//! that is the module whose whole design is `A·x <= b`.
//! [`solve_integer_mixed`] is where the two meet: the equations are solved
//! first, in integers, giving the complete family `x = x0 + V·t`; substituting
//! that family into `C·x <= d` produces `(C·V)·t <= d - C·x0`, an *equivalent*
//! integer system in `t`. Integrality is therefore settled by HNF before any
//! projection is attempted -- the hard half of the mixed problem is handled
//! exactly, and only the inequality half inherits dark shadow's sound-but-not-
//! complete projection.

use crate::solver::hnf::hermite;
use crate::solver::inverse::{invert_rational, rank_checked, Matrix};

/// The outcome of an integer solve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntSolveResult {
    /// Solutions exist. `particular` is one of them and `nullspace` is a
    /// `Z`-basis of `{x : A·x = 0}` when the system is a pure equation set,
    /// so the full solution family is
    /// `x = particular + Σ t_i · nullspace[i]`, `t ∈ Z`.
    Solutions {
        particular: Vec<i64>,
        nullspace: Vec<Vec<i64>>,
    },
    /// Proven: no integer solution exists.
    NoSolution,
    /// Not decided -- an intermediate value left `i64`, a rank test
    /// overflowed, or a projection stayed non-empty. Never means "probably no
    /// solution".
    Unknown,
}

impl IntSolveResult {
    /// `true` iff the solve produced a witness.
    pub fn is_sat(&self) -> bool {
        matches!(self, IntSolveResult::Solutions { .. })
    }

    /// The witness, if any.
    pub fn witness(&self) -> Option<&Vec<i64>> {
        match self {
            IntSolveResult::Solutions { particular, .. } => Some(particular),
            _ => None,
        }
    }
}

/// Solve `A·x = b` over the integers, returning one particular solution and a
/// `Z`-basis of the nullspace.
///
/// `Unknown` on shape mismatch, on `i128` overflow inside rank or HNF, or if a
/// produced coefficient leaves `i64`. `NoSolution` only when it is proven.
pub fn solve_integer_system(a: &Matrix, b: &[i64]) -> IntSolveResult {
    let m = a.cols;
    if a.rows != b.len() {
        return IntSolveResult::Unknown;
    }

    // Step 1: feasibility over Q. rank(A) == rank(A|b) is exact.
    let rank_a = match rank_checked(a) {
        Some(r) => r,
        None => return IntSolveResult::Unknown,
    };
    let aug = match augmented(a, b) {
        Some(v) => v,
        None => return IntSolveResult::Unknown,
    };
    let rank_aug = match rank_checked(&aug) {
        Some(r) => r,
        None => return IntSolveResult::Unknown,
    };
    if rank_aug != rank_a {
        return IntSolveResult::NoSolution;
    }

    // Rank 0: A is all zero, b is all zero (else the rank test above caught
    // it), and every x solves the system.
    if rank_a == 0 {
        return IntSolveResult::Solutions {
            particular: vec![0i64; m],
            nullspace: (0..m).map(|i| unit_vector(m, i)).collect(),
        };
    }

    // Step 2: keep a maximal independent set of rows. Legitimate only because
    // step 1 proved the dropped rows are implied.
    let rows = match independent_rows(a, rank_a) {
        Some(v) => v,
        None => return IntSolveResult::Unknown,
    };
    let picked: Vec<Vec<i64>> = rows.iter().map(|&i| a.row(i)).collect();
    let a_p = match Matrix::from_rows(&picked) {
        Some(v) => v,
        None => return IntSolveResult::Unknown,
    };
    let b_p: Vec<i64> = rows.iter().map(|&i| b[i]).collect();

    // Step 3: Hermite core on the full-row-rank system.
    solve_with_hnf(&a_p, &b_p)
}

/// The HNF core of [`solve_integer_system`], for a system the caller has
/// already established has full row rank (`a.rows <= a.cols`).
///
/// Split out so the divisibility test -- which is decisive *only* for a
/// full-row-rank system -- cannot be reached with a rank-deficient matrix.
pub fn solve_with_hnf(a: &Matrix, b: &[i64]) -> IntSolveResult {
    let n = a.rows;
    let m = a.cols;
    if b.len() != n || n > m {
        return IntSolveResult::Unknown;
    }

    let (h, u) = match hermite(a) {
        Some(v) => v,
        None => {
            // Fallback for square matrices: use rational inverse to find a
            // rational solution, then check if it's integral.
            if n == m {
                if let Some((num, den)) = invert_rational(a) {
                    // x = A^-1 * b = (num/den) * b
                    // Check if each component is integral
                    let mut particular = Vec::with_capacity(n);
                    for i in 0..n {
                        let mut acc: i128 = 0;
                        for j in 0..n {
                            // num[i][j] / den[i][j] * b[j]
                            // = (num[i][j] * b[j]) / den[i][j]
                            let num_val = num.get(i, j) as i128 * b[j] as i128;
                            let den_val = den.get(i, j) as i128;
                            if den_val == 0 || num_val % den_val != 0 {
                                return IntSolveResult::Unknown;
                            }
                            let val = num_val / den_val;
                            if val < i64::MIN as i128 || val > i64::MAX as i128 {
                                return IntSolveResult::Unknown;
                            }
                            particular.push(val as i64);
                        }
                    }
                    // Verify the solution
                    if verifies(a, &particular, b) {
                        return IntSolveResult::Solutions {
                            particular,
                            nullspace: Vec::new(),
                        };
                    }
                }
            }
            return IntSolveResult::Unknown;
        }
    };

    // Forward substitution on the lower-triangular H, in i128.
    let mut y = vec![0i128; m];
    for i in 0..n {
        let mut acc = b[i] as i128;
        for j in 0..i {
            acc -= h.get(i, j) as i128 * y[j];
        }
        let piv = h.get(i, i) as i128;
        if piv <= 0 {
            return IntSolveResult::Unknown;
        }
        if acc % piv != 0 {
            // H's diagonal is a basis of the lattice L(A), so a missed
            // division proves b is not in L(A): no integer solution.
            return IntSolveResult::NoSolution;
        }
        y[i] = acc / piv;
    }

    // x = U·y, with the free coordinates of y left at zero.
    let particular = match mat_vec_i64(&u, &y) {
        Some(v) => v,
        None => return IntSolveResult::Unknown,
    };

    // Columns past the last pivot of U span the integer nullspace of A.
    let nullspace: Vec<Vec<i64>> = if n == m {
        Vec::new()
    } else {
        (n..m).map(|j| u.col(j)).collect()
    };

    IntSolveResult::Solutions {
        particular,
        nullspace,
    }
}

/// Build `[A | b]`.
fn augmented(a: &Matrix, b: &[i64]) -> Option<Matrix> {
    if a.rows != b.len() {
        return None;
    }
    let mut out = Matrix::new(a.rows, a.cols + 1);
    for i in 0..a.rows {
        for j in 0..a.cols {
            out.set(i, j, a.get(i, j));
        }
        out.set(i, a.cols, b[i]);
    }
    Some(out)
}

/// Indices of a maximal independent set of rows of `A`, chosen greedily by
/// exact rank. `want` is the rank to reach (the rank of `A`). `None` if any
/// rank test overflows -- we refuse rather than risk retaining a dependent
/// row, which would break the decisiveness of the divisibility test.
fn independent_rows(a: &Matrix, want: usize) -> Option<Vec<usize>> {
    let mut picked: Vec<usize> = Vec::with_capacity(want);
    let mut cur = 0usize;
    for i in 0..a.rows {
        if cur == want {
            break;
        }
        let mut rows: Vec<Vec<i64>> = picked.iter().map(|&k| a.row(k)).collect();
        rows.push(a.row(i));
        let cand = Matrix::from_rows(&rows)?;
        match rank_checked(&cand) {
            Some(r) if r > cur => {
                cur = r;
                picked.push(i);
            }
            Some(_) => {}
            None => return None,
        }
    }
    if cur == want {
        Some(picked)
    } else {
        None
    }
}

/// The `i`-th standard basis vector of `Z^n`.
fn unit_vector(n: usize, i: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    v[i] = 1;
    v
}

/// `A·x` with `i128` accumulation, narrowed back to `i64`; `None` if an entry
/// does not fit.
fn mat_vec_i64(a: &Matrix, x: &[i128]) -> Option<Vec<i64>> {
    let mut out = Vec::with_capacity(a.rows);
    for i in 0..a.rows {
        let mut acc: i128 = 0;
        for j in 0..a.cols {
            acc += a.get(i, j) as i128 * x[j];
        }
        if acc < i64::MIN as i128 || acc > i64::MAX as i128 {
            return None;
        }
        out.push(acc as i64);
    }
    Some(out)
}

/// Check `A·x == b` exactly. Lets a caller re-validate a returned witness
/// independently of the solver that produced it.
pub fn verifies(a: &Matrix, x: &[i64], b: &[i64]) -> bool {
    if x.len() != a.cols || b.len() != a.rows {
        return false;
    }
    let xf: Vec<i128> = x.iter().map(|&v| v as i128).collect();
    match mat_vec_i64(a, &xf) {
        Some(prod) => prod.iter().zip(b.iter()).all(|(&p, &q)| p == q),
        None => false,
    }
}

/// Check `A·x <= b` exactly (`i128` accumulation, so no wrap hides a
/// violation).
pub fn satisfies_inequalities(a: &Matrix, x: &[i64], b: &[i64]) -> bool {
    if x.len() != a.cols || b.len() != a.rows {
        return false;
    }
    for i in 0..a.rows {
        let mut acc: i128 = 0;
        for j in 0..a.cols {
            acc += a.get(i, j) as i128 * x[j] as i128;
        }
        if acc > b[i] as i128 {
            return false;
        }
    }
    true
}

use crate::solver::dark_shadow::{solve_int, IntConstraint, IntSolution, IntSystem};

/// Narrow an `i128` to `i64`, refusing instead of wrapping.
fn narrow_i64(v: i128) -> Option<i64> {
    if v < i64::MIN as i128 || v > i64::MAX as i128 {
        None
    } else {
        Some(v as i64)
    }
}

/// Solve `A·x <= b` over the integers.
///
/// This delegates to the integer Fourier-Motzkin engine of
/// [`crate::solver::dark_shadow`], which is the module built for inequalities:
/// gcd tightening, projection with the dark-shadow arm, then lifting back to a
/// checked witness. `nullspace` is always empty for an inequality system --
/// its solution set is not a coset of a lattice, so there is no basis to
/// report -- and `Solutions` carries only the verified point.
pub fn solve_integer_inequalities(a: &Matrix, b: &[i64]) -> IntSolveResult {
    let sys = match IntSystem::from_matrix(a, b) {
        Some(s) => s,
        None => return IntSolveResult::Unknown,
    };
    match solve_int(&sys) {
        IntSolution::Sat(w) => {
            // Belt and braces: never hand back a witness we have not checked.
            if satisfies_inequalities(a, &w, b) {
                IntSolveResult::Solutions {
                    particular: w,
                    nullspace: Vec::new(),
                }
            } else {
                IntSolveResult::Unknown
            }
        }
        IntSolution::Unsat => IntSolveResult::NoSolution,
        IntSolution::Unknown(_) => IntSolveResult::Unknown,
    }
}

/// Solve the mixed system `A·x = b`, `C·x <= d` over the integers.
///
/// # Why this is exact, not a heuristic
///
/// [`solve_integer_system`] returns the *complete* integer family of the
/// equations,
///
/// ```text
///     x = x0 + t_1 v_1 + ... + t_k v_k,     t ∈ Z^k
/// ```
///
/// so substituting it into the inequalities turns `C·x <= d` into a system in
/// `t` alone,
///
/// ```text
///     (C·V)·t <= d - C·x0
/// ```
///
/// This substitution is an equivalence over the integers: every integer `t`
/// solving it yields an integer solution `x` of the mixed system, and every
/// integer solution of the mixed system arises from exactly one such `t`
/// (because the `v_i` are a `Z`-basis of the nullspace, not merely a
/// `Q`-basis). Integrality is therefore already settled by HNF before the
/// projection is even attempted, and the only incompleteness left is the one
/// dark shadow has by construction.
///
/// Consequences for the answers: `NoSolution` means genuinely proven infeasible
/// (the equations are unsolvable, or the substituted inequalities are refuted),
/// and `Solutions` always carries a point verified against both halves.
pub fn solve_integer_mixed(a: &Matrix, b: &[i64], c: &Matrix, d: &[i64]) -> IntSolveResult {
    if c.rows != d.len() || a.cols != c.cols || a.rows != b.len() {
        return IntSolveResult::Unknown;
    }
    let (x0, basis) = match solve_integer_system(a, b) {
        IntSolveResult::Solutions {
            particular,
            nullspace,
        } => (particular, nullspace),
        IntSolveResult::NoSolution => return IntSolveResult::NoSolution,
        IntSolveResult::Unknown => return IntSolveResult::Unknown,
    };

    // No free parameters: the lone candidate either satisfies the inequalities
    // or the mixed system is proven infeasible -- no projection needed.
    if basis.is_empty() {
        return if satisfies_inequalities(c, &x0, d) {
            IntSolveResult::Solutions {
                particular: x0,
                nullspace: Vec::new(),
            }
        } else {
            IntSolveResult::NoSolution
        };
    }

    // (C·V)·t <= d - C·x0, in i128 then narrowed.
    let k = basis.len();
    let mut constraints = Vec::with_capacity(c.rows);
    for i in 0..c.rows {
        let mut cx: i128 = 0;
        for j in 0..c.cols {
            cx += c.get(i, j) as i128 * x0[j] as i128;
        }
        let mut row: Vec<i64> = Vec::with_capacity(k);
        for v in &basis {
            let mut acc: i128 = 0;
            for j in 0..c.cols {
                acc += c.get(i, j) as i128 * v[j] as i128;
            }
            match narrow_i64(acc) {
                Some(w) => row.push(w),
                None => return IntSolveResult::Unknown,
            }
        }
        let need = match narrow_i64(d[i] as i128 - cx) {
            Some(w) => w,
            None => return IntSolveResult::Unknown,
        };
        constraints.push(IntConstraint::new(row, need));
    }
    let sys = IntSystem {
        nvars: k,
        constraints,
    };

    match solve_int(&sys) {
        IntSolution::Sat(t) => {
            let mut x = x0.clone();
            for (idx, tv) in t.iter().enumerate() {
                let v = &basis[idx];
                for j in 0..x.len() {
                    let nx = x[j] as i128 + v[j] as i128 * *tv as i128;
                    match narrow_i64(nx) {
                        Some(w) => x[j] = w,
                        None => return IntSolveResult::Unknown,
                    }
                }
            }
            if verifies(a, &x, b) && satisfies_inequalities(c, &x, d) {
                IntSolveResult::Solutions {
                    particular: x,
                    nullspace: Vec::new(),
                }
            } else {
                IntSolveResult::Unknown
            }
        }
        IntSolution::Unsat => IntSolveResult::NoSolution,
        IntSolution::Unknown(_) => IntSolveResult::Unknown,
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

    fn mat(rows: &[Vec<i64>]) -> Matrix {
        Matrix::from_rows(rows).unwrap()
    }

    /// Brute force over a bounded box: is there an integer x with A·x == b?
    fn brute_has_solution(a: &Matrix, b: &[i64], half: i64) -> Option<Vec<i64>> {
        let n = a.cols;
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
            if verifies(a, &x, b) {
                return Some(x);
            }
        }
        None
    }

    #[test]
    fn unique_solution_square_full_rank() {
        // 2x + y = 5, x - y = 1  ->  x = 2, y = 1
        let a = mat(&[vec![2, 1], vec![1, -1]]);
        let b = [5, 1];
        match solve_integer_system(&a, &b) {
            IntSolveResult::Solutions {
                particular,
                nullspace,
            } => {
                assert_eq!(particular, vec![2, 1]);
                assert!(nullspace.is_empty(), "square full rank has no kernel");
                assert!(verifies(&a, &particular, &b));
            }
            other => panic!("expected Solutions, got {other:?}"),
        }
    }

    #[test]
    fn no_integer_solution_is_proven() {
        // Rationally inconsistent: x + y = 1, 2x + 2y = 3.
        let a = mat(&[vec![1, 1], vec![2, 2]]);
        assert_eq!(
            solve_integer_system(&a, &[1, 3]),
            IntSolveResult::NoSolution
        );

        // Rationally fine, integrally impossible: 2x = 1.
        let a2 = mat(&[vec![2]]);
        assert_eq!(solve_integer_system(&a2, &[1]), IntSolveResult::NoSolution);
        assert!(brute_has_solution(&a2, &[1], 8).is_none());
    }

    #[test]
    fn zero_system_gives_identity_nullspace() {
        let a = mat(&[vec![0, 0], vec![0, 0]]);
        match solve_integer_system(&a, &[0, 0]) {
            IntSolveResult::Solutions {
                particular,
                nullspace,
            } => {
                assert_eq!(particular, vec![0, 0]);
                assert_eq!(nullspace, vec![vec![1, 0], vec![0, 1]]);
            }
            other => panic!("{other:?}"),
        }
        let b = mat(&[vec![0, 0]]);
        assert_eq!(solve_integer_system(&b, &[7]), IntSolveResult::NoSolution);
    }

    #[test]
    fn rank_deficient_but_consistent_is_decided() {
        // The risk register calls a missed rank-deficient case a bug: dependent
        // but consistent rows must still produce Solutions.
        let a = mat(&[vec![1, 2], vec![2, 4]]);
        let b = [5, 10];
        match solve_integer_system(&a, &b) {
            IntSolveResult::Solutions {
                particular,
                nullspace,
            } => {
                assert!(verifies(&a, &particular, &b));
                assert_eq!(nullspace.len(), 1, "2 vars, rank 1 -> 1 kernel vector");
                for v in &nullspace {
                    assert_eq!(a.get(0, 0) * v[0] + a.get(0, 1) * v[1], 0);
                }
            }
            other => panic!("expected Solutions, got {other:?}"),
        }
    }

    #[test]
    fn shape_mismatch_is_unknown() {
        let a = mat(&[vec![1, 2]]);
        assert_eq!(solve_integer_system(&a, &[1, 2]), IntSolveResult::Unknown);
        assert_eq!(solve_with_hnf(&a, &[]), IntSolveResult::Unknown);
        // over-determined but consistent
        let tall = mat(&[vec![1], vec![1], vec![1]]);
        match solve_integer_system(&tall, &[2, 2, 2]) {
            IntSolveResult::Solutions {
                particular,
                nullspace,
            } => {
                assert_eq!(particular, vec![2]);
                assert!(nullspace.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            solve_integer_system(&tall, &[2, 2, 3]),
            IntSolveResult::NoSolution
        );
    }

    #[test]
    fn fuzz_witness_and_nosolution_never_lie() {
        // The reference's fuzz strategy for integer.rs: random (A, b); verify
        // every solution, and corroborate every NoSolution with brute force.
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        let (mut sat, mut none, mut unk) = (0usize, 0usize, 0usize);
        for _ in 0..3000 {
            let m = rand_in(&mut st, 1, 3) as usize;
            let n = rand_in(&mut st, 1, 3) as usize;
            let a = mat(&(0..m)
                .map(|_| {
                    (0..n)
                        .map(|_| rand_in(&mut st, -3, 3))
                        .collect::<Vec<i64>>()
                })
                .collect::<Vec<_>>());
            let b: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -6, 6)).collect();
            match solve_integer_system(&a, &b) {
                IntSolveResult::Solutions {
                    particular,
                    nullspace,
                } => {
                    sat += 1;
                    assert!(verifies(&a, &particular, &b), "bad witness {particular:?}");
                    assert_eq!(particular.len(), n);
                    for v in &nullspace {
                        let mut col = vec![0i64; m];
                        for j in 0..n {
                            for i in 0..m {
                                col[i] += a.get(i, j) * v[j];
                            }
                        }
                        assert!(col.iter().all(|&x| x == 0), "{v:?} not in kernel");
                    }
                }
                IntSolveResult::NoSolution => {
                    none += 1;
                    if let Some(x) = brute_has_solution(&a, &b, 6) {
                        panic!("claimed NoSolution but {x:?} solves {a:?} = {b:?}");
                    }
                }
                IntSolveResult::Unknown => unk += 1,
            }
        }
        assert!(sat > 100 && none > 100, "sat={sat} none={none} unk={unk}");
        assert_eq!(unk, 0, "small inputs must all be decided, got {unk}");
        println!("integer fuzz: sat={sat} no_solution={none}");
    }

    /// Every integer solution inside a box.
    fn brute_solutions(a: &Matrix, b: &[i64], half: i64) -> Vec<Vec<i64>> {
        let n = a.cols;
        let span = 2 * half + 1;
        let mut out = Vec::new();
        let mut x = vec![0i64; n];
        let total: i64 = span.pow(n as u32);
        for t in 0..total {
            let mut rest = t;
            for k in 0..n {
                x[k] = (rest % span) - half;
                rest /= span;
            }
            if verifies(a, &x, b) {
                out.push(x.clone());
            }
        }
        out
    }

    /// Is `d` an integer combination of `basis`?
    fn in_span(basis: &[Vec<i64>], d: &[i64]) -> bool {
        let k = basis.len();
        if k == 0 {
            return d.iter().all(|&v| v == 0);
        }
        let rng = -12i64..=12;
        if k == 1 {
            return rng
                .clone()
                .any(|t| (0..d.len()).all(|j| basis[0][j] * t == d[j]));
        }
        for t0 in rng.clone() {
            for t1 in rng.clone() {
                if (0..d.len()).all(|j| basis[0][j] * t0 + basis[1][j] * t1 == d[j]) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn nullspace_basis_is_complete() {
        // x0 + Z·basis must contain *every* integer solution, which only holds
        // if the basis is a Z-basis rather than merely a Q-basis.
        let a = mat(&[vec![1, 1, 1]]);
        let b = [2];
        let (x0, basis) = match solve_integer_system(&a, &b) {
            IntSolveResult::Solutions {
                particular,
                nullspace,
            } => (particular, nullspace),
            other => panic!("{other:?}"),
        };
        assert_eq!(basis.len(), 2);
        let all = brute_solutions(&a, &b, 3);
        assert!(all.len() > 10, "expected a rich solution set, got {all:?}");
        for s in all {
            let d: Vec<i64> = (0..3).map(|j| s[j] - x0[j]).collect();
            assert!(
                in_span(&basis, &d),
                "solution {s:?} (delta {d:?}) outside the family {basis:?} at {x0:?}"
            );
        }
    }

    #[test]
    fn inequalities_delegate_and_verify() {
        // 0 <= x <= 3, 0 <= y, x + y <= 4
        let a = mat(&[vec![1, 0], vec![-1, 0], vec![0, 1], vec![1, 1]]);
        let b = [3, 0, 0, 4];
        match solve_integer_inequalities(&a, &b) {
            IntSolveResult::Solutions {
                particular,
                nullspace,
            } => {
                assert!(nullspace.is_empty(), "inequalities report no basis");
                assert!(
                    satisfies_inequalities(&a, &particular, &b),
                    "bad witness {particular:?}"
                );
            }
            other => panic!("expected Solutions for a feasible box, got {other:?}"),
        }
        // x >= 1 and x <= 0: refuted
        let bad = mat(&[vec![1], vec![-1]]);
        assert_eq!(
            solve_integer_inequalities(&bad, &[0, -1]),
            IntSolveResult::NoSolution
        );
    }

    #[test]
    fn invert_rational_fallback_for_non_unimodular_square() {
        // A = [[2, 0], [0, 2]] has determinant 4 (not unimodular).
        // A * x = [4, 4] has integer solution x = [2, 2].
        // HNF should fail unimodularity check, but invert_rational fallback works.
        let a = mat(&[vec![2, 0], vec![0, 2]]);
        let b = [4, 4];
        match solve_integer_system(&a, &b) {
            IntSolveResult::Solutions { particular, nullspace } => {
                assert_eq!(particular, vec![2, 2]);
                assert!(nullspace.is_empty(), "square system has no nullspace");
            }
            other => panic!("expected Solutions for non-unimodular square, got {other:?}"),
        }

        // Another case: A = [[2, 1], [1, 2]], det = 3
        // A * x = [3, 3] has solution x = [1, 1]
        let a2 = mat(&[vec![2, 1], vec![1, 2]]);
        let b2 = [3, 3];
        match solve_integer_system(&a2, &b2) {
            IntSolveResult::Solutions { particular, nullspace } => {
                assert_eq!(particular, vec![1, 1]);
                assert!(nullspace.is_empty());
            }
            other => panic!("expected Solutions, got {other:?}"),
        }

        // Case with no integer solution: A = [[2, 0], [0, 2]], b = [1, 1]
        let b3 = [1, 1];
        match solve_integer_system(&a, &b3) {
            IntSolveResult::NoSolution => {}
            other => panic!("expected NoSolution, got {other:?}"),
        }

        // Case where rational solution exists but is not integral:
        // A = [[2, 0], [0, 2]], b = [1, 2] -> x = [0.5, 1] not integral
        let b4 = [1, 2];
        match solve_integer_system(&a, &b4) {
            IntSolveResult::NoSolution => {}
            other => panic!("expected NoSolution for non-integral rational, got {other:?}"),
        }
    }

    #[test]
    fn mixed_systems_are_decided() {
        // x + y = 3 with x >= 0, y >= 0, x <= 1: has solutions (1,2), (0,3).
        let eq = mat(&[vec![1, 1]]);
        let ineq = mat(&[vec![-1, 0], vec![0, -1], vec![1, 0]]);
        match solve_integer_mixed(&eq, &[3], &ineq, &[0, 0, 1]) {
            IntSolveResult::Solutions { particular, .. } => {
                assert_eq!(particular[0] + particular[1], 3);
                assert!(particular[0] >= 0 && particular[1] >= 0 && particular[0] <= 1);
            }
            other => panic!("expected Solutions, got {other:?}"),
        }
        // x + y = 3 with x >= 5, y >= 5: equations fine, inequalities impossible
        let tight = mat(&[vec![-1, 0], vec![0, -1]]);
        assert_eq!(
            solve_integer_mixed(&eq, &[3], &tight, &[-5, -5]),
            IntSolveResult::NoSolution
        );
        // inconsistent equations short-circuit, no inequality analysis needed
        let bad_eq = mat(&[vec![1, 1], vec![2, 2]]);
        assert_eq!(
            solve_integer_mixed(&bad_eq, &[1, 3], &eq, &[3]),
            IntSolveResult::NoSolution
        );
        // a shape mismatch is Unknown, not a refutation
        assert_eq!(
            solve_integer_mixed(&eq, &[3], &eq, &[3, 9]),
            IntSolveResult::Unknown
        );
    }

    #[test]
    fn mixed_fuzz_never_lies() {
        let mut st = 0x1234abcdu64;
        let (mut sat, mut none) = (0usize, 0usize);
        for _ in 0..400 {
            let n = rand_in(&mut st, 1, 2) as usize;
            let eq = mat(&(0..rand_in(&mut st, 1, 2))
                .map(|_| {
                    (0..n)
                        .map(|_| rand_in(&mut st, -3, 3))
                        .collect::<Vec<i64>>()
                })
                .collect::<Vec<_>>());
            let b: Vec<i64> = (0..eq.rows).map(|_| rand_in(&mut st, -4, 4)).collect();
            let ineq = mat(&(0..rand_in(&mut st, 1, 2))
                .map(|_| {
                    (0..n)
                        .map(|_| rand_in(&mut st, -3, 3))
                        .collect::<Vec<i64>>()
                })
                .collect::<Vec<_>>());
            let d: Vec<i64> = (0..ineq.rows).map(|_| rand_in(&mut st, -6, 6)).collect();
            match solve_integer_mixed(&eq, &b, &ineq, &d) {
                IntSolveResult::Solutions { particular, .. } => {
                    sat += 1;
                    assert!(verifies(&eq, &particular, &b), "equations broken");
                    assert!(
                        satisfies_inequalities(&ineq, &particular, &d),
                        "inequalities broken"
                    );
                }
                IntSolveResult::NoSolution => {
                    none += 1;
                    for s in brute_solutions(&eq, &b, 4) {
                        assert!(
                            !satisfies_inequalities(&ineq, &s, &d),
                            "NoSolution but {s:?} satisfies both"
                        );
                    }
                }
                IntSolveResult::Unknown => {}
            }
        }
        assert!(sat > 20 && none > 20, "sat={sat} none={none}");
    }

    #[test]
    fn verifies_helpers_reject_bad_input() {
        let a = mat(&[vec![1, 2]]);
        assert!(!verifies(&a, &[1], &[3])); // wrong x length
        assert!(!verifies(&a, &[1, 1], &[3, 4])); // wrong b length
        assert!(verifies(&a, &[1, 1], &[3]));
        // 1*1 + 2*1 = 3 <= 2 is false, <= 3 true
        assert!(!satisfies_inequalities(&a, &[1, 1], &[2]));
        assert!(satisfies_inequalities(&a, &[1, 1], &[3]));
        assert!(!satisfies_inequalities(&a, &[1], &[9]));
    }
}
