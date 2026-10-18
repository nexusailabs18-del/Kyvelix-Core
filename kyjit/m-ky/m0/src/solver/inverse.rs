//! Integer matrices, exact determinant, rank and inverse (Wolfe §4.2.2).
//!
//! ## Why the spec's `invert` is not the whole story
//!
//! Algorithm Invert in the reference (Wolfe §4.2.2) divides rows by the
//! pivot. That is only exact when the pivot divides every entry of its
//! row, i.e. for **unimodular** matrices. The spec flags this and
//! decides: "keep `invert` for integer-valued inverses; add
//! `invert_rational` returning `(numerator, denominator)` for the general
//! case". This module implements exactly that decision:
//!
//! - [`invert`] returns `Some` only when `A⁻¹` is **integral**;
//! - [`invert_rational`] returns the general rational inverse as two
//!   integer matrices `(num, den)` with `A⁻¹ = num/den` entrywise and
//!   `den[i][j] > 0`, `gcd(|num|, den) == 1`;
//! - [`adjugate`] returns the classical adjugate together with `det(A)`,
//!   so that `adj(A) = det(A)·A⁻¹`.
//!
//! All arithmetic is exact over the rationals, on `i128`
//! numerator/denominator pairs reduced by their gcd at every step. Every
//! operation is checked; on overflow the `Option` API returns `None` and
//! [`rank`] returns a conservative value (it never over-reports rank).
//!
//! ## Determinant convention
//!
//! The spec sketch says `determinant` "returns None if singular". That is
//! lossy: `det` of a singular matrix is `0`, a perfectly good integer,
//! and `det(A·B) == det(A)·det(B)` is cleanest with `Some(0)` rather than
//! `None`. This module therefore returns `Some(0)` for singular square
//! matrices and reserves `None` for "not square" or "overflow".
//! [`is_unimodular`] only ever accepts `±1`, so the acceptance behaviour
//! is identical to the spec's.

// ============================================================================
// Matrix
// ============================================================================

/// A dense row-major integer matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    /// Row-major, `rows * cols` entries.
    pub data: Vec<i64>,
}

impl Matrix {
    /// A `rows × cols` zero matrix.
    pub fn new(rows: usize, cols: usize) -> Self {
        Matrix {
            rows,
            cols,
            data: vec![0i64; rows * cols],
        }
    }

    /// Build from a row-major slice. `None` if the shape does not match.
    pub fn from_slice(rows: usize, cols: usize, data: &[i64]) -> Option<Self> {
        if data.len() != rows * cols {
            return None;
        }
        Some(Matrix {
            rows,
            cols,
            data: data.to_vec(),
        })
    }

    /// Build from rows of equal length. `None` if ragged.
    pub fn from_rows(rows: &[Vec<i64>]) -> Option<Self> {
        let r = rows.len();
        let c = rows.first().map(|v| v.len()).unwrap_or(0);
        if rows.iter().any(|v| v.len() != c) {
            return None;
        }
        let mut data = Vec::with_capacity(r * c);
        for row in rows {
            data.extend_from_slice(row);
        }
        Some(Matrix {
            rows: r,
            cols: c,
            data,
        })
    }

    #[inline]
    pub fn idx(&self, i: usize, j: usize) -> usize {
        debug_assert!(i < self.rows && j < self.cols);
        i * self.cols + j
    }

    #[inline]
    pub fn get(&self, i: usize, j: usize) -> i64 {
        self.data[self.idx(i, j)]
    }

    #[inline]
    pub fn set(&mut self, i: usize, j: usize, v: i64) {
        let k = self.idx(i, j);
        self.data[k] = v;
    }

    /// The `n × n` identity.
    pub fn identity(n: usize) -> Self {
        let mut m = Matrix::new(n, n);
        for i in 0..n {
            m.set(i, i, 1);
        }
        m
    }

    pub fn transpose(&self) -> Self {
        let mut out = Matrix::new(self.cols, self.rows);
        for i in 0..self.rows {
            for j in 0..self.cols {
                out.set(j, i, self.get(i, j));
            }
        }
        out
    }

    /// Exact matrix product. `None` if the shapes do not multiply or an
    /// entry overflows `i64`.
    pub fn matmul(&self, other: &Matrix) -> Option<Matrix> {
        if self.cols != other.rows {
            return None;
        }
        let mut out = Matrix::new(self.rows, other.cols);
        for i in 0..self.rows {
            for j in 0..other.cols {
                let mut acc: i128 = 0;
                for k in 0..self.cols {
                    acc += self.get(i, k) as i128 * other.get(k, j) as i128;
                }
                if acc < i64::MIN as i128 || acc > i64::MAX as i128 {
                    return None;
                }
                out.set(i, j, acc as i64);
            }
        }
        Some(out)
    }

    pub fn row(&self, i: usize) -> Vec<i64> {
        (0..self.cols).map(|j| self.get(i, j)).collect()
    }

    pub fn col(&self, j: usize) -> Vec<i64> {
        (0..self.rows).map(|i| self.get(i, j)).collect()
    }

    pub fn set_row(&mut self, i: usize, row: &[i64]) {
        debug_assert_eq!(row.len(), self.cols);
        for j in 0..self.cols {
            self.set(i, j, row[j]);
        }
    }

    pub fn is_square(&self) -> bool {
        self.rows == self.cols
    }

    pub fn is_zero(&self) -> bool {
        self.data.iter().all(|&v| v == 0)
    }

    /// Swap rows `i` and `k`.
    pub fn swap_rows(&mut self, i: usize, k: usize) {
        if i == k {
            return;
        }
        for j in 0..self.cols {
            let (a, b) = (self.get(i, j), self.get(k, j));
            self.set(i, j, b);
            self.set(k, j, a);
        }
    }

    /// Negate row `i`.
    pub fn negate_row(&mut self, i: usize) {
        for j in 0..self.cols {
            let v = self.get(i, j);
            self.set(i, j, -v);
        }
    }

    /// `row i <- row i - alpha * row k`.
    pub fn sub_row(&mut self, i: usize, k: usize, alpha: i64) {
        let row = self.combine_rows(i, k, 1, -alpha);
        self.set_row(i, &row);
    }

    /// `row i <- alpha * row i + beta * row k` (the `combine_rows` of the
    /// spec). Both rows are read before either is written, so the result
    /// is returned as a fresh row rather than mutated in place.
    pub fn combine_rows(&self, i: usize, k: usize, alpha: i64, beta: i64) -> Vec<i64> {
        (0..self.cols)
            .map(|j| {
                let v =
                    alpha as i128 * self.get(i, j) as i128 + beta as i128 * self.get(k, j) as i128;
                debug_assert!(v >= i64::MIN as i128 && v <= i64::MAX as i128);
                v as i64
            })
            .collect()
    }

    /// Divide row `i` by `c`. Returns `false`, leaving the row untouched,
    /// when `c == 0` or the division is not exact -- the caller must not
    /// use this on a non-unimodular matrix.
    pub fn divide_row_exact(&mut self, i: usize, c: i64) -> bool {
        if c == 0 {
            return false;
        }
        let mut out = vec![0i64; self.cols];
        for j in 0..self.cols {
            let v = self.get(i, j);
            if v % c != 0 {
                return false;
            }
            out[j] = v / c;
        }
        self.set_row(i, &out);
        true
    }

