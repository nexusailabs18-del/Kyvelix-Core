//! Extended Euclidean algorithm and the gcd test (Wolfe §4.2.1).
//!
//! This is the most-used primitive in the codebase: every level that
//! reasons about a Diophantine equation starts here.
//!
//! ## Input range
//!
//! The public API is `i64`, matching the rest of the IR. Internally the
//! extended algorithm runs on `i128` because the Bezout coefficient `x`
//! can reach `|b|/g` and the intermediate products in
//! `general_solution_2` (`mult * x0`) can exceed `i64`. Every function
//! that can overflow returns `None` (for the `Option`-returning forms) or
//! the conservative "not solvable" answer rather than wrapping.
//! `i64::MIN` is refused outright because `|i64::MIN|` is not
//! representable and every sign manipulation on it is undefined in
//! two's complement.

use std::cmp::Ordering;

/// The domain of the `try_*` entry points.
///
/// `i64::MIN` has no representable absolute value, so every sign
/// manipulation on it is undefined (it panics in debug builds via
/// `abs()`/`checked_abs()` and silently wraps in release builds). The
/// strict API therefore refuses it. The spec-named total API
/// (`gcd`/`findgcd`/`gcd_many`) clamps it to `i64::MIN + 1` so that it can
/// never panic; that clamp is documented at each definition.
#[inline]
fn negatable(v: i64) -> bool {
    v != i64::MIN
}

/// Clamp `i64::MIN` to `i64::MIN + 1` (total API only).
#[inline]
fn clamp_min(v: i64) -> i64 {
    if v == i64::MIN {
        i64::MIN + 1
    } else {
        v
    }
}

/// `gcd(a, b)` — Euclid's algorithm (Wolfe §4.2.1). Always non-negative.
///
/// `gcd(a, 0) == |a|`, `gcd(0, 0) == 0`. Total: never panics and never
/// wraps. `i64::MIN` is clamped to `i64::MIN + 1` (see [`negatable`]);
/// for the entire non-`MIN` domain the result is exact.
pub fn gcd(a: i64, b: i64) -> i64 {
    let mut a = clamp_min(a).unsigned_abs();
    let mut b = clamp_min(b).unsigned_abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    // `a <= 2^63` because gcd divides both inputs; the only overflow is
    // `2^63` itself, which saturates to `i64::MAX`.
    if a > i64::MAX as u64 {
        i64::MAX
    } else {
        a as i64
    }
}

/// `gcd` over a slice. The gcd of an empty slice is `0`.
pub fn gcd_many(coeffs: &[i64]) -> i64 {
    let mut g: i64 = 0;
    for &c in coeffs {
        g = gcd(g, c);
    }
    g
}

/// Extended gcd: returns `(g, x, y)` with `a·x + b·y = g`.
///
/// Total (never panics). `i64::MIN` is clamped to `i64::MIN + 1`; for
/// every other input the identity `a·x + b·y == g` holds exactly and
/// `g == gcd(a, b) >= 0`.
///
/// The strict variant used inside the math engine is [`try_findgcd`].
pub fn findgcd(a: i64, b: i64) -> (i64, i64, i64) {
    match try_findgcd(a, b) {
        Some(t) => t,
        // Only reachable for i64::MIN inputs: recompute on the clamped
        // domain so the identity holds with respect to the clamp.
        None => try_findgcd(clamp_min(a), clamp_min(b)).unwrap_or((gcd(a, b), 1, 0)),
    }
}

