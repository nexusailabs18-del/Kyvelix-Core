//! Full `Extreme`: exact bounds for rectangular and nonrectangular regions
//! (Wolfe §4.7, reference §2.6).
//!
//! Every dependence test in M0–M2 asks the same question. Given a write
//! subscript `a·i + a0` and a read subscript `b·j + b0` over a loop nest, can
//! the two ever name the same location? Rearranged,
//!
//! ```text
//!     b0 - a0 = a·i - b·j        with lo_k <= i_k, j_k <= hi_k
//! ```
//!
//! so the answer hinges on whether `b0 - a0` lies in the *value set* of the
//! linear form `a·i - b·j` over the iteration space. [`extreme`] computes that
//! value set's extrema over a box; [`bounds_test`] applies the Banerjee-Wolfe
//! lexicographic split, and [`extreme_nonrectangular`] handles the
//! triangular/trapezoidal nests where each bound mentions the outer variables.
//!
//! # The ordering split
//!
//! A dependence `i > j` in lexicographic order means: equal at every outer
//! level, `i_L = j_L + 1` at the level that carries it, free at inner levels.
//! Substituting those cases into the per-level extrema
//!
//! ```text
//!     mu(a, b, p, q, eps) = (a+b)·p + b·eps - (q - p - eps)·(a- - b)+
//!     nu(a, b, p, q, eps) = (a+b)·p + b·eps + (q - p - eps)·(a+ + b)+
//! ```
//!
//! with `x+ = max(x, 0)` and `x- = max(-x, 0)`, gives the reference's split
//! verbatim: outer levels `mu(a_r - b_r, 0, p, q, 0)`, the carried level
//! `mu(a_L, -b_L, p, q, 1)`, inner levels
//! `mu(a_r, 0, ..) + mu(-b_r, 0, ..)`. The dependence is refuted exactly when
//! `b0 - a0` falls outside the accumulated `[mu_bar, nu_bar]`.
//!
//! # Exactness
//!
//! Every product and sum is accumulated in `i128`. When a result leaves `i64`
//! it is clamped **outward** -- a maximum never below the true value, a minimum
//! never above -- so a clamped [`Bounds`] is always a *sound* (if weaker)
//! answer, and its `exact` flag records whether it is also tight.
//! [`bounds_test_holds` ] refuses to refute on a non-exact interval, so no
//! clamping can ever turn a real dependence into a claimed "None".

use crate::solver::inverse::Matrix;

/// An inclusive range `[lower, upper]` of the values a linear form can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub lower: i64,
    pub upper: i64,
    /// `false` once a value has been clamped outward to keep it inside `i64`.
    /// The interval is then still a valid bound, just not a tight one.
    pub exact: bool,
}

impl Bounds {
    /// An exact interval.
    pub fn new(lower: i64, upper: i64) -> Self {
        Bounds {
            lower,
            upper,
            exact: true,
        }
    }

    /// An empty interval -- the bounds test refuted the dependence.
    pub fn empty() -> Self {
        Bounds {
            lower: 1,
            upper: 0,
            exact: true,
        }
    }

    /// The whole of `i64`: "no information".
    pub fn top() -> Self {
        Bounds {
            lower: i64::MIN,
            upper: i64::MAX,
            exact: false,
        }
    }

    /// `true` when no value is possible.
    pub fn is_empty(&self) -> bool {
        self.lower > self.upper
    }

    /// Is `v` inside the interval?
    pub fn contains(&self, v: i64) -> bool {
        self.lower <= v && v <= self.upper
    }

    /// Widen `self` to cover `v` (used when clamping keeps a value sound).
    pub fn meet(&self, other: &Bounds) -> Bounds {
        Bounds {
            lower: self.lower.max(other.lower),
            upper: self.upper.min(other.upper),
            exact: self.exact && other.exact,
        }
    }
}

/// Narrow `i128` to `i64`, clamping outward and reporting whether the value
/// survived intact. Outward means: values above `i64::MAX` come back as
/// `i64::MAX` (a maximum stays an upper bound, a minimum rounded down is still
/// a lower bound of anything representable that the caller compares), values
/// below `i64::MIN` come back as `i64::MIN`.
fn clamp(v: i128) -> (i64, bool) {
    if v > i64::MAX as i128 {
        (i64::MAX, false)
    } else if v < i64::MIN as i128 {
        (i64::MIN, false)
    } else {
        (v as i64, true)
    }
}

/// `x+ = max(x, 0)`.
pub fn pos(x: i64) -> i64 {
    if x > 0 {
        x
    } else {
        0
    }
}

/// `x- = max(-x, 0)`, so `x = x+ - x-` and `|x| = x+ + x-`.
pub fn neg(x: i64) -> i64 {
    if x < 0 {
        -x
    } else {
        0
    }
}

/// `x+` in `i128`, so `i64::MIN` needs no special case.
fn pos128(x: i128) -> i128 {
    if x > 0 {
        x
    } else {
        0
    }
}

/// `x-` in `i128`.
fn neg128(x: i128) -> i128 {
    if x < 0 {
        -x
    } else {
        0
    }
}

/// `mu(a, b, p, q, eps)` -- the minimum of `a·x + b·y` over the triangle
///
/// ```text
///     p <= x <= q        x + eps <= y <= q
/// ```
///
/// ```text
///     mu = (a+b)·p + b·eps - (q - p - eps)·(a- - b)+
/// ```
///
/// Read `y = x + eps + d` with `d >= 0`, so the form is `(a+b)·x + b·eps + b·d`
/// and the `d` budget is `q - p - eps`. Increasing `d` by one pushes `x`'s floor
/// up by one: it buys `b` and costs `a`, so the step pays off exactly when
/// `b > a`, i.e. when `a- - b > 0`. Hence spending the whole budget, which the
/// formula does, is optimal, and the value is `mu`.
fn mu128(a: i128, b: i128, p: i128, q: i128, eps: i128) -> i128 {
    // Saturating throughout: (a+b) and (q-p-eps) each reach ~2^64 from i64
    // inputs, so their product is right at the i128 limit. Saturation only ever
    // widens the interval, which keeps the bound sound.
    let first = (a + b)
        .saturating_mul(p)
        .saturating_add(b.saturating_mul(eps));
    let second = (q - p - eps).saturating_mul(pos128(neg128(a) - b));
    first.saturating_sub(second)
}

/// `nu(a, b, p, q, eps)` -- the maximum of `a·x + b·y` over the same triangle:
///
/// ```text
///     nu = (a+b)·p + b·eps + (q - p - eps)·(a+ + b)+
/// ```
///
/// Here the `d` step buys `b` and costs `a` in the *opposite* direction (a
/// larger `x` floor means a larger `x+`-weighted value when `a > 0`), so the
/// budget is spent when `a + b > 0`.
fn nu128(a: i128, b: i128, p: i128, q: i128, eps: i128) -> i128 {
    let first = (a + b)
        .saturating_mul(p)
        .saturating_add(b.saturating_mul(eps));
    let second = (q - p - eps).saturating_mul(pos128(pos128(a) + b));
    first.saturating_add(second)
}