    /// [`Matrix::combine_rows`] with overflow checks; `None` if any entry
    /// would leave `i64`.
    pub fn combine_rows_checked(
        &self,
        i: usize,
        k: usize,
        alpha: i64,
        beta: i64,
    ) -> Option<Vec<i64>> {
        let mut out = Vec::with_capacity(self.cols);
        for j in 0..self.cols {
            let v = alpha as i128 * self.get(i, j) as i128 + beta as i128 * self.get(k, j) as i128;
            if v < i64::MIN as i128 || v > i64::MAX as i128 {
                return None;
            }
            out.push(v as i64);
        }
        Some(out)
    }

    /// `row i <- row i - alpha * row k` with overflow checks.
    pub fn sub_row_checked(&mut self, i: usize, k: usize, alpha: i64) -> Option<()> {
        let row = self.combine_rows_checked(i, k, 1, -alpha)?;
        self.set_row(i, &row);
        Some(())
    }

    /// Swap columns `j` and `l`.
    pub fn swap_cols(&mut self, j: usize, l: usize) {
        if j == l {
            return;
        }
        for i in 0..self.rows {
            let (a, b) = (self.get(i, j), self.get(i, l));
            self.set(i, j, b);
            self.set(i, l, a);
        }
    }

    /// Negate column `j`.
    pub fn negate_col(&mut self, j: usize) {
        for i in 0..self.rows {
            let v = self.get(i, j);
            self.set(i, j, -v);
        }
    }

    /// [`Matrix::negate_col`] with an overflow check: `None` (leaving the
    /// matrix untouched) if the column contains `i64::MIN`, whose negation
    /// is not representable.
    pub fn negate_col_checked(&mut self, j: usize) -> Option<()> {
        let cj = self.col(j);
        let mut out = Vec::with_capacity(self.rows);
        for &v in &cj {
            out.push(v.checked_neg()?);
        }
        for i in 0..self.rows {
            self.set(i, j, out[i]);
        }
        Some(())
    }

    /// `col j <- alpha * col j + beta * col l`, overflow-checked.
    ///
    /// Used by the Hermite Normal Form, where the transform must be a
    /// unimodular **column** operation so that `A·U = H` is preserved.
    pub fn combine_cols_checked(
        &mut self,
        j: usize,
        l: usize,
        alpha: i64,
        beta: i64,
    ) -> Option<()> {
        let mut out = vec![0i64; self.rows];
        for i in 0..self.rows {
            let v = alpha as i128 * self.get(i, j) as i128 + beta as i128 * self.get(i, l) as i128;
            if v < i64::MIN as i128 || v > i64::MAX as i128 {
                return None;
            }
            out[i] = v as i64;
        }
        for i in 0..self.rows {
            self.set(i, j, out[i]);
        }
        Some(())
    }

    /// Apply the 2x2 column transform
    /// `cols j, l <- x*col_j + y*col_l ,  p*col_j + q*col_l`
    /// simultaneously to both columns, reading the old values first.
    ///
    /// The caller supplies the whole transform, which is what makes this
    /// usable from the Hermite reduction: the coefficients `x, y, p, q` are
    /// derived from the **source** matrix `H` (where `a = H[r][j]`,
    /// `b = H[r][l]`, `g = gcd(a, b)`, `x*a + y*b = g`, `p = -(b/g)`,
    /// `q = a/g`), but must be applied to `H` *and* to the accumulator `U`
    /// identically. Deriving them from `U` instead would be a different,
    /// non-unimodular map.
    ///
    /// `debug_assert`s that the transform is unimodular (determinant `1`),
    /// and returns `None` on overflow, leaving both columns untouched.
    pub fn bezout_cols_t(
        &mut self,
        j: usize,
        l: usize,
        x: i64,
        y: i64,
        p: i64,
        q: i64,
    ) -> Option<()> {
        debug_assert_eq!(
            x as i128 * q as i128 - y as i128 * p as i128,
            1,
            "bezout_cols_t called with a non-unimodular transform"
        );
        // Snapshot both columns first: the updates are simultaneous.
        let cj: Vec<i64> = self.col(j);
        let cl: Vec<i64> = self.col(l);
        let mut nj = vec![0i64; self.rows];
        let mut nl = vec![0i64; self.rows];
        for i in 0..self.rows {
            let v1 = x as i128 * cj[i] as i128 + y as i128 * cl[i] as i128;
            let v2 = p as i128 * cj[i] as i128 + q as i128 * cl[i] as i128;
            if v1 < i64::MIN as i128
                || v1 > i64::MAX as i128
                || v2 < i64::MIN as i128
                || v2 > i64::MAX as i128
            {
                return None;
            }
            nj[i] = v1 as i64;
            nl[i] = v2 as i64;
        }
        for i in 0..self.rows {
            self.set(i, j, nj[i]);
        }
        for i in 0..self.rows {
            self.set(i, l, nl[i]);
        }
        Some(())
    }

    /// `cols j, l <- x·col_j + y·col_l , -(b/g)·col_j + (a/g)·col_l`,
    /// where `a = self[r][j]`, `b = self[r][l]`, `(g, x, y) = findgcd(a, b)`.
    ///
    /// This is the 2×2 unimodular transform at the heart of Hermite
    /// reduction: it sends entry `(r, j)` to `g > 0` and entry `(r, l)` to
    /// `0`, and its determinant is `(x·a + y·b)/g = 1`.
    ///
    /// Correct **only** when `self` is the matrix the coefficients were
    /// derived from; use [`Matrix::bezout_cols_t`] to mirror a transform
    /// onto the accumulator. Returns `None` on overflow, leaving both
    /// columns untouched.
    pub fn bezout_cols(
        &mut self,
        r: usize,
        j: usize,
        l: usize,
        g: i64,
        x: i64,
        y: i64,
    ) -> Option<()> {
        if g == 0 {
            return None;
        }
        let a = self.get(r, j);
        let b = self.get(r, l);
        let (p, q) = (-(b / g), a / g);
        self.bezout_cols_t(j, l, x, y, p, q)
    }
}

impl std::fmt::Display for Matrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for i in 0..self.rows {
            write!(f, "[")?;
            for j in 0..self.cols {
                if j > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", self.get(i, j))?;
            }
            writeln!(f, "]")?;
        }
        Ok(())
    }
}
// ============================================================================
// Exact rationals on i128 (private)
// ============================================================================

/// A reduced rational: `num / den` with `den > 0` and `gcd(|num|, den) == 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rat {
    num: i128,
    den: i128,
}

impl Rat {
    fn new(num: i128, den: i128) -> Option<Rat> {
        if den == 0 {
            return None;
        }
        let (mut n, mut d) = (num, den);
        if d < 0 {
            n = n.checked_neg()?;
            d = d.checked_neg()?;
        }
        let g = gcd_i128(n, d);
        if g > 1 {
            n /= g;
            d /= g;
        }
        Some(Rat { num: n, den: d })
    }

    fn zero() -> Rat {
        Rat { num: 0, den: 1 }
    }

    fn one() -> Rat {
        Rat { num: 1, den: 1 }
    }

