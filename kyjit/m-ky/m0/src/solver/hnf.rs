use crate::solver::gcd::try_findgcd;
use crate::solver::inverse::{is_unimodular, Matrix};

/// Why a matrix is not (verifiably) in Hermite Normal Form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HnfError {
    /// `A·U != H` (only detectable when the product does not overflow).
    ProductMismatch,
    /// `H` is not lower triangular.
    NotTriangular,
    /// Some diagonal entry of `H` is not positive.
    NonPositivePivot,
    /// Some `|h_ij|` with `j < i` is not smaller than `h_ii`.
    ResidueOutOfRange,
    /// `U` is not square, or its determinant is not `±1`.
    NotUnimodular,
    /// An intermediate value left `i64`; the check is inconclusive.
    Overflow,
}

/// Zero out entry `(i, k)` using pivot column `c`, via the 2x2 Bezout
/// transform of columns `(c, k)`.
///
/// The transform's coefficients are derived from **`h` only** and then
/// applied identically to `h` and `u`, which is what preserves the
/// invariant `A·U = H`. Precondition: `h.get(i, c) != 0`, which the caller
/// guarantees via [`reduce_cols`]. Returns `None` on overflow.
pub fn reduce_row_pair(h: &mut Matrix, u: &mut Matrix, i: usize, c: usize, k: usize) -> Option<()> {
    let a = h.get(i, c);
    let b = h.get(i, k);
    if b == 0 {
        return Some(());
    }
    if a == 0 {
        return None;
    }
    let (g, x, y) = try_findgcd(a, b)?;
    if g == 0 {
        return None;
    }
    // `try_findgcd` already refused `i64::MIN`, so `b / g` always negates
    // safely and the transform's determinant is exactly `1`:
    // `x*q - y*p = (x*a + y*b)/g = 1`.
    let p = -(b / g);
    let q = a / g;
    h.bezout_cols_t(c, k, x, y, p, q)?;
    u.bezout_cols_t(c, k, x, y, p, q)?;
    Some(())
}

/// Reduce row `i`'s entries in columns `(c, m)` to zero, by repeatedly
/// applying [`reduce_row_pair`] against column `c`.
///
/// On success `H[i][j] == 0` for every `j` in `(c, m)` and `H[i][c] >= 0`,
/// so the pivot of row `i` is in column `c`.
pub fn reduce_cols(h: &mut Matrix, u: &mut Matrix, i: usize, c: usize) -> Option<()> {
    let m = h.cols;
    // `reduce_row_pair` needs a nonzero pivot in column `c`; if `h[i][c]`
    // is zero, pull one in from the right first.
    if h.get(i, c) == 0 {
        let k = ((c + 1)..m).find(|&k| h.get(i, k) != 0)?;
        h.swap_cols(c, k);
        u.swap_cols(c, k);
    }
    for k in (c + 1)..m {
        if h.get(i, k) != 0 {
            reduce_row_pair(h, u, i, c, k)?;
        }
    }
    // The pivot is now the only nonzero of row i at or after column c.
    if h.get(i, c) < 0 {
        h.negate_col_checked(c)?;
        u.negate_col_checked(c)?;
    }
    Some(())
}