/// `mu` as `i64`, clamped outward when the value does not fit.
pub fn mu(a: i64, b: i64, p: i64, q: i64, eps: i64) -> i64 {
    mu_exact(a, b, p, q, eps).0
}

/// `nu` as `i64`, clamped outward when the value does not fit.
pub fn nu(a: i64, b: i64, p: i64, q: i64, eps: i64) -> i64 {
    nu_exact(a, b, p, q, eps).0
}

/// [`mu`] also reporting whether the value is tight (never clamped).
pub fn mu_exact(a: i64, b: i64, p: i64, q: i64, eps: i64) -> (i64, bool) {
    clamp(mu128(
        a as i128,
        b as i128,
        p as i128,
        q as i128,
        eps as i128,
    ))
}

/// [`nu`] also reporting whether the value is tight (never clamped).
pub fn nu_exact(a: i64, b: i64, p: i64, q: i64, eps: i64) -> (i64, bool) {
    clamp(nu128(
        a as i128,
        b as i128,
        p as i128,
        q as i128,
        eps as i128,
    ))
}

/// `eps = 1` variant used at the carried level: the minimum of `a·x + b·y` with
/// `x <= y` (weak form, `x + 1 <= y` is the strict one). Exposed because the
/// reference's split calls `mu(a_L, -b_L, p, q, 1)` directly.
pub fn mu_strict(a: i64, b: i64, p: i64, q: i64) -> i64 {
    mu(a, b, p, q, 1)
}

/// The weak (`eps = 0`) counterpart of [`mu_strict`].
pub fn mu_weak(a: i64, b: i64, p: i64, q: i64) -> i64 {
    mu(a, b, p, q, 0)
}

/// Extrema of `a·x + a0` over the inclusive range `[l, u]`.
///
/// ```text
///     min = a+·l + a-·u + a0        max = a+·u + a-·l + a0
/// ```
pub fn extreme_1(a: i64, l: i64, u: i64) -> Bounds {
    extreme_1_off(a, l, u, 0)
}

/// [`extreme_1`] with the constant term folded in.
///
/// `neg(x)` is the *magnitude* `max(-x, 0)`, so the signed negative part
/// `min(x, 0)` is `-neg(x)`. Hence min = `a+·l - a-·u` and
/// max = `a+·u - a-·l` (the reference states these as `a+·l + a−·u` under the
/// signed convention `a− = min(a, 0)`).
pub fn extreme_1_off(a: i64, l: i64, u: i64, a0: i64) -> Bounds {
    if l > u {
        return Bounds::empty();
    }
    let (a, l, u, a0) = (a as i128, l as i128, u as i128, a0 as i128);
    let (lower, e1) = clamp(
        pos128(a)
            .saturating_mul(l)
            .saturating_sub(neg128(a).saturating_mul(u))
            .saturating_add(a0),
    );
    let (upper, e2) = clamp(
        pos128(a)
            .saturating_mul(u)
            .saturating_sub(neg128(a).saturating_mul(l))
            .saturating_add(a0),
    );
    Bounds {
        lower,
        upper,
        exact: e1 && e2,
    }
}

/// Extrema of `Σ a_k·x_k + a0` over the box `lo_k <= x_k <= hi_k` -- the
/// rectangular case of Wolfe's `Extreme`. The variables are independent, so
/// the per-variable extrema sum to the exact bound.
pub fn extreme(a: &[i64], a0: i64, lo: &[i64], hi: &[i64]) -> Bounds {
    if a.len() != lo.len() || a.len() != hi.len() {
        return Bounds::empty();
    }
    let mut sum: i128 = a0 as i128;
    let mut maxsum: i128 = a0 as i128;
    for k in 0..a.len() {
        if lo[k] > hi[k] {
            return Bounds::empty();
        }
        let (ak, l, h) = (a[k] as i128, lo[k] as i128, hi[k] as i128);
        sum += pos128(ak) * l - neg128(ak) * h;
        maxsum += pos128(ak) * h - neg128(ak) * l;
    }
    let (lower, e1) = clamp(sum);
    let (upper, e2) = clamp(maxsum);
    Bounds {
        lower,
        upper,
        exact: e1 && e2,
    }
}

/// The value set of the distance form `a·i - b·j` for a **backward** dependence
/// carried at level `level` (1-based): `i_r = j_r` for outer levels,
/// `i_level = j_level + 1` at the carried level, both free at inner levels.
///
/// Each case reduces to one μ/ν call, because μ/ν are exact over the triangle
/// `p <= x <= q, x + eps <= y <= q`:
///
/// | case | constraint on `(j_r, i_r)` | contribution |
/// |---|---|---|
/// | outer `r + 1 < level` | `i_r = j_r` | `mu(a_r - b_r, 0, p, q, 0)` |
/// | carried `r + 1 = level` | `j_r + 1 <= i_r` | `mu(-b_r, a_r, p, q, 1)` |
/// | inner `r + 1 > level` | none | `mu(a_r,0,..) + mu(-b_r,0,..)` |
///
/// `level == 0` means "no ordering at all" (every level treated as inner), the
/// loop-independent test.
///
/// The reference writes the carried term as `mu(a_L, -b_L, p, q, 1)`; that is
/// the mirrored (`i_L + 1 <= j_L`, forward) arm, which [`bounds_span_forward`]
/// implements. Both arms are needed for a complete Banerjee test, and their
/// union is what [`bounds_test_holds`] consults.
pub fn bounds_span(a: &[i64], b: &[i64], p: &[i64], q: &[i64], level: usize) -> Bounds {
    bounds_span_dir(a, b, p, q, level, true)
}

/// The forward (`i_level + 1 <= j_level`) counterpart of [`bounds_span`].
pub fn bounds_span_forward(a: &[i64], b: &[i64], p: &[i64], q: &[i64], level: usize) -> Bounds {
    bounds_span_dir(a, b, p, q, level, false)
}