    fn is_zero(self) -> bool {
        self.num == 0
    }

    fn neg(self) -> Option<Rat> {
        Rat::new(self.num.checked_neg()?, self.den)
    }

    fn add(self, other: Rat) -> Option<Rat> {
        let n = self
            .num
            .checked_mul(other.den)?
            .checked_add(other.num.checked_mul(self.den)?)?;
        let d = self.den.checked_mul(other.den)?;
        Rat::new(n, d)
    }

    fn sub(self, other: Rat) -> Option<Rat> {
        self.add(other.neg()?)
    }

    fn mul(self, other: Rat) -> Option<Rat> {
        Rat::new(
            self.num.checked_mul(other.num)?,
            self.den.checked_mul(other.den)?,
        )
    }

    fn div(self, other: Rat) -> Option<Rat> {
        if other.num == 0 {
            return None;
        }
        Rat::new(
            self.num.checked_mul(other.den)?,
            self.den.checked_mul(other.num)?,
        )
    }

    /// `Some(v)` iff this rational is an integer representable in `i64`.
    fn to_i64_exact(self) -> Option<i64> {
        if self.den != 1 {
            return None;
        }
        if self.num < i64::MIN as i128 || self.num > i64::MAX as i128 {
            return None;
        }
        Some(self.num as i64)
    }

    /// Narrow to `(i64, i64)`. `None` on overflow; the rational itself
    /// need not be integral.
    fn to_pair(self) -> Option<(i64, i64)> {
        if self.num < i64::MIN as i128
            || self.num > i64::MAX as i128
            || self.den < i64::MIN as i128
            || self.den > i64::MAX as i128
        {
            return None;
        }
        Some((self.num as i64, self.den as i64))
    }
}