/// Reduce `A` to column echelon form `D`, returning `(D, U)` with
/// `A·U = D`.
///
/// Algorithm `ReduceMatrix` of the reference (Wolfe §4.2.3). `D` is
/// *column* echelon: scanning columns left to right, the row of the first
/// nonzero strictly increases, and a zero column forces every later
/// column to be zero. Equivalently `D` is lower triangular up to a
/// permutation of its columns.
pub fn reduce_matrix(a: &Matrix) -> Option<(Matrix, Matrix)> {
    let n = a.rows;
    let m = a.cols;
    let mut d = a.clone();
    let mut u = Matrix::identity(m);

    // Column echelon: the pivot of column `j` sits in row `first_nz(j)`,
    // and `first_nz` strictly increases across columns. `min_row` tracks
    // one past the previous pivot row; every row below `min_row` is zero
    // in every column from `j` on, because those rows were closed when
    // their pivot column was processed and later operations only touch
    // columns at or after `j`.
    let mut min_row = 0usize;
    for j in 0..m {
        if min_row >= n {
            break;
        }
        // Among the columns still available, pick the one whose first
        // nonzero row is the *earliest* (ties broken leftmost): that row
        // becomes the next pivot row and guarantees `first_nz` increases.
        let mut best: Option<(usize, usize)> = None;
        for k in j..m {
            if let Some(i) = ((min_row)..n).find(|&i| d.get(i, k) != 0) {
                if best.map_or(true, |(_, bi)| i < bi) {
                    best = Some((k, i));
                }
            }
        }
        let (k, pr) = match best {
            // Nothing left to pivot: rows `min_row..` have no nonzero in
            // the whole tail, and the rows above it are already closed, so
            // every remaining column is entirely zero - the echelon tail.
            None => break,
            Some(v) => v,
        };
        if k != j {
            d.swap_cols(j, k);
            u.swap_cols(j, k);
        }

        // Close the pivot row: cancel `d[pr][k]` against `d[pr][j]` for
        // every later column. `d[pr][j]` stays nonzero throughout (it
        // becomes the running gcd), so each step has a valid pivot.
        for k in (j + 1)..m {
            if d.get(pr, k) != 0 {
                reduce_row_pair(&mut d, &mut u, pr, j, k)?;
            }
        }
        // Canonical sign: pivot positive.
        if d.get(pr, j) < 0 {
            d.negate_col_checked(j)?;
            u.negate_col_checked(j)?;
        }
        min_row = pr + 1;
    }

    Some((d, u))
}
/// Compute the Hermite Normal Form of `A` together with the unimodular
/// transform `U`, so that **`A·U = H`**.
///
/// Returns `None` when `A` does not have full row rank, when `U` would not
/// fit in `i64`, or when an intermediate entry overflows `i64`. A
/// rank-deficient input has a perfectly good lattice HNF, but its pivots
/// do not land on the diagonal, and pretending otherwise is how wrong
/// certificates get built; the caller should fall back to
/// [`reduce_matrix`] or to `Unknown` instead.
///
/// # Postconditions (all re-checked by [`verify_hnf`])
///
/// * `A·U == H`
/// * `H[i][j] == 0` for every `j > i` (lower triangular)
/// * `H[i][i] > 0` for every `i`
/// * `0 <= H[i][j] < H[i][i]` for every `j < i`
/// * `det(U) == ±1`
/// * columns `n..m` of `H` are zero, so the first `n` columns of `H` are
///   a basis of the lattice `L(A) = {A·x : x ∈ Z^m}`.
///
/// # Algorithm
///
/// Rows are processed top to bottom. For row `i`:
///
/// 1. every entry `H[i][k]`, `k > i`, is cancelled with the 2×2
///    unimodular Bezout transform of columns `(i, k)`, so the pivot lands
///    in column `i`;
/// 2. that pivot is made positive;
/// 3. every earlier entry `H[i][j]`, `j < i`, is reduced by
///    `col_j ← col_j - q·col_i` with `q = ⌊H[i][j] / H[i][i]⌋`.
///
/// Step 3 cannot disturb rows above `i`, because for `i' < i` the entry
/// `H[i'][i]` is already zero (it was cancelled in step 1 when row `i'`
/// was processed).
pub fn hermite(a: &Matrix) -> Option<(Matrix, Matrix)> {
    let n = a.rows;
    let m = a.cols;
    let mut h = a.clone();
    let mut u = Matrix::identity(m);

    for i in 0..n {
        // Step 1: row i gets a single nonzero at column i.
        reduce_cols(&mut h, &mut u, i, i)?;

        let pivot = h.get(i, i);
        if pivot <= 0 {
            // Rank-deficient: no pivot on the diagonal. Refuse rather
            // than emit a non-canonical H.
            return None;
        }

        // Step 3: canonical residue range for the entries below the
        // diagonal (those are the only entries step 3 can touch).
        for j in 0..i {
            let q = h.get(i, j).div_euclid(pivot);
            if q != 0 {
                h.combine_cols_checked(j, i, 1, -q)?;
                u.combine_cols_checked(j, i, 1, -q)?;
            }
        }
    }

    Some((h, u))
}