fn bounds_span_dir(
    a: &[i64],
    b: &[i64],
    p: &[i64],
    q: &[i64],
    level: usize,
    backward: bool,
) -> Bounds {
    let m = a.len();
    if b.len() != m || p.len() != m || q.len() != m {
        return Bounds::empty();
    }
    let mut mu_sum: i128 = 0;
    let mut nu_sum: i128 = 0;
    let mut exact = true;
    for r in 0..m {
        let (pr, qr) = (p[r] as i128, q[r] as i128);
        if pr > qr {
            return Bounds::empty();
        }
        let (ar, br) = (a[r] as i128, b[r] as i128);
        let (mn, nx) = if level == 0 || r + 1 > level {
            // inner (or unconstrained): the two variables are independent
            (
                mu128(ar, 0, pr, qr, 0) + mu128(-br, 0, pr, qr, 0),
                nu128(ar, 0, pr, qr, 0) + nu128(-br, 0, pr, qr, 0),
            )
        } else if r + 1 < level {
            // outer: the ordering forces i_r == j_r
            (mu128(ar - br, 0, pr, qr, 0), nu128(ar - br, 0, pr, qr, 0))
        } else {
            // carried: a strict one-step gap on this level. The triangle is
            // empty when the level has a single iteration, so no dependence is
            // carried here and this arm refutes.
            if qr - pr < 1 {
                return Bounds::empty();
            }
            if backward {
                // j_r + 1 <= i_r: x = j_r (coefficient -b_r), y = i_r (a_r)
                (mu128(-br, ar, pr, qr, 1), nu128(-br, ar, pr, qr, 1))
            } else {
                // i_r + 1 <= j_r: x = i_r (a_r), y = j_r (coefficient -b_r)
                (mu128(ar, -br, pr, qr, 1), nu128(ar, -br, pr, qr, 1))
            }
        };
        let (l, e1) = clamp(mn);
        let (u, e2) = clamp(nx);
        exact &= e1 && e2;
        mu_sum += l as i128;
        nu_sum += u as i128;
    }
    let (lower, e1) = clamp(mu_sum);
    let (upper, e2) = clamp(nu_sum);
    Bounds {
        lower,
        upper,
        exact: exact && e1 && e2,
    }
}

/// The reference's `bounds_test` (Wolfe §4.7): the interval
/// `[mu_bar, nu_bar]` of the distance form `a·i - b·j` under the ordering
/// carried at `level`. The two subscripts `a·i + a0` (write) and `b·j + b0`
/// (read) can collide only if
///
/// ```text
///     mu_bar <= b0 - a0 <= nu_bar
/// ```
///
/// so an interval that misses `b0 - a0` refutes the dependence. `a0`/`b0` are
/// carried in the signature to keep the reference's call shape; the comparison
/// itself lives in [`bounds_test_holds`].
pub fn bounds_test(
    a: &[i64],
    _a0: i64,
    b: &[i64],
    _b0: i64,
    p: &[i64],
    q: &[i64],
    level: usize,
) -> Bounds {
    bounds_span(a, b, p, q, level)
}

/// `true` when the bounds test leaves some dependence possible -- either arm
/// (backward or forward) covers `b0 - a0`, or the interval was clamped and the
/// test must not refute. This is the sound refutation gate for M0 Stage 4:
/// `false` means *proven* no dependence at `level`.
pub fn bounds_test_holds(
    a: &[i64],
    a0: i64,
    b: &[i64],
    b0: i64,
    p: &[i64],
    q: &[i64],
    level: usize,
) -> bool {
    let need = match (b0 as i128).checked_sub(a0 as i128) {
        Some(v) => v,
        // The offsets themselves overflow: inconclusive, never refute.
        None => return true,
    };
    for backward in [true, false] {
        let span = bounds_span_dir(a, b, p, q, level, backward);
        if span.is_empty() {
            // this arm has no admissible iteration pair at all
            continue;
        }
        if !span.exact {
            // clamped: cannot conclude, and "cannot conclude" means possible.
            return true;
        }
        if (span.lower as i128) <= need && need <= (span.upper as i128) {
            return true;
        }
    }
    false
}

/// Every arm of the test at once, for callers that want the intervals rather
/// than a yes/no: `(backward, forward)` spans of `a·i - b·j`.
pub fn bounds_test_arms(
    a: &[i64],
    b: &[i64],
    p: &[i64],
    q: &[i64],
    level: usize,
) -> (Bounds, Bounds) {
    (
        bounds_span_dir(a, b, p, q, level, true),
        bounds_span_dir(a, b, p, q, level, false),
    )
}

/// Extrema of `f·x + f0` over the *nonrectangular* (triangular / trapezoidal)
/// region
///
/// ```text
///     ll_i + Σ_{j<i} l[i][j]·x_j   <=   x_i   <=   uu_i + Σ_{j<i} u[i][j]·x_j
/// ```
///
/// for `i = 1..m`, with `l` and `u` strictly lower triangular (`l[i][j]` and
/// `u[i][j]` are read only for `j < i`) and `ll`/`uu` the constant parts.
/// `for (i = 0..N) for (j = 0..i)` is `ll = [0, 0]`, `uu = [N-1, 0]`,
/// `u[1][0] = 1`, everything else zero.
///
/// # The recursion (Wolfe's `Extreme`, §4.7.2)
///
/// The innermost variable is eliminated first, each time substituting its
/// affine bound into the running bounding functions. With `A` the upper arm --
/// `A[0]` the constant, `A[k]` the coefficient of `x_k`:
///
/// ```text
///     for n = m downto 1:
///         c = A[n]
///         A[0] += c+ · uu[n-1] - c- · ll[n-1]
///         for j = 1 .. n-1:
///             A[j] += c+ · u[n-1][j-1] - c- · l[n-1][j-1]
/// ```
///
/// The step is *pointwise valid*: `c·x_n <= c+·u_n(x) - c-·l_n(x)` holds at
/// every point of the region because `l_n <= x_n <= u_n` and
/// `c·x_n <= c+·u_n` / `c·x_n <= -c-·l_n` according to the sign of `c`. So the
/// final `A[0]` is always a sound maximum, and the mirrored arm `B` (swap `l`
/// for `u`) is always a sound minimum. On nests whose bound functions are
/// monotone nondecreasing -- every triangular nest, since `l = 0` and `u >= 0`
/// there -- the bound is attained, and the interval is exact.
///
/// # Deviation from the reference pseudocode
///
/// The reference writes the accumulation as `+ neg(a_n)·ll[n-1]`. That is wrong
/// under its own definition `neg(x) = max(-x, 0)`: the negative coefficient
/// case needs `-c-·l_n`, since multiplying the *lower* bound inequality by a
/// negative number flips it. The subtraction here is what makes `min <= max`
/// hold on every example; see the fuzz test against enumeration.
pub fn extreme_nonrectangular(
    f: &[i64],
    f0: i64,
    l: &Matrix,
    ll: &[i64],
    u: &Matrix,
    uu: &[i64],
) -> Bounds {
    let m = f.len();
    if ll.len() != m || uu.len() != m || l.rows < m || l.cols < m || u.rows < m || u.cols < m {
        return Bounds::empty();
    }
    // Replace each level's bound function by the implied constant whenever that
    // is pointwise no worse, then run the recursion on the tightened region.
    // Plain substitution alone is only a sound over-approximation; the
    // tightening is what recovers exactness on nests where a level is feasible
    // over part of its outer level's range.
    let (tl, tu, tll, tuu) = match tighten_region(l, u, ll, uu) {
        None => return Bounds::empty(),
        Some(r) => r,
    };
    let (l, u, ll, uu) = (&tl, &tu, tll.as_slice(), tuu.as_slice());

    // A: upper arm. B: lower arm. Index 0 is the constant term, index k the
    // coefficient of x_k (1-based variable numbering).
    let mut a: Vec<i128> = Vec::with_capacity(m + 1);
    let mut b: Vec<i128> = Vec::with_capacity(m + 1);
    a.push(f0 as i128);
    b.push(f0 as i128);
    for k in 0..m {
        a.push(f[k] as i128);
        b.push(f[k] as i128);
    }

    for n in (1..=m).rev() {
        let (an, bn) = (a[n], b[n]);
        // The variable's own constant bounds. Every product and sum saturates:
        // coefficients compound level by level and can pass far beyond i64, and
        // saturation in the widening direction never makes a bound tighter.
        a[0] = a[0]
            .saturating_add(pos128(an).saturating_mul(uu[n - 1] as i128))
            .saturating_sub(neg128(an).saturating_mul(ll[n - 1] as i128));
        b[0] = b[0]
            .saturating_add(pos128(bn).saturating_mul(ll[n - 1] as i128))
            .saturating_sub(neg128(bn).saturating_mul(uu[n - 1] as i128));
        // Its dependence on the variables that survive.
        for j in 1..n {
            let (unj, lnj) = (u.get(n - 1, j - 1) as i128, l.get(n - 1, j - 1) as i128);
            a[j] = a[j]
                .saturating_add(pos128(an).saturating_mul(unj))
                .saturating_sub(neg128(an).saturating_mul(lnj));
            b[j] = b[j]
                .saturating_add(pos128(bn).saturating_mul(lnj))
                .saturating_sub(neg128(bn).saturating_mul(unj));
        }
    }

    let (lower, e1) = clamp(b[0]);
    let (upper, e2) = clamp(a[0]);
    Bounds {
        lower,
        upper,
        exact: e1 && e2,
    }
}