/// Strict extended gcd. `None` if an argument is `i64::MIN` or a Bezout
/// coefficient would not fit in `i64`. Guarantees `g == gcd(a, b) >= 0`
/// and `a·x + b·y == g`, and `|x| <= max(|b|/g, 1)`,
/// `|y| <= max(|a|/g, 1)`.
pub fn try_findgcd(a: i64, b: i64) -> Option<(i64, i64, i64)> {
    if !negatable(a) || !negatable(b) {
        return None;
    }
    // Iterative Euclid tracking the Bezout pair on i128.
    let (aa, bb) = (a as i128, b as i128);
    let (mut old_r, mut r) = (aa, bb);
    let (mut old_s, mut s) = (1i128, 0i128); // coefficient of a
    let (mut old_t, mut t) = (0i128, 1i128); // coefficient of b

    while r != 0 {
        let q = old_r / r;
        let next_r = old_r - q * r;
        old_r = r;
        r = next_r;
        let next_s = old_s - q * s;
        old_s = s;
        s = next_s;
        let next_t = old_t - q * t;
        old_t = t;
        t = next_t;
    }

    // Normalise sign: g >= 0.
    let (mut g, mut x, mut y) = (old_r, old_s, old_t);
    if g < 0 {
        g = -g;
        x = -x;
        y = -y;
    }

    if g > i64::MAX as i128
        || x < i64::MIN as i128
        || x > i64::MAX as i128
        || y < i64::MIN as i128
        || y > i64::MAX as i128
    {
        return None;
    }
    Some((g as i64, x as i64, y as i64))
}

/// Single-equation gcd test: is `a·x + b·y = c` solvable in integers?
/// True iff `gcd(a, b) | c`.
pub fn gcd_test_2(a: i64, b: i64, c: i64) -> bool {
    let g = gcd(a, b);
    if g == 0 {
        c == 0
    } else {
        c % g == 0
    }
}

/// Multi-variable gcd test: is `Σ coeffs[i]·x[i] = c` solvable?
/// True iff `gcd(coeffs) | c`.
pub fn gcd_test_many(coeffs: &[i64], c: i64) -> bool {
    let g = gcd_many(coeffs);
    if g == 0 {
        c == 0
    } else {
        c % g == 0
    }
}

/// General solution of `a·x + b·y = c`.
///
/// Returns `Some((x0, y0, dx, dy))` such that the full integer solution
/// set is `{ (x0 + dx·t, y0 + dy·t) : t ∈ Z }`, or `None` when
/// `gcd(a, b) ∤ c` (or on overflow).
///
/// When `a == b == 0` the equation is `0 = c`: `Some((0,0,0,0))` if
/// `c == 0`, else `None`.
pub fn general_solution_2(a: i64, b: i64, c: i64) -> Option<(i64, i64, i64, i64)> {
    if !negatable(a) || !negatable(b) {
        return None;
    }
    let (g, x0, y0) = try_findgcd(a, b)?;
    if g == 0 {
        return if c == 0 { Some((0, 0, 0, 0)) } else { None };
    }
    if c % g != 0 {
        return None;
    }
    let mult = (c / g) as i128;
    let px = mult * (x0 as i128);
    let py = mult * (y0 as i128);
    let dx = b / g;
    let dy = -(a / g);
    if px < i64::MIN as i128
        || px > i64::MAX as i128
        || py < i64::MIN as i128
        || py > i64::MAX as i128
    {
        return None;
    }
    Some((px as i64, py as i64, dx, dy))
}