/// gcd on non-negative `i128`, tolerating `i128::MIN` by clamping.
fn gcd_i128(mut a: i128, mut b: i128) -> i128 {
    if a < 0 {
        a = a.checked_neg().unwrap_or(i128::MAX);
    }
    if b < 0 {
        b = b.checked_neg().unwrap_or(i128::MAX);
    }
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

// ============================================================================
// Row reduction over Q (private)
// ============================================================================

/// Result of exact Gaussian elimination on a copy of `A`.
struct Reduction {
    /// Column indices of the pivots, in row order.
    pivot_cols: Vec<usize>,
    /// Sign `(-1)^(number of row swaps)`, so that
    /// `det(A) = sign * product(pivots)` for square `A`.
    sign: i128,
    /// Pivot values themselves (already reduced).
    pivots: Vec<Rat>,
    /// True if an intermediate value left `i128` (result unusable).
    overflowed: bool,
}

/// Exact Gaussian elimination with partial pivoting.
///
/// Partial pivoting (pick the largest `|entry|` in the column) is used
/// purely for numerical hygiene; it does not affect exactness, and the
/// swap sign is tracked so `det` stays identical to the Leibniz
/// expansion. Returns `None` only for the empty matrix.
fn reduce_rows(a: &Matrix) -> Reduction {
    let mut m: Vec<Vec<Rat>> = (0..a.rows)
        .map(|i| {
            (0..a.cols)
                .map(|j| Rat::new(a.get(i, j) as i128, 1).unwrap())
                .collect()
        })
        .collect();

    let mut pivot_cols = Vec::new();
    let mut pivots = Vec::new();
    let mut sign: i128 = 1;
    let mut overflowed = false;
    let mut r = 0usize;

    for j in 0..a.cols {
        if r >= a.rows {
            break;
        }
        // Partial-pivot: largest magnitude nonzero in column j at or below r.
        let mut best: Option<(usize, u128)> = None;
        for k in r..a.rows {
            if m[k][j].is_zero() {
                continue;
            }
            let mag = m[k][j].num.unsigned_abs();
            if best.map(|(_, bm)| mag > bm).unwrap_or(true) {
                best = Some((k, mag));
            }
        }
        let k = match best {
            Some((k, _)) => k,
            None => continue,
        };
        if k != r {
            m.swap(r, k);
            sign = -sign;
        }
        let pv = m[r][j];
        for i in (r + 1)..a.rows {
            if m[i][j].is_zero() {
                continue;
            }
            let f = match m[i][j].div(pv) {
                Some(f) => f,
                None => {
                    overflowed = true;
                    break;
                }
            };
            for c in j..a.cols {
                let sub = match f.mul(m[r][c]) {
                    Some(v) => v,
                    None => {
                        overflowed = true;
                        break;
                    }
                };
                m[i][c] = match m[i][c].sub(sub) {
                    Some(v) => v,
                    None => {
                        overflowed = true;
                        break;
                    }
                };
            }
            if overflowed {
                break;
            }
        }
        if overflowed {
            break;
        }
        pivot_cols.push(j);
        pivots.push(pv);
        r += 1;
    }

    Reduction {
        pivot_cols,
        sign,
        pivots,
        overflowed,
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Determinant of a square integer matrix, computed by exact
/// fraction-free elimination over Q.
///
/// Returns `Some(0)` for a singular square matrix, `None` for a
/// non-square matrix or `i64` overflow. The Leibniz identity
/// `det(A·B) == det(A)·det(B)` therefore always holds when both sides
/// are representable.
///
/// `det([]) == 1` (the empty product).
pub fn determinant(a: &Matrix) -> Option<i64> {
    if !a.is_square() {
        return None;
    }
    if a.rows == 0 {
        return Some(1);
    }
    let red = reduce_rows(a);
    if red.overflowed {
        return None;
    }
    // A square matrix has a pivot in every column iff it is nonsingular.
    if red.pivot_cols.len() < a.rows {
        return Some(0);
    }
    let mut det = Rat::new(red.sign, 1)?;
    for p in &red.pivots {
        det = det.mul(*p)?;
    }
    det.to_i64_exact()
}

/// Rank of `A` over the rationals (any shape).
///
/// On `i128` overflow this returns a **lower** bound -- it never
/// over-reports rank, so callers that use rank to conclude
/// nonsingularity stay sound.
pub fn rank(a: &Matrix) -> usize {
    reduce_rows(a).pivot_cols.len()
}

/// Rank computed over the rationals, `None` on `i128` overflow.
pub fn rank_checked(a: &Matrix) -> Option<usize> {
    let red = reduce_rows(a);
    if red.overflowed {
        None
    } else {
        Some(red.pivot_cols.len())
    }
}

/// Rank - nullity theorem helper: `nullity(A) = cols - rank(A)`.
pub fn nullity(a: &Matrix) -> usize {
    a.cols.saturating_sub(rank(a))
}

/// Classical adjugate: `adj(A) = det(A) * A^-1`, returned with
/// `det(A)`. Both are exact integers.
///
/// `adj(A)[i][j]` is the cofactor of `A[j][i]`, computed by cofactor
/// expansion (so this is `O(n!)` -- intended for the small matrices UDDP
/// actually uses, `n <= 8`). Returns `(adj, det)`, or `None` if `A` is
/// not square or the expansion overflows `i64`.
///
/// For singular `A` this still returns `(adj, 0)`, and `adj` is the zero
/// matrix whenever `rank(A) <= n - 2`.
pub fn adjugate(a: &Matrix) -> Option<(Matrix, i64)> {
    if !a.is_square() {
        return None;
    }
    let n = a.rows;
    if n == 0 {
        return Some((Matrix::new(0, 0), 1));
    }
    if n > 8 {
        return None; // cofactor expansion is factorial; refuse politely
    }
    let mut adj = Matrix::new(n, n);
    for i in 0..n {
        for j in 0..n {
            let minor = minor_det(a, j, i)?;
            // Cofactor sign (-1)^(j+i), cofactor of A[j][i].
            let signed = if (i + j) % 2 == 0 {
                minor as i128
            } else {
                -(minor as i128)
            };
            if signed < i64::MIN as i128 || signed > i64::MAX as i128 {
                return None;
            }
            adj.set(i, j, signed as i64);
        }
    }
    let det = minor_det_plain(a)?;
    Some((adj, det))
}

/// Cofactor expansion determinant (private; `n <= 8` enforced by caller
/// through [`adjugate`]).
fn minor_det_plain(a: &Matrix) -> Option<i64> {
    let n = a.rows;
    if n == 0 {
        return Some(1);
    }
    if n == 1 {
        return Some(a.get(0, 0));
    }
    if n == 2 {
        let v =
            a.get(0, 0) as i128 * a.get(1, 1) as i128 - a.get(0, 1) as i128 * a.get(1, 0) as i128;
        if v < i64::MIN as i128 || v > i64::MAX as i128 {
            return None;
        }
        return Some(v as i64);
    }
    let mut total: i128 = 0;
    for j in 0..n {
        let m = minor_det(a, 0, j)?;
        let term = a.get(0, j) as i128 * m as i128;
        if j % 2 == 0 {
            total = total.checked_add(term)?;
        } else {
            total = total.checked_sub(term)?;
        }
    }
    if total < i64::MIN as i128 || total > i64::MAX as i128 {
        None
    } else {
        Some(total as i64)
    }
}

/// Determinant of `A` with row `ri` and column `rj` deleted.
fn minor_det(a: &Matrix, ri: usize, rj: usize) -> Option<i64> {
    let n = a.rows;
    if n <= 1 {
        return Some(1);
    }
    let mut sub: Vec<Vec<i64>> = Vec::with_capacity(n - 1);
    for i in 0..n {
        if i == ri {
            continue;
        }
        let mut row = Vec::with_capacity(n - 1);
        for j in 0..n {
            if j == rj {
                continue;
            }
            row.push(a.get(i, j));
        }
        sub.push(row);
    }
    let m = Matrix::from_rows(&sub)?;
    minor_det_plain(&m)
}

/// Solve `A * X = I` exactly over Q. Returns the inverse as a rational
/// matrix in reduced `(num, den)` pairs with `den > 0`, or `None` when
/// `A` is not square, is singular, or the arithmetic overflows `i128`.
///
/// This is the general-case companion to [`invert`], implementing the
/// spec's `invert_rational` decision. The pair is **entrywise**:
/// `A^-1[i][j] == num[i][j] / den[i][j]`, each fraction in lowest terms
/// with a positive denominator.
///
/// Note that this is deliberately *not* a matrix product: `num` alone is
/// not a factor of `A^-1`, because the division is per entry. A singular
/// matrix has no inverse at all, hence `None`; use [`invert`] to learn
/// the rank of a rank-deficient matrix.
pub fn invert_rational(a: &Matrix) -> Option<(Matrix, Matrix)> {
    if !a.is_square() {
        return None;
    }
    // Gauss-Jordan on [A | I] over Rat, one reduced row at a time.
    let n = a.rows;
    let mut m: Vec<Vec<Rat>> = (0..n)
        .map(|i| {
            (0..2 * n)
                .map(|j| {
                    if j < n {
                        Rat::new(a.get(i, j) as i128, 1).unwrap()
                    } else if j - n == i {
                        Rat::one()
                    } else {
                        Rat::zero()
                    }
                })
                .collect()
        })
        .collect();

    let mut r = 0usize;
    for j in 0..n {
        if r >= n {
            break;
        }
        let mut best: Option<(usize, u128)> = None;
        for k in r..n {
            if m[k][j].is_zero() {
                continue;
            }
            let mag = m[k][j].num.unsigned_abs();
            if best.map(|(_, bm)| mag > bm).unwrap_or(true) {
                best = Some((k, mag));
            }
        }
        let k = match best {
            Some((k, _)) => k,
            None => continue,
        };
        m.swap(r, k);
        let pv = m[r][j];
        for c in 0..(2 * n) {
            m[r][c] = m[r][c].div(pv)?;
        }
        for i in 0..n {
            if i == r || m[i][j].is_zero() {
                continue;
            }
            let f = m[i][j];
            for c in 0..(2 * n) {
                m[i][c] = m[i][c].sub(f.mul(m[r][c])?)?;
            }
        }
        r += 1;
    }

    // Fewer than n pivots means A is singular and has no inverse.
    if r != n {
        return None;
    }

    let mut num = Matrix::new(n, n);
    let mut den = Matrix::new(n, n);
    for i in 0..n {
        for j in 0..n {
            let (p, q) = m[i][n + j].to_pair()?;
            num.set(i, j, p);
            den.set(i, j, q);
        }
    }
    Some((num, den))
}

/// Compute `A^-1` or the rank, exactly.
///
/// Returns `(Some(B), n)` when `A` is `n x n` and `A^-1` is **integral**
/// and representable in `i64`; `(None, r)` otherwise, where `r` is the
/// rank (useful even when the inverse is not integral).
///
/// ## Why this is not the spec's Algorithm Invert
///
/// The spec (Wolfe §4.2.2) divides a row by its pivot. That is only
/// exact when the pivot divides every entry of the row -- a condition
/// that **unimodularity does not imply**. Counterexample:
///
/// ```text
/// A = [ 0 -4  3 -1]        pivot in column 0 is -1, divides
///     [ 0  3 -3  1]        pivot in column 1 is  3, but the row holds
///     [ 0 -1  1  0]        -4 and -1: 3 does not divide them.
///     [-1  6 -6  2]
/// ```
///
/// Algorithm Invert therefore *fails* on perfectly invertible unimodular
/// matrices. This implementation instead uses the unimodular row
/// operations themselves -- swaps, negations, and the Bezout combination
/// `row i, row k <- x·row i + y·row k, -(b/g)·row i + (a/g)·row k`
/// (`x·a + y·b = g` from [`findgcd`]) -- which are exact by construction
/// and preserve invertibility in `GL_n(Z)`. The same operations are
/// applied to an identity matrix, so when the left block reaches `I` the
/// right block is `A^-1`.
///
/// Returning `(None, r)` with `r < n` means the left block has a
/// zero column, i.e. `A` is singular (or rank-deficient); the returned
/// `r` is the rank.
pub fn invert(a: &Matrix) -> (Option<Matrix>, usize) {
    if !a.is_square() {
        return (None, rank(a));
    }
    let n = a.rows;
    if n == 0 {
        return (Some(Matrix::new(0, 0)), 0);
    }

    let mut work = a.clone();
    let mut b = Matrix::identity(n);

    // Work column by column. `r` is the row into which the next pivot
    // will be placed; rows above `r` are already in reduced form.
    let mut r = 0usize;
    for j in 0..n {
        // Step 1: move a nonzero entry of column j into row r.
        let mut found = None;
        for i in r..n {
            if work.get(i, j) != 0 {
                found = Some(i);
                break;
            }
        }
        let k = match found {
            Some(k) => k,
            None => continue, // column j is entirely zero -> singular
        };
        if k != r {
            work.swap_rows(r, k);
            b.swap_rows(r, k);
        }

        // Step 2: clear every entry below the pivot using Bezout
        // combinations, which keep integer entries exact at all times.
        for i in (r + 1)..n {
            let p = work.get(r, j);
            let q = work.get(i, j);
            if q == 0 {
                continue;
            }
            let (g, x, y) = match crate::solver::gcd::try_findgcd(p, q) {
                Some(t) => t,
                None => return (None, rank(a)),
            };
            if g == 0 {
                continue;
            }
            // new row r = x·row r + y·row i     (column j becomes g)
            // new row i = -(q/g)·row r + (p/g)·row i   (column j becomes 0)
            let nr = match work.combine_rows_checked(r, i, x, y) {
                Some(v) => v,
                None => return (None, rank(a)),
            };
            let ni = match work.combine_rows_checked(r, i, -(q / g), p / g) {
                Some(v) => v,
                None => return (None, rank(a)),
            };
            let nbr = match b.combine_rows_checked(r, i, x, y) {
                Some(v) => v,
                None => return (None, rank(a)),
            };
            let nbi = match b.combine_rows_checked(r, i, -(q / g), p / g) {
                Some(v) => v,
                None => return (None, rank(a)),
            };
            work.set_row(r, &nr);
            work.set_row(i, &ni);
            b.set_row(r, &nbr);
            b.set_row(i, &nbi);
        }

        // Step 3: normalise the pivot to +1 (negation is unimodular).
        let p = work.get(r, j);
        if p < 0 {
            work.negate_row(r);
            b.negate_row(r);
        }

        // Step 4: clear every entry above the pivot (it is already 1).
        for i in 0..r {
            let alpha = work.get(i, j);
            if alpha != 0 {
                if work.sub_row_checked(i, r, alpha).is_none()
                    || b.sub_row_checked(i, r, alpha).is_none()
                {
                    return (None, rank(a));
                }
            }
        }

        r += 1;
        if r == n {
            break;
        }
    }

    if r == n {
        // Every column had a nonzero pivot, so `work` is upper triangular
        // with all diagonal entries positive. Its determinant is
        // `+/-det(A)`, so `work == I` exactly when `|det(A)| == 1`; verify
        // rather than assume, since a pivot of, say, 5 would otherwise
        // yield a bogus "inverse".
        if work == Matrix::identity(n) {
            (Some(b), n)
        } else {
            (None, n)
        }
    } else {
        (None, r)
    }
}

/// `|det(A)| == 1` for square `A`, **overflow-immune**.
///
/// Exact integer determinants blow up exponentially in the dimension, so
/// the fraction-free path (`determinant`) frequently returns `None` on
/// matrices whose entries are `i64` -- exactly the unimodular transform
/// accumulators the Hermite reduction produces. Refusing to certify those
/// would make the HNF verifier useless.
///
/// Instead the determinant is evaluated modulo a product `P` of distinct
/// 31-bit primes whose product provably exceeds twice the Hadamard bound
/// of `A`. Then `det ≡ 1 (mod p)` for every prime pins `det = 1`, and
/// `det ≡ -1` for every prime pins `det = -1` -- both are *proofs*, never
/// guesses. A matrix too wide for the prime table, or a residue pattern
/// that does not pin the determinant, returns `false` ("not proven").
pub fn is_unimodular(a: &Matrix) -> bool {
    if !a.is_square() {
        return false;
    }
    let n = a.rows;
    if n == 0 {
        return true; // det of the empty matrix is 1
    }
    // Hadamard: |det(A)| <= prod_i (sqrt(n) * max_j |A[i][j]|), hence
    //   2 * log2|det| <= n * log2(n) + 2 * sum_i log2(mx_i)
    // with mx_i the largest entry magnitude in row i. Estimating from the
    // actual entries rather than from i64::MAX keeps the number of primes -
    // and hence the cost - proportional to the determinant's true size.
    let mut bound2: u64 = 0; // >= 2 * log2|det|
    for i in 0..n {
        let mut mx: u64 = 0;
        for j in 0..n {
            mx = mx.max(a.get(i, j).unsigned_abs());
        }
        // log2(mx + 1) rounded up, >= log2(mx).
        let mx1 = mx.saturating_add(1);
        let bits = 64 - mx1.leading_zeros() as u64;
        bound2 = bound2.saturating_add(2 * bits);
    }
    bound2 =
        bound2.saturating_add((n as u64) * (n as u64).next_power_of_two().trailing_zeros() as u64);
    // Each prime exceeds 2^30, so k primes give a modulus above 2^(30k).
    // Pinning det = +-1 needs 30k > log2|det| + 1, i.e. 60k > bound2 + 2;
    // k = ceil((bound2 + 63) / 60) satisfies that and is always >= 1.
    let k = bound2.saturating_add(63).div_ceil(60) as usize;
    if k > UNIMODULAR_PRIMES.len() {
        return false;
    }
    let mut sign: Option<i8> = None;
    for &p in &UNIMODULAR_PRIMES[..k] {
        let d = match det_mod_p(a, p) {
            Some(v) => v,
            None => return false,
        };
        let s = if d == 1 {
            1i8
        } else if d == p - 1 {
            -1i8
        } else {
            return false; // det is definitively not +-1
        };
        match sign {
            None => sign = Some(s),
            Some(prev) if prev == s => {}
            Some(_) => return false, // +-1 mod different primes pins |det| > 1
        }
    }
    sign == Some(1) || sign == Some(-1)
}

/// Pairwise distinct primes in `(2^30, 2^32)`, each verified prime by trial
/// division. Every prime exceeds `2^30`, so `k` of them give a modulus
/// above `2^(30k)`; all 72 give more than 2160 bits, enough to certify any
/// square `i64` matrix up to `31 x 31` in the worst case. Residue products
/// are formed in `i128` (`p^2 < 2^64`), so no overflow is possible.
static UNIMODULAR_PRIMES: [i64; 72] = [
    2147482661, 2147482663, 2147482681, 2147482693, 2147482697, 2147482739, 2147482763, 2147482801,
    2147482811, 2147482817, 2147482819, 2147482859, 2147482867, 2147482873, 2147482877, 2147482921,
    2147482937, 2147482943, 2147482949, 2147482951, 2147483029, 2147483033, 2147483053, 2147483059,
    2147483069, 2147483077, 2147483123, 2147483137, 2147483171, 2147483179, 2147483237, 2147483249,
    2147483269, 2147483323, 2147483353, 2147483399, 2147483423, 2147483477, 2147483489, 2147483497,
    2147483543, 2147483549, 2147483563, 2147483579, 2147483587, 2147483629, 2147483647, 2147483659,
    2147483693, 2147483713, 2147483743, 2147483777, 2147483783, 2147483813, 2147483857, 2147483867,
    2147483869, 2147483887, 2147483893, 2147483929, 2147483951, 2147483993, 2147483999, 2147484007,
    2147484037, 2147484041, 2147484043, 2147484061, 2147484083, 2147484109, 2147484161, 2147484167,
];

/// `det(A) mod p` for prime `p < 2^31`, by Gaussian elimination. All
/// arithmetic stays in `i64`/`i128` (products of two residues are below
/// `2^62`), so no overflow is possible.
fn det_mod_p(a: &Matrix, p: i64) -> Option<i64> {
    let n = a.rows;
    let mut m: Vec<Vec<i64>> = (0..n)
        .map(|i| (0..n).map(|j| a.get(i, j).rem_euclid(p)).collect())
        .collect();
    let mut det: i64 = 1;
    for c in 0..n {
        let piv = (c..n).find(|&r| m[r][c] != 0)?;
        if piv != c {
            m.swap(c, piv);
            det = (p - det) % p;
        }
        let d = m[c][c];
        det = (det as i128 * d as i128 % p as i128) as i64;
        let inv = mod_inverse(d, p)?;
        for r in (c + 1)..n {
            let f = m[r][c];
            if f == 0 {
                continue;
            }
            let factor = (f as i128 * inv as i128 % p as i128) as i64;
            for j in c..n {
                let v = m[r][j] as i128 - factor as i128 * m[c][j] as i128;
                m[r][j] = (v % p as i128).rem_euclid(p as i128) as i64;
            }
        }
    }
    Some(det)
}

/// `d^{-1} mod p` for prime `p` and `0 < d < p`, via extended Euclid.
fn mod_inverse(d: i64, p: i64) -> Option<i64> {
    let (g, x, _) = crate::solver::gcd::try_findgcd(d, p)?;
    if g != 1 {
        return None;
    }
    Some(x.rem_euclid(p))
}

/// Verify `A * B == I` exactly. Used by tests and by callers that need a
/// cheap correctness check on a candidate inverse.
pub fn is_inverse(a: &Matrix, b: &Matrix) -> bool {
    if !a.is_square() || !b.is_square() || a.rows != b.rows {
        return false;
    }
    match a.matmul(b) {
        Some(p) => p == Matrix::identity(a.rows),
        None => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::gcd::gcd;

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

    /// Random unimodular matrix: start from I and apply random elementary
    /// row operations. Every step has determinant +/-1, so the product does.
    fn rand_unimodular(state: &mut u64, n: usize) -> Matrix {
        let mut m = Matrix::identity(n);
        for _ in 0..(4 * n) {
            let op = xs(state) % 3;
            let i = (xs(state) % n as u64) as usize;
            let k = (xs(state) % n as u64) as usize;
            match op {
                0 => {
                    if i != k {
                        m.swap_rows(i, k);
                    }
                }
                1 => m.negate_row(i),
                _ => {
                    if i != k {
                        let a = rand_in(state, -3, 3);
                        let row = m.combine_rows(i, k, 1, a);
                        m.set_row(i, &row);
                    }
                }
            }
        }
        m
    }

    /// Independent determinant oracle: the Leibniz expansion over all
    /// permutations. Exponential, hence only usable for `n <= 5`.
    fn brute_det(m: &Matrix) -> i128 {
        fn go(m: &Matrix, perm: &mut Vec<usize>, k: usize, total: &mut i128) {
            let n = m.rows;
            if k == n {
                let mut sign = 1i128;
                for i in 0..n {
                    for j in (i + 1)..n {
                        if perm[i] > perm[j] {
                            sign = -sign;
                        }
                    }
                }
                let mut prod: i128 = 1;
                for i in 0..n {
                    prod *= m.get(i, perm[i]) as i128;
                }
                *total += sign * prod;
                return;
            }
            for i in k..n {
                perm.swap(k, i);
                go(m, perm, k + 1, total);
                perm.swap(k, i);
            }
        }
        let n = m.rows;
        let mut perm: Vec<usize> = (0..n).collect();
        let mut total: i128 = 0;
        go(m, &mut perm, 0, &mut total);
        total
    }

    /// Independent rank oracle: the largest `k` with a nonzero `k x k` minor.
    fn brute_rank(m: &Matrix) -> usize {
        let max_k = m.rows.min(m.cols);
        let mut best = 0usize;
        for k in 1..=max_k {
            if best == k - 1 {
                let mut found = false;
                for rs in subsets(m.rows, k) {
                    for cs in subsets(m.cols, k) {
                        let mut sub: Vec<Vec<i64>> = Vec::with_capacity(k);
                        for &r in &rs {
                            sub.push(cs.iter().map(|&c| m.get(r, c)).collect());
                        }
                        let sm = Matrix::from_rows(&sub).unwrap();
                        if brute_det(&sm) != 0 {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                if found {
                    best = k;
                }
            }
        }
        best
    }

    /// All `k`-subsets of `0..n`, ascending.
    fn subsets(n: usize, k: usize) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut cur: Vec<usize> = (0..k).collect();
        if k > n {
            return out;
        }
        loop {
            out.push(cur.clone());
            let mut i = k;
            while i > 0 {
                i -= 1;
                if cur[i] != i + n - k {
                    cur[i] += 1;
                    for j in (i + 1)..k {
                        cur[j] = cur[j - 1] + 1;
                    }
                    break;
                }
                if i == 0 {
                    return out;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Unit tests
    // ------------------------------------------------------------------

    #[test]
    fn matrix_basics() {
        let m = Matrix::from_slice(2, 3, &[1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(m.get(0, 2), 3);
        assert_eq!(m.get(1, 0), 4);
        assert_eq!(m.row(0), vec![1, 2, 3]);
        assert_eq!(m.col(1), vec![2, 5]);
        assert!(Matrix::from_slice(2, 2, &[1, 2, 3]).is_none());
        assert!(Matrix::from_rows(&[vec![1, 2], vec![3]]).is_none());
        assert_eq!(Matrix::identity(3).get(1, 1), 1);
        assert_eq!(Matrix::identity(3).get(0, 1), 0);
        assert!(Matrix::identity(2).is_square());
        assert!(Matrix::new(2, 2).is_zero());
        assert_eq!(
            Matrix::from_slice(2, 2, &[1, 2, 3, 4]).unwrap().transpose(),
            Matrix::from_slice(2, 2, &[1, 3, 2, 4]).unwrap()
        );
    }

    #[test]
    fn determinant_basics() {
        assert_eq!(determinant(&Matrix::new(0, 0)), Some(1));
        assert_eq!(
            determinant(&Matrix::from_slice(1, 1, &[7]).unwrap()),
            Some(7)
        );
        assert_eq!(
            determinant(&Matrix::from_slice(2, 2, &[1, 2, 3, 4]).unwrap()),
            Some(-2)
        );
        assert_eq!(
            determinant(&Matrix::from_slice(2, 2, &[1, 2, 2, 4]).unwrap()),
            Some(0)
        );
        assert_eq!(
            determinant(&Matrix::from_slice(2, 3, &[1, 2, 3, 4, 5, 6]).unwrap()),
            None
        );
        assert_eq!(determinant(&Matrix::identity(5)), Some(1));
    }

    #[test]
    fn invert_unimodular_examples() {
        // [[1,2],[3,4]]^-1 = [[-2,1],[1.5,-0.5]] is not integral.
        let a = Matrix::from_slice(2, 2, &[1, 2, 3, 4]).unwrap();
        let (inv, r) = invert(&a);
        assert!(inv.is_none());
        assert_eq!(r, 2);

        // A swap is unimodular and self-inverse.
        let s = Matrix::from_slice(2, 2, &[0, 1, 1, 0]).unwrap();
        let (inv, r) = invert(&s);
        assert_eq!(r, 2);
        assert_eq!(inv.unwrap(), s);

        // [[1,0],[0,-1]] is its own inverse.
        let d = Matrix::from_slice(2, 2, &[1, 0, 0, -1]).unwrap();
        let (inv, _) = invert(&d);
        assert_eq!(inv.unwrap(), d);
    }

    #[test]
    fn adjugate_examples() {
        let a = Matrix::from_slice(2, 2, &[1, 2, 3, 4]).unwrap();
        let (adj, det) = adjugate(&a).unwrap();
        assert_eq!(det, -2);
        // adj = [[4, -2], [-3, 1]]
        assert_eq!(adj, Matrix::from_slice(2, 2, &[4, -2, -3, 1]).unwrap());
        // adj(A) = det(A) * A^-1  <=>  A * adj(A) = det(A) * I
        let prod = a.matmul(&adj).unwrap();
        assert_eq!(prod, Matrix::from_slice(2, 2, &[-2, 0, 0, -2]).unwrap());
    }

    #[test]
    fn invert_rational_examples() {
        let a = Matrix::from_slice(2, 2, &[1, 2, 3, 4]).unwrap();
        let (num, den) = invert_rational(&a).unwrap();
        // A^-1 = [[-2, 1], [3/2, -1/2]]
        assert_eq!(num, Matrix::from_slice(2, 2, &[-2, 1, 3, -1]).unwrap());
        assert_eq!(den, Matrix::from_slice(2, 2, &[1, 1, 2, 2]).unwrap());
        // Every denominator is positive and the fractions are reduced.
        for i in 0..2 {
            for j in 0..2 {
                assert!(den.get(i, j) > 0);
                assert_eq!(gcd(num.get(i, j), den.get(i, j)), 1);
            }
        }
        assert!(rational_inverse_identity(&a, &num, &den));
        // A singular matrix has no inverse at all.
        let s = Matrix::from_slice(2, 2, &[1, 2, 2, 4]).unwrap();
        assert!(invert_rational(&s).is_none());
        // ... but `invert` still reports its rank.
        assert_eq!(invert(&s), (None, 1));
    }

    #[test]
    fn is_unimodular_basics() {
        assert!(is_unimodular(&Matrix::identity(4)));
        assert!(is_unimodular(
            &Matrix::from_slice(2, 2, &[0, 1, 1, 0]).unwrap()
        ));
        assert!(!is_unimodular(
            &Matrix::from_slice(2, 2, &[1, 2, 3, 4]).unwrap()
        ));
        assert!(!is_unimodular(&Matrix::new(2, 2)));
        assert!(!is_unimodular(&Matrix::new(2, 3)));
    }

    // ------------------------------------------------------------------
    // Fuzz: determinant
    // ------------------------------------------------------------------

    /// `determinant` matches the Leibniz expansion on random square
    /// matrices, in every size from 0 to 5.
    #[test]
    fn fuzz_determinant_vs_leibniz() {
        let mut st = 0x1111_2222_3333_4444u64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..4000 {
            let n = (xs(&mut st) % 6) as usize; // 0..=5
            let m = rand_matrix(&mut st, n, n, 6);
            let got = determinant(&m).unwrap();
            let want = brute_det(&m);
            tested += 1;
            if got as i128 != want {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("n={} det={} want={}\n{}", n, got, want, m));
                }
            }
        }
        println!("determinant fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "determinant disagreed with Leibniz {} times", bad);
    }

    /// `det(A * B) == det(A) * det(B)` on random square matrices.
    #[test]
    fn fuzz_determinant_multiplicative() {
        let mut st = 0x9876_5432_10fe_dcba_u64 as u64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..3000 {
            let n = 1 + (xs(&mut st) % 4) as usize;
            let a = rand_matrix(&mut st, n, n, 5);
            let b = rand_matrix(&mut st, n, n, 5);
            let da = determinant(&a).unwrap();
            let db = determinant(&b).unwrap();
            let prod = match a.matmul(&b) {
                Some(p) => p,
                None => continue,
            };
            let dp = determinant(&prod).unwrap();
            tested += 1;
            let want = da.checked_mul(db);
            if Some(dp) != want {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "n={} det(A)={} det(B)={} det(AB)={} want={:?}",
                        n, da, db, dp, want
                    ));
                }
            }
        }
        println!(
            "determinant multiplicativity fuzz: tested={} bad={}",
            tested, bad
        );
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "det multiplicative identity violated {} times", bad);
    }

    // ------------------------------------------------------------------
    // Fuzz: rank
    // ------------------------------------------------------------------

    /// `rank` matches the largest-nonzero-minor oracle on small matrices,
    /// and `rank + nullity == cols`.
    #[test]
    fn fuzz_rank_vs_minors() {
        let mut st = 0xfeed_face_dead_c0de_u64 as u64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..1500 {
            let r = (xs(&mut st) % 4) as usize;
            let c = (xs(&mut st) % 4) as usize;
            let m = rand_matrix(&mut st, r, c, 3);
            let got = rank(&m);
            let want = brute_rank(&m);
            tested += 1;
            if got != want {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("rank={} want={}\n{}", got, want, m));
                }
            }
            assert_eq!(got + nullity(&m), c);
        }
        println!("rank fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "rank disagreed with the minor oracle {} times", bad);
    }

    // ------------------------------------------------------------------
    // Fuzz: inverse
    // ------------------------------------------------------------------

    /// For random unimodular `A`: `invert` succeeds, `A * A^-1 == I`, and
    /// `is_unimodular` agrees.
    #[test]
    fn fuzz_invert_unimodular() {
        let mut st = 0x0bad_c0de_1234_5678u64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..2000 {
            let n = 1 + (xs(&mut st) % 5) as usize;
            let a = rand_unimodular(&mut st, n);
            if !is_unimodular(&a) {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "unimodular generator produced non-unimodular\n{}",
                        a
                    ));
                }
                continue;
            }
            let (inv, r) = invert(&a);
            tested += 1;
            if r != n {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("unimodular rank {} != {}\n{}", r, n, a));
                }
                continue;
            }
            match inv {
                Some(b) => {
                    if !is_inverse(&a, &b) {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("A*inv != I\nA={}\ninv={}", a, b));
                        }
                    }
                    if !is_unimodular(&b) {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("inverse not unimodular\n{}", b));
                        }
                    }
                }
                None => {
                    bad += 1;
                    if ex.len() < 5 {
                        ex.push(format!("invert failed on unimodular\n{}", a));
                    }
                }
            }
        }
        println!("invert-unimodular fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "unimodular inversion violated {} times", bad);
        assert!(tested > 1500);
    }

    /// On random integer matrices, whenever `invert` returns `Some` the
    /// result must really be the inverse; otherwise the reported rank must
    /// agree with the minor oracle.
    #[test]
    fn fuzz_invert_never_lies() {
        let mut st = 0x2222_3333_4444_5555u64;
        let mut succeeded = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..3000 {
            let n = 1 + (xs(&mut st) % 4) as usize;
            let a = rand_matrix(&mut st, n, n, 4);
            let (inv, r) = invert(&a);
            match inv {
                Some(b) => {
                    succeeded += 1;
                    if !is_inverse(&a, &b) {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("bogus inverse\nA={}\nB={}", a, b));
                        }
                    }
                }
                None => {
                    let want = brute_rank(&a);
                    if r != want {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("rank {} != {}\n{}", r, want, a));
                        }
                    }
                }
            }
        }
        println!(
            "invert-never-lies fuzz: integral inverses={} bad={}",
            succeeded, bad
        );
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "invert reported an incorrect result {} times", bad);
    }

    /// The rational inverse of `A` is `num ⊘ den` entrywise. Check the
    /// defining identity `(num ⊘ den) · A == I` directly.
    fn rational_inverse_identity(a: &Matrix, num: &Matrix, den: &Matrix) -> bool {
        let n = a.rows;
        for i in 0..n {
            for j in 0..n {
                // acc_num / acc_den, reduced as we go.
                let mut acc_num: i128 = 0;
                let mut acc_den: i128 = 1;
                for k in 0..n {
                    let term_n = num.get(i, k) as i128 * a.get(k, j) as i128;
                    let term_d = den.get(i, k) as i128;
                    if term_n == 0 {
                        continue;
                    }
                    let n2 = acc_num * term_d + term_n * acc_den;
                    let d2 = acc_den * term_d;
                    let g = gcd_i128(n2, d2);
                    acc_num = n2 / g;
                    acc_den = d2 / g;
                }
                let want = if i == j { 1i128 } else { 0i128 };
                if acc_num != want * acc_den {
                    return false;
                }
            }
        }
        true
    }

    /// `invert_rational` returns the exact rational inverse of a
    /// nonsingular integer matrix, and the correct rank of a singular one.
    #[test]
    fn fuzz_invert_rational_is_inverse() {
        let mut st = 0x7777_8888_9999_aaaau64;
        let mut tested = 0u64;
        let mut singular = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..2000 {
            let n = 1 + (xs(&mut st) % 5) as usize;
            let a = rand_matrix(&mut st, n, n, 6);
            let want_rank = brute_rank(&a);
            let (num, den) = match invert_rational(&a) {
                Some(v) => v,
                None => {
                    // Singular: `invert` must still report the true rank.
                    singular += 1;
                    let (inv, r) = invert(&a);
                    if inv.is_some() {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("singular matrix inverted\nA={}", a));
                        }
                    }
                    if r != want_rank {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("invert rank {} != {}\nA={}", r, want_rank, a));
                        }
                    }
                    continue;
                }
            };
            tested += 1;
            if want_rank != n {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "nonsingular contract but rank {} < {}\nA={}",
                        want_rank, n, a
                    ));
                }
            }
            // Reduced, positive denominators everywhere.
            for i in 0..n {
                for j in 0..n {
                    if den.get(i, j) <= 0 {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!(
                                "non-positive denominator {}\n{}",
                                den.get(i, j),
                                den
                            ));
                        }
                    }
                    if gcd(num.get(i, j), den.get(i, j)) != 1 {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!("unreduced fraction\nnum={}\nden={}", num, den));
                        }
                    }
                }
            }
            if !rational_inverse_identity(&a, &num, &den) {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "rational inverse does not invert\nA={}\nnum={}\nden={}",
                        a, num, den
                    ));
                }
            }
        }
        println!(
            "invert-rational fuzz: tested={} singular={} bad={}",
            tested, singular, bad
        );
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert!(singular >= 20, "too few singular cases: {}", singular);
        assert_eq!(bad, 0, "rational inversion violated {} times", bad);
    }

    /// Dedicated singular coverage: build matrices with a duplicated row,
    /// which are singular by construction, and check rank plus the
    /// rank-preservation property of the reduction.
    #[test]
    fn fuzz_invert_rational_singular() {
        let mut st = 0x2b2b_2b2b_2b2b_2b2bu64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..1500 {
            let n = 2 + (xs(&mut st) % 4) as usize; // >= 2 so a row can repeat
            let mut a = rand_matrix(&mut st, n, n, 4);
            // Duplicate row 0 into row 1 (possibly negated) -> singular.
            let src = a.row(0);
            let mut dup = src.clone();
            if xs(&mut st) % 2 == 0 {
                for v in dup.iter_mut() {
                    *v = -*v;
                }
            }
            a.set_row(1, &dup);

            let want_rank = brute_rank(&a);
            if want_rank == n {
                // Cannot happen for a duplicated row; guard the oracle.
                bad += 1;
                continue;
            }
            tested += 1;
            // A singular matrix has no rational inverse.
            if invert_rational(&a).is_some() {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("singular matrix inverted\n{}", a));
                }
            }
            // `invert` must report `None` plus the correct rank.
            let (inv, r) = invert(&a);
            if inv.is_some() {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("invert returned Some on singular\n{}", a));
                }
            }
            if r != want_rank {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("singular rank {} != {}\n{}", r, want_rank, a));
                }
            }
            // `is_unimodular` must never accept a singular matrix.
            if is_unimodular(&a) {
                bad += 1;
            }
        }
        println!("singular fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert!(tested > 1000);
        assert_eq!(bad, 0, "singular handling violated {} times", bad);
    }

    /// `adj(A) = det(A) * A^-1`, i.e. `A * adj(A) == det(A) * I`.
    #[test]
    fn fuzz_adjugate_identity() {
        let mut st = 0x1357_9bdf_2468_ace0u64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..800 {
            let n = 1 + (xs(&mut st) % 4) as usize;
            let a = rand_matrix(&mut st, n, n, 4);
            let (adj, det) = match adjugate(&a) {
                Some(v) => v,
                None => continue,
            };
            tested += 1;
            if det as i128 != brute_det(&a) {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("det {} != {}", det, brute_det(&a)));
                }
            }
            let prod = a.matmul(&adj).unwrap();
            for i in 0..n {
                for j in 0..n {
                    let want = if i == j { det } else { 0 };
                    if prod.get(i, j) != want {
                        bad += 1;
                        if ex.len() < 5 {
                            ex.push(format!(
                                "(A*adj)[{},{}] = {} != {}",
                                i,
                                j,
                                prod.get(i, j),
                                want
                            ));
                        }
                    }
                }
            }
        }
        println!("adjugate fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "adjugate identity violated {} times", bad);
    }

    /// `is_unimodular` agrees with `|det| == 1` on random matrices.
    #[test]
    fn fuzz_is_unimodular_matches_det() {
        let mut st = 0x4444_5555_6666_7777u64;
        let mut bad = 0u64;
        for _ in 0..4000 {
            let n = (xs(&mut st) % 4) as usize;
            let m = rand_matrix(&mut st, n, n, 4);
            let want = matches!(determinant(&m), Some(1) | Some(-1));
            if is_unimodular(&m) != want {
                bad += 1;
            }
        }
        assert_eq!(bad, 0);
    }
}