/// The precondition of [`extreme_nonrectangular`]: `l` and `u` must be strictly
/// lower triangular, i.e. level `i`'s bounds may only mention `x_0 .. x_{i-1}`.
/// A caller building the region from a parsed nest should check this before
/// trusting the result.
pub fn region_is_triangular(l: &Matrix, u: &Matrix, m: usize) -> bool {
    for i in 0..m.min(l.rows) {
        for j in i..m.min(l.cols) {
            if l.get(i, j) != 0 {
                return false;
            }
        }
    }
    for i in 0..m.min(u.rows) {
        for j in i..m.min(u.cols) {
            if u.get(i, j) != 0 {
                return false;
            }
        }
    }
    true
}

/// `true` when the region is *provably* empty: the implied box of
/// [`tighten_bounds`] collapses, or a level is infeasible even at the widest
/// outer point.
///
/// Sound but not complete -- interval relaxation can miss emptiness created by
/// coupling between levels, and nothing unsound hinges on that:
/// [`extreme_nonrectangular`] stays a valid bound on the value set either way.
pub fn region_is_empty(l: &Matrix, u: &Matrix, ll: &[i64], uu: &[i64], m: usize) -> bool {
    match tighten_bounds(l, u, ll, uu, m) {
        None => true,
        Some((lo, hi)) => (0..m).any(|i| lo[i] > hi[i]),
    }
}

/// `extreme_nonrectangular` specialised to the rectangular nest, where `l` and
/// `u` are zero and every level's bounds are the constants `ll`/`uu`. Routes
/// through [`extreme`] so the result is exact by construction.
pub fn extreme_rectangular(f: &[i64], f0: i64, ll: &[i64], uu: &[i64]) -> Bounds {
    extreme(f, f0, ll, uu)
}

/// Tighten the constant parts of a triangular region into an implied box, or
/// return `None` when the tightening proves the region empty.
///
/// Two rules hold at every feasible point and are applied to a fixpoint:
///
/// 1. *own bounds*: `x_i >= ll_i + Σ_{j<i} l[i][j]·x_j`, so `x_i` is at least the
///    minimum of that expression over the current box of the outer levels
///    (dually for the upper bound).
/// 2. *feasibility*: level `i`'s interval must be nonempty, i.e.
///    `Σ_{j<i} (u[i][j] - l[i][j])·x_j >= ll_i - uu_i`. Isolating one `x_j` and
///    bounding the rest by the box gives an implied bound on `x_j`.
///
/// Rule 2 is what makes [`extreme_nonrectangular`] exact on nests where a level
/// is feasible only over part of its outer level's range -- the shape of
/// `for (i = 0..N) for (j = 2..=i)`, where the outer loop truly starts at `i = 2`.
/// Without it, plain substitution yields a sound but loose bound.
///
/// Every bound produced is valid at every feasible point, so tightening only
/// ever shrinks the description toward the true feasible set, never below it.
pub fn tighten_bounds(
    l: &Matrix,
    u: &Matrix,
    ll: &[i64],
    uu: &[i64],
    m: usize,
) -> Option<(Vec<i128>, Vec<i128>)> {
    if ll.len() < m || uu.len() < m {
        return None;
    }
    // The box starts as the whole of i64, which is trivially a valid
    // over-approximation of the feasible set. It must NOT start as (ll, uu):
    // ll_i and uu_i are the constant *parts* of level i's bounds, not bounds on
    // x_i, since the affine terms can push x_i far outside them. Starting there
    // would refute feasible regions such as `1 <= x_1 <= x_0` with `x_0 <= 1`.
    let mut lo: Vec<i128> = vec![i64::MIN as i128; m];
    let mut hi: Vec<i128> = vec![i64::MAX as i128; m];

    // lo only grows and hi only shrinks, so propagation is monotone and
    // terminates; the round cap only bounds work, and stopping early keeps the
    // box loose (sound) rather than wrong.
    for _ in 0..(m + 3) {
        let mut changed = false;

        for i in 0..m {
            let mut lower = ll[i] as i128;
            let mut upper = uu[i] as i128;
            for j in 0..i {
                let (lj, uj) = (l.get(i, j) as i128, u.get(i, j) as i128);
                lower = lower
                    .saturating_add(pos128(lj).saturating_mul(lo[j]))
                    .saturating_sub(neg128(lj).saturating_mul(hi[j]));
                upper = upper
                    .saturating_add(pos128(uj).saturating_mul(hi[j]))
                    .saturating_sub(neg128(uj).saturating_mul(lo[j]));
            }
            if lower > lo[i] {
                lo[i] = lower;
                changed = true;
            }
            if upper < hi[i] {
                hi[i] = upper;
                changed = true;
            }
            if lo[i] > hi[i] {
                return None;
            }
        }

        for i in 1..m {
            let d = (ll[i] as i128).saturating_sub(uu[i] as i128);
            let mut max_all: i128 = 0;
            let mut coef: Vec<i128> = Vec::with_capacity(i);
            for k in 0..i {
                let c = (u.get(i, k) as i128).saturating_sub(l.get(i, k) as i128);
                max_all = max_all
                    .saturating_add(pos128(c).saturating_mul(hi[k]))
                    .saturating_sub(neg128(c).saturating_mul(lo[k]));
                coef.push(c);
            }
            if max_all < d {
                return None; // infeasible even at the widest point
            }
            for j in 0..i {
                let c = coef[j];
                if c == 0 {
                    continue;
                }
                // the other variables contribute at most `others_max`
                let others_max = max_all
                    .saturating_sub(pos128(c).saturating_mul(hi[j]))
                    .saturating_add(neg128(c).saturating_mul(lo[j]));
                let need = d.saturating_sub(others_max); // what c·x_j must reach
                if c > 0 {
                    let lb = ceil_div(need, c);
                    if lb > lo[j] {
                        lo[j] = lb;
                        changed = true;
                    }
                } else {
                    // c < 0: x_j <= floor(need / c) = -ceil(need / (-c))
                    let ub = ceil_div(need, c.saturating_neg()).saturating_neg();
                    if ub < hi[j] {
                        hi[j] = ub;
                        changed = true;
                    }
                }
                if lo[j] > hi[j] {
                    return None;
                }
            }
        }

        if !changed {
            break;
        }
    }
    Some((lo, hi))
}