/// Verify that `(h, u)` really is the Hermite Normal Form of `a`.
///
/// Checks every postcondition of [`hermite`] and returns
/// `Err(HnfError::Overflow)` instead of a guess whenever an intermediate
/// product leaves `i64` -- so a successful return is a proof, and a
/// failure never masquerades as one.
pub fn verify_hnf(a: &Matrix, h: &Matrix, u: &Matrix) -> Result<(), HnfError> {
    let n = a.rows;
    let m = a.cols;

    if h.rows != n || h.cols != m {
        return Err(HnfError::ProductMismatch);
    }
    if u.rows != m || u.cols != m {
        return Err(HnfError::NotUnimodular);
    }

    // A·U == H, computed entrywise with i128 accumulation.
    for i in 0..n {
        for j in 0..m {
            let mut acc: i128 = 0;
            for k in 0..m {
                acc += a.get(i, k) as i128 * u.get(k, j) as i128;
            }
            if acc < i64::MIN as i128 || acc > i64::MAX as i128 {
                return Err(HnfError::Overflow);
            }
            if acc as i64 != h.get(i, j) {
                return Err(HnfError::ProductMismatch);
            }
        }
    }

    // Lower triangular, positive pivots, canonical residue range.
    for i in 0..n {
        for j in (i + 1)..m {
            if h.get(i, j) != 0 {
                return Err(HnfError::NotTriangular);
            }
        }
        let pivot = h.get(i, i);
        if pivot <= 0 {
            return Err(HnfError::NonPositivePivot);
        }
        for j in 0..i {
            let v = h.get(i, j);
            if v < 0 || v >= pivot {
                return Err(HnfError::ResidueOutOfRange);
            }
        }
    }

    if !is_unimodular(u) {
        return Err(HnfError::NotUnimodular);
    }

    Ok(())
}

/// Hermite Normal Form of a single integer vector, i.e. the generator of
/// the ideal `{Σ a_i·x_i : x ∈ Z^m}`.
///
/// Returns `Some(())`-style convenience wrapper: `Some((g, u))` where `g`
/// is the gcd and `u` is a unimodular vector with `Σ a_i·u_i == g`, or
/// `None` when every coefficient is zero.
pub fn hermite_vector(a: &[i64]) -> Option<(i64, Vec<i64>)> {
    let m = a.len();
    let mat = Matrix::from_rows(&[a.to_vec()])?;
    let (h, u) = hermite(&mat)?;
    let g = h.get(0, 0);
    let urow: Vec<i64> = (0..m).map(|j| u.get(j, 0)).collect();
    Some((g, urow))
}

/// The lattice index of a full-row-rank `A`: the absolute determinant of
/// the first `n` columns of its HNF, i.e. the number of cosets of `L(A)`
/// in `Z^n`.
///
/// Returns `None` when `A` is not full row rank or the HNF overflows.
pub fn lattice_index(a: &Matrix) -> Option<i64> {
    let n = a.rows;
    if n == 0 || a.cols < n {
        return None;
    }
    let rank_a = crate::solver::inverse::rank(a);
    if rank_a != n {
        return None;
    }
    let (h, _u) = hermite(a)?;
    let mut prod: i128 = 1;
    for i in 0..n {
        prod *= h.get(i, i) as i128;
        if prod > i64::MAX as i128 {
            return None;
        }
    }
    Some(prod as i64)
}

