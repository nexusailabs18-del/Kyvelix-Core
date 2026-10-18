//! Trip-count computation (Wolfe §6.5.7, Phase 4).
//!
//! Given an induction variable `x` that starts at `init`, advances by `step`
//! per iteration, and is compared against a bound that starts at `bound` and
//! advances by `bound_step` per iteration, `trip_count` returns the number of
//! iterations the loop body executes and the exit value of `x`.
//!
//! The loop continues at iteration `t` exactly when
//!
//! ```text
//! init + step·t  <  bound + bound_step·t
//! ⟺  (init - bound)  <  (bound_step - step)·t
//! ⟺  d·t > -n          with  n = bound - init,  d = bound_step - step
//! ```
//!
//! so the trip count is the smallest `t ≥ 0` with `d·t ≥ n`. The four cases
//! of the specification follow:
//!
//! | Condition | Trip count |
//! |---|---|
//! | `step > bound_step` (`d < 0`… see note) | `max(0, ⌈n/d⌉)` |
//! | `step == bound_step` (`d == 0`) and `n > 0` | `+∞` (uncountable) |
//! | `step == bound_step` and `n ≤ 0` | `0` |
//! | `step < bound_step` | `0` when `n ≤ 0`, else `+∞` |
//!
//! (All arithmetic is done in `i128`; a result outside `i64` is reported as
//! uncountable rather than wrapping.)

/// The trip count of a countable loop, or `None` when it never exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TripCount {
    /// Number of iterations; `None` = uncountable (a non-terminating loop).
    pub count: Option<i64>,
    /// The value of the IV on exit (`init + step·count`); `None` if
    /// uncountable.
    pub exit_value: Option<i64>,
}

/// Compute the trip count of `x = init; while (x < bound) { …; x += step;
/// bound += bound_step; }`.
pub fn trip_count(init: i64, step: i64, bound: i64, bound_step: i64) -> TripCount {
    let n = bound as i128 - init as i128;
    let d = step as i128 - bound_step as i128;

    let count: i128 = if d > 0 {
        // t >= ceil(n / d), clamped at 0.
        if n <= 0 {
            0
        } else {
            (n + d - 1) / d
        }
    } else if d == 0 {
        if n > 0 {
            return TripCount { count: None, exit_value: None };
        }
        0
    } else {
        // d < 0. The gap `n - d·t` grows with `t`, so the loop exits only if
        // it never entered: `n <= 0`.
        if n <= 0 {
            0
        } else {
            return TripCount { count: None, exit_value: None };
        }
    };

    let count_i64 = match i64::try_from(count) {
        Ok(c) => c,
        Err(_) => return TripCount { count: None, exit_value: None },
    };
    let exit = init as i128 + step as i128 * count;
    let exit_value = i64::try_from(exit).ok();
    TripCount {
        count: Some(count_i64),
        exit_value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_ascending_loop() {
        // for (i = 0; i < 10; i++) → 10 iterations, exit 10.
        assert_eq!(
            trip_count(0, 1, 10, 0),
            TripCount { count: Some(10), exit_value: Some(10) }
        );
        // for (i = 3; i < 8; i += 2) → 3 iterations (3,5,7), exit 9.
        assert_eq!(
            trip_count(3, 2, 8, 0),
            TripCount { count: Some(3), exit_value: Some(9) }
        );
    }

    #[test]
    fn empty_loop() {
        assert_eq!(
            trip_count(5, 1, 5, 0),
            TripCount { count: Some(0), exit_value: Some(5) }
        );
        assert_eq!(
            trip_count(9, 1, 5, 0),
            TripCount { count: Some(0), exit_value: Some(9) }
        );
    }

    #[test]
    fn equal_rates() {
        // Never closes the gap from below → uncountable.
        assert_eq!(trip_count(0, 1, 10, 1).count, None);
        // Already past → zero.
        assert_eq!(trip_count(5, 1, 5, 1).count, Some(0));
    }

    #[test]
    fn bound_advances_slower() {
        // i: 0, 2, ... and bound: 1, 2, ... The guard holds only for the first
        // test (0 < 1), so the body runs once and i exits at 2.
        let tc = trip_count(0, 2, 1, 1);
        assert_eq!(tc.count, Some(1));
        assert_eq!(tc.exit_value, Some(2));
        // Equal starting values never enter the loop at all.
        assert_eq!(trip_count(0, 2, 0, 1).count, Some(0));
    }

    #[test]
    fn trip_count_rejects_a_widening_guard() {
        // The closed form answers only for a guard that linear arithmetic can
        // settle. `step <= bound_step` with the gap still open (`n > 0`) means
        // the bound never gets overtaken: the loop is either infinite or its
        // exit depends on a guard shape this formula does not model, so the
        // honest answer is "uncountable" — `None`, never a guessed number.
        assert_eq!(trip_count(0, 1, 10, 1).count, None); // equal rates, gap open
        assert_eq!(trip_count(0, 1, 10, 2).count, None); // bound outruns the IV
        assert_eq!(trip_count(0, 0, 5, 0).count, None); // IV frozen below the bound
        // A negative net rate means the gap grows; entry is the only chance to
        // have already exited, so a loop that entered cannot be counted either.
        assert_eq!(trip_count(0, 1, 10, 5).count, None);
        // Contrast: the same guard with the gap already closed is countable —
        // zero trips — proving the rejection is about the open gap, not the
        // rates themselves.
        assert_eq!(trip_count(20, 1, 10, 5).count, Some(0));
        // And every result here carries no exit value, because there is no trip
        // count to evaluate `init + step·count` at.
        assert_eq!(trip_count(0, 1, 10, 1), TripCount { count: None, exit_value: None });
    }
}