/// `ceil(a / b)` for `b > 0`, in `i128`.
fn ceil_div(a: i128, b: i128) -> i128 {
    debug_assert!(b > 0);
    // ceil(a/b) = -floor(-a/b); div_euclid is floor for positive divisor.
    (-a).div_euclid(b).saturating_neg()
}

/// `min(v, i64::MAX)`, i.e. round an upper bound toward `-inf` only far enough
/// to fit `i64`, never past it (an upper bound rounded up stays an upper bound;
/// `i64::MAX` is itself valid because every variable is an `i64`).
fn hi_to_i64(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        // the implied bound is below every representable value: the region is
        // empty for this level, and i64::MIN keeps the description valid.
        i64::MIN
    } else {
        v as i64
    }
}

/// `max(v, i64::MIN)`, the dual of [`hi_to_i64`].
fn lo_to_i64(v: i128) -> i64 {
    if v < i64::MIN as i128 {
        i64::MIN
    } else if v > i64::MAX as i128 {
        i64::MAX
    } else {
        v as i64
    }
}

/// A region description with each level's bound function replaced by the
/// implied constant whenever that is pointwise no worse over the tightened box.
///
/// Both `x_i <= uu_i + Σ_{j<i} u[i][j]·x_j` and the implied bound `x_i <= H_i`
/// hold at every feasible point. Where `H_i` never exceeds the affine row over
/// the box, the *constant* is the better bound function: it carries no
/// coefficients, so [`extreme_nonrectangular`] can no longer follow a level's
/// bound into the outer variables it does not actually depend on. That is what
/// makes the recursion exact on nests where a level is infeasible over part of
/// its outer level's range.
///
/// Returns `None` when the region is provably empty.
pub fn tighten_region(
    l: &Matrix,
    u: &Matrix,
    ll: &[i64],
    uu: &[i64],
) -> Option<(Matrix, Matrix, Vec<i64>, Vec<i64>)> {
    let m = ll.len();
    if uu.len() != m || l.rows < m || l.cols < m || u.rows < m || u.cols < m {
        return None;
    }
    let mut cl = l.clone();
    let mut cu = u.clone();
    let mut cll: Vec<i64> = ll.to_vec();
    let mut cuu: Vec<i64> = uu.to_vec();

    // Each pass can only shrink bounds (or drop a coefficient once its constant
    // is provably as good), so the chain is monotone and terminates. The cap
    // bounds the work; stopping early keeps the description loose, never wrong.
    for _ in 0..(m + 2) {
        match tighten_once(&cl, &cu, &cll, &cuu, m) {
            None => return None,
            Some((nl, nu, nll, nuu)) => {
                let same = (0..m).all(|i| nll[i] == cll[i] && nuu[i] == cuu[i])
                    && row_eq(&nl, &cl, m)
                    && row_eq(&nu, &cu, m);
                cl = nl;
                cu = nu;
                cll = nll;
                cuu = nuu;
                if same {
                    break;
                }
            }
        }
    }
    Some((cl, cu, cll, cuu))
}

fn row_eq(a: &Matrix, b: &Matrix, m: usize) -> bool {
    (0..m).all(|i| (0..m).all(|j| a.get(i, j) == b.get(i, j)))
}

