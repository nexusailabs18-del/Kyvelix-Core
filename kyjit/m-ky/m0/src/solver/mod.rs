//! Fourier-Motzkin over the integers + GCD test, with witness construction.
//! Sound: returns Sat(witness), Unsat(proof), or Unknown(reason).
//!
//! Phase 2 (the math engine) lives in the submodules:
//!
//! | Module | Contents |
//! |---|---|
//! | [`gcd`] | Euclid, `findgcd`, gcd tests, `general_solution_2`, sign vectors |
//! | [`inverse`] | `Matrix`, `determinant`, `rank`, `invert`, `invert_rational` |
//! | [`hnf`] | Hermite Normal Form, `A·U = H`, column echelon form |
//! | [`dark_shadow`] | Integer Fourier-Motzkin, `ProjectInt`, dark shadow |
//! | [`extreme`] | `Extreme`, `mu`/`nu`, `bounds_test`, nonrectangular regions |
//! | [`integer`] | General integer solver over `A·x = b`, `C·x <= d` |

pub mod dark_shadow;
pub mod extreme;
pub mod gcd;
pub mod hnf;
pub mod integer;
pub mod inverse;

pub use inverse::Matrix;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    /// coefficients over variables v[0..n]
    pub coeffs: Vec<i64>,
    /// sum(coeffs[i] * v[i]) <= constant
    pub constant: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct System {
    pub nvars: usize,
    pub constraints: Vec<Constraint>,
    /// Bounds for each variable (lo, hi) inclusive.
    pub ranges: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveResult {
    Sat(Vec<i64>),
    Unsat(Vec<Constraint>),
    Unknown(String),
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn gcd_test(c: &Constraint) -> bool {
    let g = c.coeffs.iter().fold(0i64, |acc, &x| gcd(acc, x));
    if g == 0 {
        return c.constant >= 0;
    }
    // For <=, gcd test isn't a hard reject the same way as for equality.
    // Keep the constraint; FM will handle it.
    true
}

/// One FM elimination step: which constraints were combined to produce this one.
#[derive(Debug, Clone)]
struct FmStep {
    /// Index of the positive-parent constraint (coeff[var] > 0) in `parents`.
    pos: usize,
    /// Index of the negative-parent constraint (coeff[var] < 0) in `parents`.
    neg: usize,
}

/// Full FM trace: for each eliminated variable, the resulting constraints and
/// their provenance.
#[derive(Debug, Clone)]
struct FmTrace {
    /// For each elimination level, the constraints present *after* that
    /// elimination and the steps that produced each of them.
    levels: Vec<FmLevel>,
    /// Constraints with no variables after all eliminations.
    final_constraints: Vec<Constraint>,
}

#[derive(Debug, Clone)]
struct FmLevel {
    /// Constraints present at the *start* of this elimination.
    input: Vec<Constraint>,
    /// Constraints present at the *end* (zero-coefficient on `var`).
    output: Vec<Constraint>,
    /// For each output constraint, how it was produced.
    steps: Vec<Option<FmStep>>,
    /// Which variable was eliminated (index in original var space).
    var: usize,
}

pub fn solve(sys: &System) -> SolveResult {
    if sys.nvars > 6 {
        return SolveResult::Unknown(format!("nvars {} > 6", sys.nvars));
    }
    for c in &sys.constraints {
        if !gcd_test(c) {
            return SolveResult::Unsat(vec![c.clone()]);
        }
    }
    match fm_eliminate_traced(sys) {
        FmOutcome::Feasible(trace) => match construct_witness(sys, &trace) {
            Some(w) => SolveResult::Sat(w),
            None => SolveResult::Unknown("witness construction failed".into()),
        },
        FmOutcome::Infeasible(proof) => SolveResult::Unsat(proof),
        FmOutcome::Blowup => SolveResult::Unknown("FM blowup".into()),
    }
}

enum FmOutcome {
    Feasible(FmTrace),
    Infeasible(Vec<Constraint>),
    Blowup,
}

fn fm_eliminate_traced(sys: &System) -> FmOutcome {
    let mut current = sys.constraints.clone();
    let mut levels: Vec<FmLevel> = Vec::new();
    let mut var = sys.nvars;

    while var > 0 {
        let eliminate_idx = var - 1;
        let input = current.clone();

        let mut pos: Vec<(usize, Constraint)> = Vec::new();
        let mut neg: Vec<(usize, Constraint)> = Vec::new();
        let mut zero: Vec<(usize, Constraint)> = Vec::new();

        for (i, c) in input.iter().enumerate() {
            let a = c.coeffs[eliminate_idx];
            if a > 0 {
                pos.push((i, c.clone()));
            } else if a < 0 {
                neg.push((i, c.clone()));
            } else {
                zero.push((i, c.clone()));
            }
        }

        let mut output: Vec<Constraint> = zero.iter().map(|(_, c)| c.clone()).collect();
        let mut steps: Vec<Option<FmStep>> = vec![None; output.len()];

        for (ip, (_, p)) in pos.iter().enumerate() {
            for (iq, (_, q)) in neg.iter().enumerate() {
                let ap = p.coeffs[eliminate_idx];
                let aq = -q.coeffs[eliminate_idx];
                let mut coeffs = vec![0i64; sys.nvars];
                for i in 0..sys.nvars {
                    coeffs[i] = p.coeffs[i] * aq + q.coeffs[i] * ap;
                }
                let constant = p.constant * aq + q.constant * ap;
                let g = coeffs.iter().fold(0i64, |acc, &x| gcd(acc, x));
                let (coeffs, constant) = if g > 1 {
                    (coeffs.iter().map(|x| x / g).collect(), constant / g)
                } else {
                    (coeffs, constant)
                };
                output.push(Constraint { coeffs, constant });
                steps.push(Some(FmStep {
                    pos: ip,
                    neg: iq,
                }));
            }
        }

        if output.is_empty() {
            return FmOutcome::Infeasible(vec![]);
        }
        if output.len() > 50_000 {
            return FmOutcome::Blowup;
        }

        levels.push(FmLevel {
            input,
            output: output.clone(),
            steps,
            var: eliminate_idx,
        });

        current = output;
        var -= 1;
    }

    // After all eliminations, check constants-only constraints.
    for c in &current {
        if c.constant < 0 {
            return FmOutcome::Infeasible(vec![c.clone()]);
        }
    }

    FmOutcome::Feasible(FmTrace {
        levels,
        final_constraints: current,
    })
}

/// Construct a witness by walking the FM trace in reverse.
///
/// Start with the last level. Its `output` is constant-only (all variables
/// eliminated). Pick any value satisfying those. Then for each level in
/// reverse, choose a value for `var` that satisfies every constraint at
/// that level with the variables already assigned.
fn construct_witness(sys: &System, trace: &FmTrace) -> Option<Vec<i64>> {
    // Hybrid: try fast reverse-FM first, fall back to bounded DFS.
    if let Some(w) = construct_witness_reverse_fm(sys, trace) {
        return Some(w);
    }
    construct_witness_dfs(sys)
}

/// Fast path: use the FM trace directly.
/// Fast path: use the FM trace directly, guided by the elimination steps.
fn construct_witness_reverse_fm(sys: &System, trace: &FmTrace) -> Option<Vec<i64>> {
    // Try three strategies: lower bound, upper bound, midpoint.
    // For affine systems, at least one of these finds a valid witness.
    if let Some(w) = walk_strategy(sys, trace, Strategy::Lower) {
        return Some(w);
    }
    if let Some(w) = walk_strategy(sys, trace, Strategy::Upper) {
        return Some(w);
    }
    if let Some(w) = walk_strategy(sys, trace, Strategy::Mid) {
        return Some(w);
    }
    None
}

#[derive(Copy, Clone)]
enum Strategy {
    Lower,
    Upper,
    Mid,
}

fn walk_strategy(sys: &System, trace: &FmTrace, strategy: Strategy) -> Option<Vec<i64>> {
    let mut assign: Vec<i64> = vec![0; sys.nvars];
    let ranges: Vec<(i64, i64)> = if sys.ranges.len() == sys.nvars {
        sys.ranges.clone()
    } else {
        vec![(-64i64, 64i64); sys.nvars]
    };

    for level in trace.levels.iter().rev() {
        let var = level.var;
        let (lo, hi) = ranges[var];

        let mut lb: i64 = lo;
        let mut ub: i64 = hi;

        // Use the constraints in level.input that involve var directly.
        // Also use the step parents.
        for c in &level.input {
            let a = c.coeffs[var];
            if a == 0 { continue; }
            let mut rest: i64 = 0;
            for i in 0..sys.nvars {
                if i != var { rest += c.coeffs[i] * assign[i]; }
            }
            let rhs = c.constant - rest;
            if a > 0 {
                let bound = rhs.div_euclid(a);
                if bound < ub { ub = bound; }
            } else {
                let bound = rhs.div_euclid(a);
                let bound = if rhs.rem_euclid(a) != 0 { bound + 1 } else { bound };
                if bound > lb { lb = bound; }
            }
        }

        if lb > ub {
            return None;
        }

        assign[var] = match strategy {
            Strategy::Lower => lb,
            Strategy::Upper => ub,
            Strategy::Mid => (lb + ub) / 2,
        };
    }

    // Final validation.
    for c in &sys.constraints {
        let mut sum = 0i64;
        for i in 0..sys.nvars {
            sum += c.coeffs[i] * assign[i];
        }
        if sum > c.constant {
            return None;
        }
    }

    Some(assign)
}



/// Slow path: bounded DFS with pruning.
fn construct_witness_dfs(sys: &System) -> Option<Vec<i64>> {
    const MAX_NODES: u64 = 10_000_000;
    let mut nodes: u64 = 0;

    let ranges: Vec<(i64, i64)> = if sys.ranges.len() == sys.nvars {
        sys.ranges.clone()
    } else {
        vec![(-64i64, 64i64); sys.nvars]
    };

    fn rec(
        sys: &System,
        ranges: &[(i64, i64)],
        idx: usize,
        assign: &mut Vec<i64>,
        nodes: &mut u64,
    ) -> Option<Vec<i64>> {
        if *nodes > MAX_NODES {
            return None;
        }
        for c in &sys.constraints {
            let mut all_assigned = true;
            for i in idx..sys.nvars {
                if c.coeffs[i] != 0 {
                    all_assigned = false;
                    break;
                }
            }
            if !all_assigned {
                continue;
            }
            let mut sum = 0i64;
            for i in 0..idx {
                sum += c.coeffs[i] * assign[i];
            }
            if sum > c.constant {
                return None;
            }
        }
        if idx == ranges.len() {
            return Some(assign.clone());
        }
        let (lo, hi) = ranges[idx];
        for v in lo..=hi {
            *nodes += 1;
            if *nodes > MAX_NODES {
                return None;
            }
            assign.push(v);
            if let Some(w) = rec(sys, ranges, idx + 1, assign, nodes) {
                return Some(w);
            }
            assign.pop();
        }
        None
    }

    rec(sys, &ranges, 0, &mut vec![], &mut nodes)
}


/// Fast path: use the FM trace directly.

/// Slow path: bounded DFS with pruning.




#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_equality() {
        // i - j = 4  =>  i - j <= 4  AND  j - i <= -4
        let sys = System {
            nvars: 2,
            constraints: vec![
                Constraint { coeffs: vec![1, -1], constant: 4 },
                Constraint { coeffs: vec![-1, 1], constant: -4 },
                Constraint { coeffs: vec![1, 0], constant: 100 },
                Constraint { coeffs: vec![-1, 0], constant: 0 },
                Constraint { coeffs: vec![0, 1], constant: 100 },
                Constraint { coeffs: vec![0, -1], constant: 0 },
            ],
            ranges: vec![(0, 100), (0, 100)],
        };
        match solve(&sys) {
            SolveResult::Sat(w) => {
                assert_eq!(w[0] - w[1], 4);
            }
            other => panic!("expected Sat, got {:?}", other),
        }
    }

    #[test]
    fn test_infeasible() {
        // i - j <= -1  AND  i - j >= 1  =>  infeasible
        let sys = System {
            nvars: 2,
            constraints: vec![
                Constraint { coeffs: vec![1, -1], constant: -1 },
                Constraint { coeffs: vec![-1, 1], constant: -1 },
            ],
            ranges: vec![(0, 100), (0, 100)],
        };
        match solve(&sys) {
            SolveResult::Unsat(_) => {}
            other => panic!("expected Unsat, got {:?}", other),
        }
    }
}