/// `gcd` is re-exported here because every consumer of HNF needs it, and
/// keeping it on this path makes the module self-describing.
pub use crate::solver::gcd::gcd;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::gcd::gcd_many;
    use crate::solver::inverse::{determinant, rank as matrix_rank};

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

    fn rand_matrix(state: &mut u64, rows: usize, cols: usize, mag: i64) -> Matrix {
        let mut m = Matrix::new(rows, cols);
        for i in 0..rows {
            for j in 0..cols {
                m.set(i, j, rand_in(state, -mag, mag));
            }
        }
        m
    }

    /// Random `m x m` unimodular matrix, built from elementary column
    /// operations so its determinant is exactly `+/-1`.
    fn rand_unimodular(state: &mut u64, m: usize) -> Matrix {
        let mut u = Matrix::identity(m);
        for _ in 0..(4 * m + 4) {
            let op = xs(state) % 3;
            let j = (xs(state) % m as u64) as usize;
            let l = (xs(state) % m as u64) as usize;
            match op {
                0 => {
                    if j != l {
                        u.swap_cols(j, l);
                    }
                }
                1 => u.negate_col(j),
                _ => {
                    if j != l {
                        let a = rand_in(state, -3, 3);
                        u.combine_cols_checked(j, l, 1, a).unwrap();
                    }
                }
            }
        }
        u
    }

    /// Independent membership oracle for `b ∈ L(A) = {A·x : x ∈ Z^m}`,
    /// by brute-force enumeration over a bounded box.
    fn brute_in_lattice(a: &Matrix, b: &[i64], box_side: i64) -> bool {
        let m = a.cols;
        let rng: Vec<i64> = (-box_side..=box_side).collect();
        let mut x = vec![0i64; m];
        fn go(a: &Matrix, b: &[i64], rng: &[i64], idx: usize, x: &mut Vec<i64>, found: &mut bool) {
            if *found {
                return;
            }
            if idx == a.cols {
                for i in 0..a.rows {
                    let mut s: i128 = 0;
                    for j in 0..a.cols {
                        s += a.get(i, j) as i128 * x[j] as i128;
                    }
                    if s != b[i] as i128 {
                        return;
                    }
                }
                *found = true;
                return;
            }
            for &v in rng {
                x[idx] = v;
                go(a, b, rng, idx + 1, x, found);
                if *found {
                    return;
                }
            }
        }
        let mut found = false;
        go(a, b, &rng, 0, &mut x, &mut found);
        found
    }

    /// Independent membership oracle for `b ∈ L(H)` when `H` is lower
    /// triangular HNF: forward substitution with exact divisibility.
    fn hnf_membership(h: &Matrix, b: &[i64]) -> bool {
        let n = h.rows;
        let mut t = vec![0i64; h.cols];
        for i in 0..n {
            let mut acc: i128 = b[i] as i128;
            for j in 0..i {
                acc -= h.get(i, j) as i128 * t[j] as i128;
            }
            let p = h.get(i, i) as i128;
            if acc % p != 0 {
                return false;
            }
            t[i] = (acc / p) as i64;
        }
        true
    }

    // ------------------------------------------------------------------
    // Unit tests
    // ------------------------------------------------------------------

    #[test]
    fn hermite_diagonal_input() {
        // A diagonal matrix is already in HNF (all pivots positive).
        let a = Matrix::from_slice(2, 2, &[2, 0, 0, 3]).unwrap();
        let (h, u) = hermite(&a).unwrap();
        assert_eq!(h, a);
        assert_eq!(u, Matrix::identity(2));
        assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
    }

    #[test]
    fn hermite_sign_normalisation() {
        // Negative pivots become positive, with U absorbing the sign.
        let a = Matrix::from_slice(2, 2, &[-2, 0, 0, -3]).unwrap();
        let (h, u) = hermite(&a).unwrap();
        assert_eq!(h, Matrix::from_slice(2, 2, &[2, 0, 0, 3]).unwrap());
        assert_eq!(u, Matrix::from_slice(2, 2, &[-1, 0, 0, -1]).unwrap());
        assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
    }

    #[test]
    fn hermite_classic_example() {
        // A = [[3, 5]]: the lattice is 1*Z, so H = [[1, 0]] with the
        // pivot 1 pairing with the first column and the second column
        // sent to zero. Columns are the pivot column and the null column.
        let a = Matrix::from_slice(1, 2, &[3, 5]).unwrap();
        let (h, u) = hermite(&a).unwrap();
        assert_eq!(h.get(0, 0), 1);
        assert_eq!(h.get(0, 1), 0);
        assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
        assert!(is_unimodular(&u));
    }

    #[test]
    fn hermite_residue_reduction() {
        // A = [[5, 12]] -> gcd 1, but the reference HNF additionally
        // reduces the entry *below* the diagonal into [0, h_ii).
        let a = Matrix::from_slice(2, 3, &[6, 4, 0, 0, 6, 9]).unwrap();
        let (h, u) = hermite(&a).unwrap();
        assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
        assert!(h.get(0, 0) > 0 && h.get(1, 1) > 0);
        assert!(h.get(1, 0) >= 0 && h.get(1, 0) < h.get(1, 1));
    }

    #[test]
    fn hermite_idempotent() {
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..300 {
            let m = 2 + (xs(&mut st) % 3) as usize;
            let a = rand_matrix(&mut st, 2, m, 6);
            if matrix_rank(&a) != 2 {
                continue;
            }
            let (h, u) = match hermite(&a) {
                Some(v) => v,
                None => continue,
            };
            assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
            // H is a fixed point of hermite: feeding it back must be a
            // no-op (up to the identity transform).
            let (h2, _u2) = hermite(&h).unwrap();
            assert_eq!(h2, h, "HNF not idempotent on\n{}", h);
            assert_eq!(verify_hnf(&h, &h2, &Matrix::identity(m)), Ok(()));
        }
    }

    #[test]
    fn hermite_rank_deficient_refused() {
        let a = Matrix::from_slice(2, 2, &[1, 2, 2, 4]).unwrap();
        assert!(hermite(&a).is_none());
        // But reduce_matrix still produces a column echelon form.
        let (d, u) = reduce_matrix(&a).unwrap();
        assert_eq!(d.matmul(&Matrix::identity(2)), Some(d.clone()));
        assert!(is_unimodular(&u));
    }

    #[test]
    fn reduce_matrix_identity_input() {
        let a = Matrix::identity(3);
        let (d, u) = reduce_matrix(&a).unwrap();
        assert_eq!(d, a);
        assert_eq!(u, Matrix::identity(3));
    }

    #[test]
    fn hermite_vector_matches_findgcd() {
        let (g, u) = hermite_vector(&[6, 15, 21]).unwrap();
        assert_eq!(g, 3);
        assert_eq!(6 * u[0] + 15 * u[1] + 21 * u[2], 3);
        assert!(hermite_vector(&[0, 0, 0]).is_none());
    }

    #[test]
    fn lattice_index_basics() {
        // det = 6, so Z^2 / L has 6 cosets.
        let a = Matrix::from_slice(2, 2, &[2, 0, 0, 3]).unwrap();
        assert_eq!(lattice_index(&a), Some(6));
        assert_eq!(lattice_index(&Matrix::identity(3)), Some(1));
        assert_eq!(
            lattice_index(&Matrix::from_slice(2, 2, &[1, 2, 2, 4]).unwrap()),
            None
        );
    }

    #[test]
    fn hnf_error_variants_are_reachable() {
        let a = Matrix::from_slice(1, 2, &[2, 3]).unwrap();
        // Not lower triangular: H = [[2, 3]].
        let h_bad = Matrix::from_slice(1, 2, &[2, 3]).unwrap();
        assert_eq!(
            verify_hnf(&a, &h_bad, &Matrix::identity(2)),
            Err(HnfError::NotTriangular)
        );
        // Wrong product.
        let h_ok = Matrix::from_slice(1, 2, &[1, 0]).unwrap();
        let u_bad = Matrix::from_slice(2, 2, &[1, 1, 0, 1]).unwrap();
        assert_eq!(
            verify_hnf(&a, &h_ok, &u_bad),
            Err(HnfError::ProductMismatch)
        );
        // Correct pair.
        let (h, u) = hermite(&a).unwrap();
        assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
    }

    // ------------------------------------------------------------------
    // Helpers for the fuzz tests
    // ------------------------------------------------------------------

    /// Matrix-vector product over i128, narrowed back with a bound check
    /// so a fuzz failure never turns into a silent wrap.
    fn mat_vec(a: &Matrix, x: &[i64]) -> Vec<i64> {
        (0..a.rows)
            .map(|i| {
                let mut acc: i128 = 0;
                for j in 0..a.cols {
                    acc += a.get(i, j) as i128 * x[j] as i128;
                }
                assert!(
                    acc >= i64::MIN as i128 && acc <= i64::MAX as i128,
                    "helper overflow"
                );
                acc as i64
            })
            .collect()
    }

    /// `A * U` computed independently in i128, compared against `B`.
    fn product_matches(a: &Matrix, u: &Matrix, b: &Matrix) -> bool {
        if a.cols != u.rows || u.cols != b.cols || b.rows != a.rows {
            return false;
        }
        for i in 0..a.rows {
            for j in 0..b.cols {
                let mut acc: i128 = 0;
                for k in 0..a.cols {
                    acc += a.get(i, k) as i128 * u.get(k, j) as i128;
                }
                if acc < i64::MIN as i128 || acc > i64::MAX as i128 || acc as i64 != b.get(i, j) {
                    return false;
                }
            }
        }
        true
    }

    /// The "first nonzero row strictly increases" characterisation of the
    /// column echelon form produced by [`reduce_matrix`].
    fn is_column_echelon(d: &Matrix) -> bool {
        let mut last = 0usize;
        let mut saw_pivot = false;
        for j in 0..d.cols {
            let first = (0..d.rows).find(|&i| d.get(i, j) != 0);
            match first {
                Some(i) => {
                    if saw_pivot && i <= last {
                        return false;
                    }
                    last = i;
                    saw_pivot = true;
                }
                // A zero column may only occur once the echelon part is
                // finished: everything to its right must be zero too.
                None => {
                    for k in j..d.cols {
                        if (0..d.rows).any(|i| d.get(i, k) != 0) {
                            return false;
                        }
                    }
                    return true;
                }
            }
        }
        true
    }

    // ------------------------------------------------------------------
    // Fuzz tests
    // ------------------------------------------------------------------

    /// Every HNF that comes out of [`hermite`] must pass the independent
    /// [`verify_hnf`] certificate, agree with the classical invariants
    /// (first pivot = gcd of row 0, product of pivots = |det|), and
    /// accept every lattice point built from the original matrix.
    #[test]
    fn fuzz_hnf_certificate() {
        let mut st = 0x5dee_ce66_5eef_f17au64;
        let mut certified = 0usize;
        for _ in 0..4000 {
            let n = 1 + (xs(&mut st) % 3) as usize;
            let m = n + (xs(&mut st) % 4) as usize;
            let mag = 1 + (xs(&mut st) % 30) as i64;
            let a = rand_matrix(&mut st, n, m, mag);
            if matrix_rank(&a) != n {
                continue;
            }
            let (h, u) = match hermite(&a) {
                Some(v) => v,
                // Overflow on purpose: the reference gcd refuses i64::MIN,
                // so a pivot that lands there must be reported, not wrapped.
                None => continue,
            };
            certified += 1;
            let ctx = format!("A(n={} m={} mag={}):\n{}", n, m, mag, a);

            // 1. The certificate is the primary oracle.
            assert_eq!(verify_hnf(&a, &h, &u), Ok(()), "bad certificate\n{}", ctx);

            // 2. Independent invariants.
            let row0 = a.row(0);
            assert_eq!(
                h.get(0, 0),
                gcd_many(&row0),
                "first pivot is not gcd(row 0)\n{}",
                ctx
            );
            if n == m {
                let det = determinant(&a).expect("square full rank has a determinant");
                let mut prod: i128 = 1;
                for i in 0..n {
                    prod *= h.get(i, i) as i128;
                }
                assert_eq!(prod, (det as i128).abs(), "|det| mismatch\n{}", ctx);
                assert_eq!(
                    lattice_index(&a),
                    Some(det.abs()),
                    "index mismatch\n{}",
                    ctx
                );
            }

            // 3. Every point of L(A), built by hand, lies in L(H) - which is
            //    checked with forward substitution, an algorithm that shares
            //    nothing with the HNF computation itself.
            for _ in 0..6 {
                let x: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -4, 4)).collect();
                let b = mat_vec(&a, &x);
                assert!(
                    hnf_membership(&h, &b),
                    "L(A) not inside L(H) for b={:?}\n{}",
                    b,
                    ctx
                );
            }
        }
        assert!(certified > 500, "too few full-rank samples: {}", certified);
    }

    /// HNF is a *canonical* form: two matrices generating the same lattice
    /// must produce the same `H`. Right-multiplying by any unimodular `U0`
    /// preserves the lattice, so `H(A*U0) == H(A)` - a much stronger check
    /// than idempotence, because `U0` scrambles the whole input.
    #[test]
    fn fuzz_hnf_is_unique_per_lattice() {
        let mut st = 0x1b87_3593_17dd_4e79u64;
        let mut compared = 0usize;
        for _ in 0..1500 {
            let n = 1 + (xs(&mut st) % 3) as usize;
            let m = n + (xs(&mut st) % 3) as usize;
            let a = rand_matrix(&mut st, n, m, 12);
            if matrix_rank(&a) != n {
                continue;
            }
            let u0 = rand_unimodular(&mut st, m);
            let a2 = match a.matmul(&u0) {
                Some(v) => v,
                None => continue,
            };
            let (h1, _) = match hermite(&a) {
                Some(v) => v,
                None => continue,
            };
            let (h2, _) = match hermite(&a2) {
                Some(v) => v,
                None => continue,
            };
            compared += 1;
            assert_eq!(
                h1, h2,
                "same lattice, different HNF\nA:\n{}\nA*U0:\n{}\nU0:\n{}",
                a, a2, u0
            );
        }
        assert!(compared > 200, "too few comparisons: {}", compared);
    }

    /// The other inclusion: points of `L(H)` must be reachable from `A`.
    ///
    /// The oracle is a brute-force search over a bounded box, so a sample is
    /// only asserted when the witness `x = U*t` provably lands inside that
    /// box (otherwise the search would report a false negative).
    #[test]
    fn fuzz_hnf_membership_matches_brute_force() {
        let mut st = 0x243f_6a88_85a3_08d3u64;
        let mut asserted = 0usize;
        let side = 10i64;
        for _ in 0..1200 {
            let m = 2 + (xs(&mut st) % 2) as usize;
            let a = rand_matrix(&mut st, m, m, 9);
            if matrix_rank(&a) != m {
                continue;
            }
            let (h, u) = match hermite(&a) {
                Some(v) => v,
                None => continue,
            };
            assert_eq!(verify_hnf(&a, &h, &u), Ok(()), "A:\n{}", a);
            for _ in 0..8 {
                // t only spans the m pivot columns, which are a basis.
                let t: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
                let b = mat_vec(&h, &t);
                assert!(hnf_membership(&h, &b), "H rejects its own point {:?}", b);
                let x = mat_vec(&u, &t);
                if x.iter().all(|&v| v.abs() <= side) {
                    asserted += 1;
                    assert!(
                        brute_in_lattice(&a, &b, side),
                        "L(H) not inside L(A): b={:?} needs x={:?}",
                        b,
                        x
                    );
                }
            }
        }
        assert!(
            asserted > 200,
            "box too tight, only {} assertions",
            asserted
        );
    }

    /// [`reduce_matrix`] is the fallback used on rank-deficient input, so it
    /// needs a certificate of its own: `A*U = D` exactly, `U` unimodular,
    /// `D` in column echelon form with positive pivots.
    #[test]
    fn fuzz_reduce_matrix_echelon_certificate() {
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        let mut checked = 0usize;
        for _ in 0..2000 {
            let n = 1 + (xs(&mut st) % 3) as usize;
            let m = n + (xs(&mut st) % 4) as usize;
            // Deliberately include rank-deficient inputs (duplicated rows and
            // zero columns) - reduce_matrix must either refuse or be exact.
            let mut a = rand_matrix(&mut st, n, m, 10);
            if xs(&mut st) % 3 == 0 && n > 1 {
                let r = (xs(&mut st) % n as u64) as usize;
                let row = a.row(r);
                a.set_row((r + 1) % n, &row);
            }
            if xs(&mut st) % 4 == 0 {
                let c = (xs(&mut st) % m as u64) as usize;
                for i in 0..n {
                    a.set(i, c, 0);
                }
            }
            let (d, u) = match reduce_matrix(&a) {
                Some(v) => v,
                None => continue,
            };
            checked += 1;
            let ctx = format!("A:\n{}\nD:\n{}\nU:\n{}", a, d, u);
            assert!(product_matches(&a, &u, &d), "A*U != D\n{}", ctx);
            assert!(is_unimodular(&u), "U not unimodular\n{}", ctx);
            assert!(is_column_echelon(&d), "D not column echelon\n{}", ctx);
            assert_eq!(d.rows, n);
            assert_eq!(d.cols, m);
            for j in 0..m {
                if let Some(i) = (0..n).find(|&i| d.get(i, j) != 0) {
                    assert!(d.get(i, j) > 0, "negative pivot\n{}", ctx);
                }
            }
        }
        assert!(
            checked > 400,
            "too few reduce_matrix successes: {}",
            checked
        );
    }

    /// `hermite_vector` is the 1*m special case used by the Diophantine
    /// solver: the pivot must be the gcd and the returned coefficients must
    /// actually realise it.
    #[test]
    fn fuzz_hermite_vector_is_bezout_identity() {
        let mut st = 0xcafe_babe_0123_4567u64;
        for _ in 0..5000 {
            let m = 1 + (xs(&mut st) % 6) as usize;
            let a: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -60, 60)).collect();
            if a.iter().all(|&v| v == 0) {
                assert!(hermite_vector(&a).is_none());
                continue;
            }
            let (g, u) = hermite_vector(&a).unwrap();
            assert_eq!(g, gcd_many(&a), "not the gcd for {:?}", a);
            assert!(g > 0, "non-positive gcd for {:?}", a);
            let mut acc: i128 = 0;
            for i in 0..m {
                acc += a[i] as i128 * u[i] as i128;
            }
            assert_eq!(acc, g as i128, "Bezout identity fails for {:?}", a);
            assert_eq!(u.len(), m);
        }
    }

    /// Nasty inputs: `i64::MIN` must never be silently wrapped, and a matrix
    /// that is already canonical must come out untouched.
    #[test]
    fn fuzz_hnf_edge_cases_never_wrap() {
        let mut st = 0x0123_4567_89ab_cdefu64;
        // i64::MIN in a pivot position: the gcd of MIN and MIN is |MIN|,
        // which i64 cannot represent - so the transform must fail or be
        // provably right, never wrap.
        for vals in [
            vec![i64::MIN, i64::MIN],
            vec![i64::MIN, 2],
            vec![i64::MIN, i64::MAX],
            vec![1, i64::MIN, 0],
        ] {
            let a = Matrix::from_rows(&[vals.clone()]).unwrap();
            if let Some((h, u)) = hermite(&a) {
                assert_eq!(
                    verify_hnf(&a, &h, &u),
                    Ok(()),
                    "unverifiable result for {:?}\nA:\n{}",
                    vals,
                    a
                );
                assert_eq!(h.get(0, 0), gcd_many(&vals), "wrong gcd for {:?}", vals);
            }
        }

        // A canonical H is a fixed point of hermite, with U = I.
        for _ in 0..800 {
            let n = 1 + (xs(&mut st) % 3) as usize;
            let mut h = Matrix::new(n, n);
            for i in 0..n {
                let pivot = 1 + (xs(&mut st) % 9) as i64;
                h.set(i, i, pivot);
                for j in 0..i {
                    h.set(i, j, rand_in(&mut st, 0, pivot - 1));
                }
            }
            let (h2, u2) = hermite(&h).unwrap();
            assert_eq!(h2, h, "canonical H not a fixed point:\n{}", h);
            assert_eq!(u2, Matrix::identity(n), "canonical H needed a transform");
            assert_eq!(verify_hnf(&h, &h2, &u2), Ok(()));
        }

        // Very wide systems (the shape UDDP actually produces: a handful of
        // dependence equations over many loop variables).
        for _ in 0..600 {
            let n = 1 + (xs(&mut st) % 4) as usize;
            let m = n + (xs(&mut st) % 20) as usize;
            let a = rand_matrix(&mut st, n, m, 3);
            if let Some((h, u)) = hermite(&a) {
                assert_eq!(verify_hnf(&a, &h, &u), Ok(()), "A:\n{}", a);
                // Columns past the last pivot vanish, so the first n columns
                // are a basis of the lattice.
                for j in n..m {
                    for i in 0..n {
                        assert_eq!(h.get(i, j), 0, "nonzero beyond the last pivot");
                    }
                }
                assert!(is_unimodular(&u));
            }
        }
    }
}

/// The transform accumulator of a `3 x 6` reduction reaches `1e17`-sized
/// entries, so its determinant is far outside `i128`: this case pins the
/// overflow-immune [`is_unimodular`] path and the `A·U = H` bookkeeping
/// that originally broke (the Bezout coefficients were being derived from
/// `U` instead of the working matrix).
/// regression tests
#[cfg(test)]
mod regressions {
    use super::*;

    #[test]
    fn hnf_registers_u_beyond_i128_determinant() {
        let a = Matrix::from_slice(
            3,
            6,
            &[
                -16, 27, 10, -26, -14, -13, -20, 24, -6, 10, 7, -29, -12, -18, 29, 10, 29, 9,
            ],
        )
        .unwrap();
        let (h, u) = hermite(&a).unwrap();
        // The accumulator genuinely overflows the exact determinant path.
        assert!(crate::solver::inverse::determinant(&u).is_none());
        assert!(is_unimodular(&u), "modular proof must certify U");
        assert_eq!(verify_hnf(&a, &h, &u), Ok(()));
        // rank 3 over 6 columns, so H is [I | 0].
        assert_eq!(
            h,
            Matrix::from_slice(
                3,
                6,
                &[1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,]
            )
            .unwrap()
        );
    }
}