/// `sign(α) ∈ {-1, 0, 1}` (Wolfe §4.1.1).
#[inline]
pub fn sign(alpha: i64) -> i32 {
    match alpha.cmp(&0) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// The sign vector `sign(a - b)` componentwise (Wolfe §4.1.1), with
/// saturating subtraction so `i64::MAX - i64::MIN` does not wrap.
pub fn sign_vector(a: &[i64], b: &[i64]) -> Vec<i32> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| sign(x.saturating_sub(y)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic xorshift generator, matching the codebase convention
    // used by the decide-level fuzzers.
    fn xs(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn rand_i64(state: &mut u64, mag: i64) -> i64 {
        // Uniform-ish in [-mag, mag], with occasional extremes.
        // i64::MIN is deliberately excluded: it is out of the strict domain.
        let r = xs(state);
        match r % 16 {
            0 => i64::MAX,
            1 => i64::MIN + 1,
            2 => 0,
            _ => ((r >> 16) as i64 % (2 * mag + 1)) - mag,
        }
    }

    #[test]
    fn gcd_basics() {
        assert_eq!(gcd(0, 0), 0);
        assert_eq!(gcd(0, 7), 7);
        assert_eq!(gcd(7, 0), 7);
        assert_eq!(gcd(-7, 0), 7);
        assert_eq!(gcd(12, 18), 6);
        assert_eq!(gcd(-12, 18), 6);
        assert_eq!(gcd(12, -18), 6);
        assert_eq!(gcd(-12, -18), 6);
        assert_eq!(gcd(17, 5), 1);
        // Total API must not panic on the one un-negatable value.
        assert_eq!(gcd(i64::MIN, 1), 1);
        assert_eq!(gcd(i64::MIN, i64::MIN), i64::MAX);
    }

    #[test]
    fn gcd_many_basics() {
        assert_eq!(gcd_many(&[]), 0);
        assert_eq!(gcd_many(&[0, 0, 0]), 0);
        assert_eq!(gcd_many(&[12, 18, 30]), 6);
        assert_eq!(gcd_many(&[0, 5, 10]), 5);
        assert_eq!(gcd_many(&[-12, 18, -30]), 6);
    }

    /// The defining identity of `try_findgcd` plus the coefficient bound.
    #[test]
    fn fuzz_findgcd_identity() {
        let mut st = 0x1234_5678_9abc_def0u64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..20000 {
            let a = rand_i64(&mut st, 100_000);
            let b = rand_i64(&mut st, 100_000);
            let (g, x, y) = match try_findgcd(a, b) {
                Some(v) => v,
                None => continue,
            };
            tested += 1;
            if g != gcd(a, b) || g < 0 {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("gcd({},{}) = {} not {}", a, b, g, gcd(a, b)));
                }
            }
            let lhs = a as i128 * x as i128 + b as i128 * y as i128;
            if lhs != g as i128 {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!("{}*{} + {}*{} = {} != {}", a, x, b, y, lhs, g));
                }
            }
            let bx = if b == 0 {
                1
            } else {
                (b as i128).abs() / g as i128
            };
            let by = if a == 0 {
                1
            } else {
                (a as i128).abs() / g as i128
            };
            if (x as i128).abs() > bx.max(1) || (y as i128).abs() > by.max(1) {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "({},{}) -> ({},{},{}) exceeds bounds",
                        a, b, g, x, y
                    ));
                }
            }
        }
        println!("findgcd fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "findgcd identity violated {} times", bad);
        assert!(tested > 15000, "too few in-range cases: {}", tested);
    }

    /// `general_solution_2` must produce a particular solution and a
    /// direction that spans exactly the solution set.
    #[test]
    fn fuzz_general_solution_2() {
        let mut st = 0xdead_beef_cafe_babeu64;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();

        for _ in 0..20000 {
            let a = rand_i64(&mut st, 40);
            let b = rand_i64(&mut st, 40);
            let c = rand_i64(&mut st, 200);
            let solvable = gcd_test_2(a, b, c);
            let got = general_solution_2(a, b, c);

            if !solvable {
                if got.is_some() {
                    bad += 1;
                    if ex.len() < 5 {
                        ex.push(format!(
                            "({},{},{}) unsolvable but reported {:?}",
                            a, b, c, got
                        ));
                    }
                }
                continue;
            }
            let (x0, y0, dx, dy) = match got {
                Some(v) => v,
                None => continue, // Bezout coefficient overflow
            };
            tested += 1;

            for t in -10i64..=10 {
                let x = x0 as i128 + dx as i128 * t as i128;
                let y = y0 as i128 + dy as i128 * t as i128;
                let lhs = a as i128 * x + b as i128 * y;
                if lhs != c as i128 {
                    bad += 1;
                    if ex.len() < 5 {
                        ex.push(format!("({},{},{}) t={} gives {}", a, b, c, t, lhs));
                    }
                    break;
                }
            }

            // a*dx + b*dy == 0: the step stays on the solution set.
            let step = a as i128 * dx as i128 + b as i128 * dy as i128;
            if step != 0 {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "({},{},{}) dir ({},{}) step {}",
                        a, b, c, dx, dy, step
                    ));
                }
            }
        }
        println!("general_solution_2 fuzz: tested={} bad={}", tested, bad);
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(bad, 0, "general_solution_2 violated {} times", bad);
    }

    /// `gcd_test_2` agrees with brute force over a small box.
    #[test]
    fn fuzz_gcd_test_2_brute_force() {
        let mut st = 0x0f0f_0f0f_1234_5678u64;
        let mut bad = 0u64;
        let mut ex: Vec<String> = Vec::new();
        for _ in 0..3000 {
            let a = (xs(&mut st) % 41) as i64 - 20;
            let b = (xs(&mut st) % 41) as i64 - 20;
            let c = (xs(&mut st) % 81) as i64 - 40;

            let mut found = false;
            'outer: for x in -80i64..=80 {
                for y in -80i64..=80 {
                    if a * x + b * y == c {
                        found = true;
                        break 'outer;
                    }
                }
            }
            if gcd_test_2(a, b, c) != found {
                bad += 1;
                if ex.len() < 5 {
                    ex.push(format!(
                        "gcd_test_2({},{},{}) = {} but brute force {}",
                        a,
                        b,
                        c,
                        gcd_test_2(a, b, c),
                        found
                    ));
                }
            }
        }
        for e in &ex {
            println!("  BAD {}", e);
        }
        assert_eq!(
            bad, 0,
            "gcd_test_2 disagrees with brute force {} times",
            bad
        );
    }

    /// `gcd_many` / `gcd_test_many` consistency.
    #[test]
    fn fuzz_gcd_many_consistency() {
        let mut st = 0xabcd_ef01_2345_6789u64;
        let mut bad = 0u64;
        for _ in 0..5000 {
            let n = 1 + (xs(&mut st) % 4) as usize;
            let coeffs: Vec<i64> = (0..n).map(|_| (xs(&mut st) % 41) as i64 - 20).collect();
            let c = (xs(&mut st) % 121) as i64 - 60;
            let g = gcd_many(&coeffs);
            let expect = if g == 0 { c == 0 } else { c % g == 0 };
            if gcd_test_many(&coeffs, c) != expect {
                bad += 1;
            }
        }
        assert_eq!(bad, 0);
    }

    /// `general_solution_2` enumerates the complete solution set within a
    /// box: every box solution must be on the reported family.
    #[test]
    fn fuzz_general_solution_2_is_complete() {
        let mut st = 0x5555_aaaa_1234_9999u64;
        let mut checked = 0u64;
        let mut bad = 0u64;
        for _ in 0..2000 {
            let a = (xs(&mut st) % 21) as i64 - 10;
            let b = (xs(&mut st) % 21) as i64 - 10;
            let c = (xs(&mut st) % 41) as i64 - 20;
            if !gcd_test_2(a, b, c) {
                continue;
            }
            let (x0, y0, dx, dy) = match general_solution_2(a, b, c) {
                Some(v) => v,
                None => continue,
            };
            checked += 1;
            for x in -30i64..=30 {
                for y in -30i64..=30 {
                    if a * x + b * y != c {
                        continue;
                    }
                    // Degenerate equation 0 = 0: every point in the box is
                    // a solution, while the family collapses to (0, 0).
                    let on_family = if a == 0 && b == 0 {
                        true
                    } else if dx != 0 {
                        let t = x - x0;
                        t % dx == 0 && y == y0 + dy * (t / dx)
                    } else if dy != 0 {
                        x == x0 && (y - y0) % dy == 0
                    } else {
                        x == x0 && y == y0
                    };
                    if !on_family {
                        bad += 1;
                    }
                }
            }
        }
        assert!(checked > 500, "too few solvable cases: {}", checked);
        assert_eq!(bad, 0, "{} box solutions missing from the family", bad);
    }

    #[test]
    fn sign_vector_matches_definition() {
        assert_eq!(sign_vector(&[3, 3, 3], &[4, 3, 0]), vec![-1, 0, 1]);
        assert_eq!(sign_vector(&[i64::MAX], &[i64::MIN]), vec![1]);
        assert_eq!(sign(-5), -1);
        assert_eq!(sign(0), 0);
        assert_eq!(sign(5), 1);
    }
}