/// One tightening pass. See [`tighten_region`] for the loop.
fn tighten_once(
    l: &Matrix,
    u: &Matrix,
    ll: &[i64],
    uu: &[i64],
    m: usize,
) -> Option<(Matrix, Matrix, Vec<i64>, Vec<i64>)> {
    let (blo, bhi) = tighten_bounds(l, u, ll, uu, m)?;

    let mut l2 = Matrix::new(m, m);
    let mut u2 = Matrix::new(m, m);
    let mut ll2: Vec<i64> = Vec::with_capacity(m);
    let mut uu2: Vec<i64> = Vec::with_capacity(m);

    for i in 0..m {
        // least value of the affine upper row, greatest of the lower row, over
        // the tightened box
        let mut row_min_u = uu[i] as i128;
        let mut row_max_l = ll[i] as i128;
        for j in 0..i {
            let (uj, lj) = (u.get(i, j) as i128, l.get(i, j) as i128);
            row_min_u = row_min_u
                .saturating_add(pos128(uj).saturating_mul(blo[j]))
                .saturating_sub(neg128(uj).saturating_mul(bhi[j]));
            row_max_l = row_max_l
                .saturating_add(pos128(lj).saturating_mul(bhi[j]))
                .saturating_sub(neg128(lj).saturating_mul(blo[j]));
        }

        // upper bound function
        let h = hi_to_i64(bhi[i]);
        if (h as i128) <= row_min_u && h <= uu[i] {
            uu2.push(h); // constant, and no looser than the row anywhere
        } else {
            uu2.push(uu[i]);
            for j in 0..i {
                u2.set(i, j, u.get(i, j));
            }
        }

        // lower bound function
        let lo = lo_to_i64(blo[i]);
        if (lo as i128) >= row_max_l && lo >= ll[i] {
            ll2.push(lo);
        } else {
            ll2.push(ll[i]);
            for j in 0..i {
                l2.set(i, j, l.get(i, j));
            }
        }
    }

    Some((l2, u2, ll2, uu2))
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

    // ---------- exactness of the pieces against brute force ----------

    /// Brute-force min of `a·x + b·y` over `p <= x <= q`, `x + eps <= y <= q`.
    fn brute_mu(a: i64, b: i64, p: i64, q: i64, eps: i64) -> Option<i64> {
        let mut best: Option<i64> = None;
        for x in p..=q {
            for y in (x + eps)..=q {
                let v = a * x + b * y;
                best = Some(best.map_or(v, |c: i64| c.min(v)));
            }
        }
        best
    }

    /// Brute-force max of the same form over the same triangle.
    fn brute_nu(a: i64, b: i64, p: i64, q: i64, eps: i64) -> Option<i64> {
        let mut best: Option<i64> = None;
        for x in p..=q {
            for y in (x + eps)..=q {
                let v = a * x + b * y;
                best = Some(best.map_or(v, |c: i64| c.max(v)));
            }
        }
        best
    }

    #[test]
    fn mu_nu_match_brute_force() {
        // The closed forms are the exact extrema over the triangle, not an
        // interval relaxation: every value must match to the unit.
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        let mut tested = 0usize;
        for a in -4i64..=4 {
            for b in -4i64..=4 {
                for p in 0i64..=3 {
                    for q in p..=4 {
                        for eps in 0i64..=1 {
                            if q - p < eps {
                                continue; // empty triangle
                            }
                            let bm = brute_mu(a, b, p, q, eps).expect("nonempty");
                            let bn = brute_nu(a, b, p, q, eps).expect("nonempty");
                            assert_eq!(mu(a, b, p, q, eps), bm, "mu({a},{b},{p},{q},{eps})");
                            assert_eq!(nu(a, b, p, q, eps), bn, "nu({a},{b},{p},{q},{eps})");
                            tested += 1;
                        }
                    }
                }
            }
        }
        // Also random coefficients, wider ranges.
        for _ in 0..2000 {
            let a = rand_in(&mut st, -9, 9);
            let b = rand_in(&mut st, -9, 9);
            let p = rand_in(&mut st, -5, 5);
            let q = p + rand_in(&mut st, 0, 6);
            for eps in 0i64..=1 {
                if q - p < eps {
                    continue;
                }
                assert_eq!(mu(a, b, p, q, eps), brute_mu(a, b, p, q, eps).unwrap());
                assert_eq!(nu(a, b, p, q, eps), brute_nu(a, b, p, q, eps).unwrap());
                tested += 1;
            }
        }
        assert!(tested > 3000, "expected wide coverage, got {tested}");
    }

    #[test]
    fn extreme_1_matches_brute_force() {
        for a in -5i64..=5 {
            for l in -4i64..=3 {
                for u in l..=4 {
                    let bnd = extreme_1(a, l, u);
                    let mut vals = Vec::new();
                    for x in l..=u {
                        vals.push(a * x);
                    }
                    assert_eq!(bnd.lower, *vals.iter().min().unwrap(), "a={a} [{l},{u}]");
                    assert_eq!(bnd.upper, *vals.iter().max().unwrap(), "a={a} [{l},{u}]");
                    assert!(bnd.exact);
                }
            }
        }
        // With an offset.
        let mut st = 1u64;
        for _ in 0..500 {
            let a = rand_in(&mut st, -7, 7);
            let a0 = rand_in(&mut st, -9, 9);
            let l = rand_in(&mut st, -4, 4);
            let u = l + rand_in(&mut st, 0, 5);
            let bnd = extreme_1_off(a, l, u, a0);
            let (mut mn, mut mx) = (i64::MAX, i64::MIN);
            for x in l..=u {
                let v = a * x + a0;
                mn = mn.min(v);
                mx = mx.max(v);
            }
            assert_eq!(bnd.lower, mn);
            assert_eq!(bnd.upper, mx);
        }
    }

    #[test]
    fn extreme_matches_brute_force_over_box() {
        let mut st = 0xc0ffeeu64;
        for _ in 0..400 {
            let m = rand_in(&mut st, 1, 3) as usize;
            let a: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
            let a0 = rand_in(&mut st, -4, 4);
            let lo: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -2, 0)).collect();
            let hi: Vec<_> = lo.iter().map(|&l| l + rand_in(&mut st, 0, 2)).collect();
            let bnd = extreme(&a, a0, &lo, &hi);
            let (mut mn, mut mx) = (i64::MAX, i64::MIN);
            let mut x = vec![0i64; m];
            let total: usize = hi
                .iter()
                .zip(lo.iter())
                .map(|(&h, &l)| (h - l + 1) as usize)
                .product();
            for n in 0..total {
                let mut rest = n;
                for k in 0..m {
                    let w = (hi[k] - lo[k] + 1) as usize;
                    x[k] = lo[k] + (rest % w) as i64;
                    rest /= w;
                }
                let v: i64 = a.iter().zip(x.iter()).map(|(&c, &v)| c * v).sum::<i64>() + a0;
                mn = mn.min(v);
                mx = mx.max(v);
            }
            assert_eq!(bnd.lower, mn, "min for {a:?} {lo:?}..{hi:?}");
            assert_eq!(bnd.upper, mx, "max for {a:?} {lo:?}..{hi:?}");
            assert!(bnd.exact);
        }
    }

    // ---------- the bounds test against a lexicographic oracle ----------

    /// Exact min/max of `a·i - b·j` over the pairs admissible to one arm:
    /// outer levels equal, the carried level strictly ordered, inner free.
    /// `None` when the arm admits no pair at all.
    fn brute_span_dir(
        a: &[i64],
        b: &[i64],
        p: &[i64],
        q: &[i64],
        level: usize,
        backward: bool,
    ) -> Option<(i64, i64)> {
        let m = a.len();
        let sizes: Vec<i64> = (0..m).map(|r| q[r] - p[r] + 1).collect();
        if sizes.iter().any(|&s| s <= 0) {
            return None;
        }
        let per: usize = sizes.iter().map(|&s| s as usize).product();
        let mut mn: Option<i64> = None;
        let mut mx: Option<i64> = None;
        let mut i = vec![0i64; m];
        let mut j = vec![0i64; m];
        for n in 0..(per * per) {
            let mut rest = n;
            for r in 0..m {
                let s = sizes[r] as usize;
                j[r] = p[r] + (rest % s) as i64;
                rest /= s;
            }
            for r in 0..m {
                let s = sizes[r] as usize;
                i[r] = p[r] + (rest % s) as i64;
                rest /= s;
            }
            let mut ok = true;
            for r in 0..m {
                if level == 0 || r + 1 > level {
                    // unconstrained
                } else if r + 1 < level {
                    ok &= i[r] == j[r];
                } else if backward {
                    ok &= i[r] >= j[r] + 1;
                } else {
                    ok &= j[r] >= i[r] + 1;
                }
            }
            if !ok {
                continue;
            }
            let v: i64 = (0..m).map(|r| a[r] * i[r] - b[r] * j[r]).sum();
            mn = Some(mn.map_or(v, |c| c.min(v)));
            mx = Some(mx.map_or(v, |c| c.max(v)));
        }
        match (mn, mx) {
            (Some(lo), Some(hi)) => Some((lo, hi)),
            _ => None,
        }
    }

    #[test]
    fn bounds_span_matches_lexicographic_brute_force() {
        let mut st = 0x5eed_beefu64;
        let mut cases = 0usize;
        for _ in 0..600 {
            let m = rand_in(&mut st, 1, 2) as usize;
            let a: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
            let b: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
            let p: Vec<i64> = (0..m).map(|_| rand_in(&mut st, 0, 1)).collect();
            let q: Vec<i64> = p.iter().map(|&v| v + rand_in(&mut st, 0, 2)).collect();
            for level in 0..=m {
                for backward in [true, false] {
                    let got = bounds_span_dir(&a, &b, &p, &q, level, backward);
                    let want = brute_span_dir(&a, &b, &p, &q, level, backward);
                    match want {
                        None => assert!(
                            got.is_empty(),
                            "arm {backward} level {level} a={a:?} b={b:?} should refute, got {got:?}"
                        ),
                        Some((lo, hi)) => {
                            assert_eq!(
                                (got.lower, got.upper),
                                (lo, hi),
                                "arm={backward} level={level} a={a:?} b={b:?} p={p:?} q={q:?}"
                            );
                            assert!(got.exact);
                        }
                    }
                    cases += 1;
                }
            }
        }
        assert!(cases > 2000, "only {cases} cases");
    }

    #[test]
    fn bounds_test_holds_never_refutes_a_real_dependence() {
        // The soundness property M0 relies on: when two subscripts really do
        // collide at an admissible ordered pair, the test must say "possible".
        let mut st = 0xa11ce_u64;
        let mut witnessed = 0usize;
        for _ in 0..1500 {
            let m = rand_in(&mut st, 1, 2) as usize;
            let a: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
            let b: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
            let p: Vec<i64> = (0..m).map(|_| rand_in(&mut st, 0, 1)).collect();
            let q: Vec<i64> = p.iter().map(|&v| v + rand_in(&mut st, 0, 2)).collect();
            let a0 = rand_in(&mut st, -3, 3);
            let b0 = rand_in(&mut st, -3, 3);
            for level in 1..=m {
                let sizes: Vec<i64> = (0..m).map(|r| q[r] - p[r] + 1).collect();
                let per: usize = sizes.iter().map(|&s| s as usize).product();
                let mut found = false;
                for n in 0..(per * per) {
                    let mut rest = n;
                    let mut j = vec![0i64; m];
                    let mut i = vec![0i64; m];
                    for r in 0..m {
                        let s = sizes[r] as usize;
                        j[r] = p[r] + (rest % s) as i64;
                        rest /= s;
                    }
                    for r in 0..m {
                        let s = sizes[r] as usize;
                        i[r] = p[r] + (rest % s) as i64;
                        rest /= s;
                    }
                    let mut ok = true;
                    for r in 0..m {
                        if r + 1 > level {
                            // inner level: free
                        } else if r + 1 < level {
                            ok &= i[r] == j[r];
                        } else {
                            ok &= i[r] > j[r];
                        }
                    }
                    if !ok {
                        continue;
                    }
                    let lhs: i64 = (0..m).map(|r| a[r] * i[r]).sum::<i64>() + a0;
                    let rhs: i64 = (0..m).map(|r| b[r] * j[r]).sum::<i64>() + b0;
                    if lhs == rhs {
                        found = true;
                        break;
                    }
                }
                if found {
                    witnessed += 1;
                    assert!(
                        bounds_test_holds(&a, a0, &b, b0, &p, &q, level),
                        "refuted a real dependence: a={a:?} b={b:?} a0={a0} b0={b0} p={p:?} q={q:?} level={level}"
                    );
                }
            }
        }
        assert!(witnessed > 50, "only {witnessed} collisions generated");
    }

    #[test]
    fn bounds_test_matches_naive_when_unconstrained() {
        // level == 0 imposes no ordering, so the span is the naive box span of
        // a·i - b·j over independent i, j.
        for a in -3i64..=3 {
            for b in -3i64..=3 {
                let span = bounds_span(&[a], &[b], &[0], &[4], 0);
                let mut mn = i64::MAX;
                let mut mx = i64::MIN;
                for i in 0..=4 {
                    for j in 0..=4 {
                        let v = a * i - b * j;
                        mn = mn.min(v);
                        mx = mx.max(v);
                    }
                }
                assert_eq!((span.lower, span.upper), (mn, mx), "a={a} b={b}");
            }
        }
    }

    /// Enumerate the nonrectangular region with a generous constant cap and
    /// return the true extrema of f·x + f0 (or None if the region is empty).
    fn brute_nonrectangular(
        f: &[i64],
        f0: i64,
        l: &Matrix,
        ll: &[i64],
        u: &Matrix,
        uu: &[i64],
        cap: i64,
    ) -> Option<(i64, i64)> {
        let m = f.len();
        let mut x = vec![0i64; m];
        let mut mn: Option<i64> = None;
        let mut mx: Option<i64> = None;
        fn rec(
            i: usize,
            x: &mut Vec<i64>,
            l: &Matrix,
            ll: &[i64],
            u: &Matrix,
            uu: &[i64],
            cap: i64,
            f: &[i64],
            f0: i64,
            mn: &mut Option<i64>,
            mx: &mut Option<i64>,
        ) {
            let m = f.len();
            if i == m {
                let v: i64 = f.iter().zip(x.iter()).map(|(&c, &v)| c * v).sum::<i64>() + f0;
                *mn = Some(mn.map_or(v, |b| b.min(v)));
                *mx = Some(mx.map_or(v, |b| b.max(v)));
                return;
            }
            let mut lo = ll[i];
            let mut hi = uu[i];
            for j in 0..i {
                lo += l.get(i, j) * x[j];
                hi += u.get(i, j) * x[j];
            }
            if lo > hi {
                return;
            }
            lo = lo.max(-cap);
            hi = hi.min(cap);
            for v in lo..=hi {
                x[i] = v;
                rec(i + 1, x, l, ll, u, uu, cap, f, f0, mn, mx);
            }
            x[i] = 0;
        }
        rec(0, &mut x, l, ll, u, uu, cap, f, f0, &mut mn, &mut mx);
        match (mn, mx) {
            (Some(lo), Some(hi)) => Some((lo, hi)),
            _ => None,
        }
    }

    fn zero_matrix(n: usize) -> Matrix {
        Matrix::new(n, n)
    }

    #[test]
    fn extreme_nonrectangular_rectangular_matches_extreme() {
        // With l = u = 0 the region is a box: the recursion must reproduce
        // extreme() exactly.
        let l = zero_matrix(2);
        let u = zero_matrix(2);
        let ll = [1i64, -2];
        let uu = [4, 5];
        for f0 in -3i64..=3 {
            for fa in -3i64..=3 {
                for fb in -3i64..=3 {
                    let got = extreme_nonrectangular(&[fa, fb], f0, &l, &ll, &u, &uu);
                    let want = extreme(&[fa, fb], f0, &ll, &uu);
                    assert_eq!(
                        (got.lower, got.upper, got.exact),
                        (want.lower, want.upper, want.exact),
                        "f=[{fa},{fb}] f0={f0}"
                    );
                }
            }
        }
    }

    #[test]
    fn extreme_nonrectangular_triangle_matches_enumeration() {
        // for (i = 0..N) for (j = lo0 ..= i + dhi):  A[i] - A[j].
        // Region: x_0 in [0, N-1], x_1 in [lo0, x_0 + dhi] -- u[1][0] = 1.
        for n in 2..=6i64 {
            for lo0 in 0..=2i64 {
                for dhi in 0..=3i64 {
                    let mut l = zero_matrix(2);
                    let mut u = zero_matrix(2);
                    u.set(1, 0, 1);
                    let ll = [0i64, lo0];
                    let uu = [n - 1, dhi];
                    for fa in -2i64..=2 {
                        for fb in -2i64..=2 {
                            for f0 in -1i64..=1 {
                                let got = extreme_nonrectangular(&[fa, fb], f0, &l, &ll, &u, &uu);
                                let want = match brute_nonrectangular(
                                    &[fa, fb],
                                    f0,
                                    &l,
                                    &ll,
                                    &u,
                                    &uu,
                                    64,
                                ) {
                                    Some(w) => w,
                                    // x_1 in [lo0, x_0 + dhi] is empty when
                                    // lo0 > (n-1) + dhi; nothing to compare.
                                    None => {
                                        assert!(
                                            got.is_empty(),
                                            "empty region but got {got:?} (n={n} lo0={lo0} dhi={dhi})"
                                        );
                                        continue;
                                    }
                                };
                                assert!(
                                    got.lower <= want.0 && want.1 <= got.upper,
                                    "unsound: f=[{fa},{fb}] f0={f0} n={n}: got {got:?} want {want:?}"
                                );
                                assert_eq!(
                                    (got.lower, got.upper),
                                    want,
                                    "not exact: f=[{fa},{fb}] f0={f0} n={n} lo0={lo0} dhi={dhi}"
                                );
                                assert!(got.exact);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn extreme_nonrectangular_fuzz_random() {
        // Random small strictly-lower-triangular regions. The recursion is a
        // sound over-approximation on every region; on the monotone ones the
        // bound is attained, which the triangle test above pins down exactly.
        let mut st = 0xdead_beefu64;
        let mut tested = 0usize;
        for _ in 0..800 {
            let m = rand_in(&mut st, 1, 3) as usize;
            let mut l = zero_matrix(m);
            let mut u = zero_matrix(m);
            for i in 1..m {
                for j in 0..i {
                    l.set(i, j, rand_in(&mut st, 0, 2));
                    u.set(i, j, rand_in(&mut st, 0, 2));
                }
            }
            let ll: Vec<i64> = (0..m).map(|_| rand_in(&mut st, 0, 2)).collect();
            let uu: Vec<i64> = (0..m).map(|_| rand_in(&mut st, 0, 3)).collect();
            let f: Vec<i64> = (0..m).map(|_| rand_in(&mut st, -3, 3)).collect();
            let f0 = rand_in(&mut st, -2, 2);
            let got = extreme_nonrectangular(&f, f0, &l, &ll, &u, &uu);
            if let Some(want) = brute_nonrectangular(&f, f0, &l, &ll, &u, &uu, 14) {
                tested += 1;
                assert!(
                    got.lower <= want.0 && want.1 <= got.upper,
                    "unsound: f={f:?} f0={f0} ll={ll:?} uu={uu:?}: got {got:?} want {want:?}"
                );
                assert!(got.exact, "unexpected clamp in small fuzz case");
            }
        }
        assert!(tested > 200, "only {tested} nonempty regions generated");
    }

    #[test]
    fn empty_region_gives_empty_bounds() {
        assert!(extreme_1(2, 5, 4).is_empty());
        assert!(extreme(&[1, 1], 0, &[3, 0], &[1, 5]).is_empty());
        let l = zero_matrix(2);
        let u = zero_matrix(2);
        assert!(extreme_nonrectangular(&[1, 1], 0, &l, &[5, 0], &u, &[1, 5]).is_empty());
    }

    #[test]
    fn single_iteration_carried_level_refutes() {
        // A level with one iteration admits no strict ordering, so both arms
        // of the test are empty and the dependence is refuted.
        let span = bounds_span(&[1], &[1], &[0], &[0], 1);
        assert!(span.is_empty());
        assert!(!bounds_test_holds(&[1], 0, &[1], 0, &[0], &[0], 1));
    }

    #[test]
    fn saturating_no_panic_on_extreme_inputs() {
        let (big, small) = (i64::MAX, i64::MIN);
        let b = extreme_1(big, small, big);
        assert!(b.lower <= b.upper);
        let _ = extreme(&[big, small], big, &[small, small], &[big, big]);
        let _ = mu(big, big, small, big, 1);
        let _ = nu(small, big, small, big, 0);
        let _ = bounds_span(&[big, small], &[big, big], &[small, small], &[big, big], 1);
        let _ = bounds_span(
            &[big, big, big],
            &[small, small, small],
            &[small; 3],
            &[big; 3],
            2,
        );
        let _ = bounds_test_holds(&[big], small, &[big], big, &[small], &[big], 1);
        let l = zero_matrix(3);
        let mut u = zero_matrix(3);
        u.set(1, 0, big);
        u.set(2, 1, big);
        let _ = extreme_nonrectangular(&[big, small, big], 0, &l, &[small; 3], &u, &[big; 3]);
    }

    #[test]
    fn region_is_triangular_rejects_nontriangular() {
        let good = zero_matrix(2);
        assert!(region_is_triangular(&good, &good, 2));
        let mut bad = zero_matrix(2);
        bad.set(0, 1, 1); // level 0's bound mentions x_1: not triangular
        assert!(!region_is_triangular(&bad, &good, 2));
        assert!(!region_is_triangular(&good, &bad, 2));
    }

    #[test]
    fn mu_nu_pairing_and_monotonicity() {
        // mu <= nu always, and widening the box cannot shrink the interval.
        let mut st = 7u64;
        for _ in 0..500 {
            let a = rand_in(&mut st, -8, 8);
            let b = rand_in(&mut st, -8, 8);
            let p = rand_in(&mut st, -4, 0);
            let q = p + rand_in(&mut st, 0, 5);
            for eps in 0i64..=1 {
                // mu/nu are only meaningful over a nonempty triangle.
                if q - p < eps {
                    continue;
                }
                let (lo, hi) = (mu(a, b, p, q, eps), nu(a, b, p, q, eps));
                assert!(lo <= hi, "mu > nu for {a},{b},{p},{q},{eps}");
            }
            // widen the upper bound by one: min cannot rise, max cannot fall
            if q > p {
                assert!(mu(a, b, p, q - 1, 0) >= mu(a, b, p, q, 0));
                assert!(nu(a, b, p, q - 1, 0) <= nu(a, b, p, q, 0));
            }
        }
    }
}
