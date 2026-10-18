//! Symbolic constraints for M1 Level 2 (Feautrier parametric FM).
//! Coefficients and constants are ParamExpr instead of i64.

use crate::param_expr::ParamExpr;

/// A linear constraint: sum(coeffs[i] * x[i]) <= constant
/// where coeffs and constant are symbolic (ParamExpr).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolicConstraint {
    pub coeffs: Vec<ParamExpr>,
    pub constant: ParamExpr,
}

impl SymbolicConstraint {
    pub fn from_const(coeffs: Vec<i64>, constant: i64) -> Self {
        SymbolicConstraint {
            coeffs: coeffs.into_iter().map(ParamExpr::Const).collect(),
            constant: ParamExpr::Const(constant),
        }
    }

    pub fn is_const(&self) -> bool {
        self.coeffs.iter().all(|c| c.is_const()) && self.constant.is_const()
    }

    pub fn as_const(&self) -> Option<(Vec<i64>, i64)> {
        let coeffs: Option<Vec<i64>> = self.coeffs.iter().map(|c| c.as_const()).collect();
        let constant = self.constant.as_const()?;
        Some((coeffs?, constant))
    }

    pub fn simplify(&self) -> Self {
        SymbolicConstraint {
            coeffs: self.coeffs.iter().map(|c| c.simplify()).collect(),
            constant: self.constant.simplify(),
        }
    }
}

/// A symbolic system: nvars variables, list of constraints,
/// plus a list of parameters.
#[derive(Debug, Clone)]
pub struct SymbolicSystem {
    pub nvars: usize,
    pub params: Vec<String>,
    pub constraints: Vec<SymbolicConstraint>,
    pub distance_expr: Vec<ParamExpr>, // D2: distance expression in terms of current variables
}

impl SymbolicSystem {
    pub fn as_const_system(&self) -> Option<crate::solver::System> {
        let mut constraints = Vec::with_capacity(self.constraints.len());
        for c in &self.constraints {
            let (coeffs, constant) = c.as_const()?;
            constraints.push(crate::solver::Constraint { coeffs, constant });
        }
        Some(crate::solver::System {
            nvars: self.nvars,
            constraints,
            ranges: vec![(0, 0); self.nvars],
        })
    }
}

#[derive(Debug, Clone)]
pub struct Piece {
    pub domain: String,
    pub distance: Vec<ParamExpr>,
}

#[derive(Debug, Clone)]
pub enum FeautrierResult {
    Const(Vec<i64>),
    Piecewise(Vec<Piece>),
    None,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct CaseSplit {
    pub condition: String,
    pub system: SymbolicSystem,
}

// ============================================================================
// D1: Fix sign classification
// ============================================================================

fn is_positive(e: &ParamExpr) -> Option<bool> {
    e.simplify().as_const().map(|v| v > 0)
}

fn is_negative(e: &ParamExpr) -> Option<bool> {
    e.simplify().as_const().map(|v| v < 0)
}

fn is_zero(e: &ParamExpr) -> Option<bool> {
    e.simplify().as_const().map(|v| v == 0)
}

pub fn eliminate_variable(sys: &SymbolicSystem) -> Vec<CaseSplit> {
    let n = sys.nvars;
    if n == 0 {
        return vec![CaseSplit {
            condition: "true".into(),
            system: sys.clone(),
        }];
    }
    let var = n - 1;

    let mut known_pos: Vec<SymbolicConstraint> = Vec::new();
    let mut known_neg: Vec<SymbolicConstraint> = Vec::new();
    let mut known_zero: Vec<SymbolicConstraint> = Vec::new();
    let mut unknown: Vec<SymbolicConstraint> = Vec::new();

    for c in &sys.constraints {
        let coeff = &c.coeffs[var];
        match (is_positive(coeff), is_negative(coeff), is_zero(coeff)) {
            (Some(true), _, _) => known_pos.push(c.clone()),
            (_, Some(true), _) => known_neg.push(c.clone()),
            (_, _, Some(true)) => known_zero.push(c.clone()),
            _ => unknown.push(c.clone()),
        }
    }

    if unknown.is_empty() {
        let mut new_sys = combine_and_eliminate(sys, &known_pos, &known_neg, &known_zero, var);
        
        // D2: Reconstruct eliminated variable and substitute into distance_expr
        let (lower, upper) = get_var_bounds(sys, var);
        if let (Some(l), Some(u)) = (&lower, &upper) {
            if l == u {
                for expr in &mut new_sys.distance_expr {
                    *expr = substitute(expr, var, l);
                }
            }
        }
        
        return vec![CaseSplit {
            condition: "true".into(),
            system: new_sys,
        }];
    }

    let mut result = Vec::new();
    for c in unknown.iter().take(1) {
        let coeff = &c.coeffs[var];
        let coeff_str = coeff.to_string_repr();

        for (cond, mut list_pos, mut list_neg, mut list_zero) in [
            (format!("{} > 0", coeff_str), known_pos.clone(), known_neg.clone(), known_zero.clone()),
            (format!("{} < 0", coeff_str), known_pos.clone(), known_neg.clone(), known_zero.clone()),
            (format!("{} == 0", coeff_str), known_pos.clone(), known_neg.clone(), known_zero.clone()),
        ] {
            if cond.ends_with("> 0") { list_pos.push(c.clone()); }
            if cond.ends_with("< 0") { list_neg.push(c.clone()); }
            if cond.ends_with("== 0") { list_zero.push(c.clone()); }

            let mut new_sys = combine_and_eliminate(sys, &list_pos, &list_neg, &list_zero, var);
            
            // D2: Substitute if bounds match
            let (lower, upper) = get_var_bounds(sys, var);
            if let (Some(l), Some(u)) = (&lower, &upper) {
                if l == u {
                    for expr in &mut new_sys.distance_expr {
                        *expr = substitute(expr, var, l);
                    }
                }
            }

            result.push(CaseSplit {
                condition: cond,
                system: new_sys,
            });
        }
    }
    result
}

// D2: Helper to find bounds for a variable from constraints
fn get_var_bounds(sys: &SymbolicSystem, var: usize) -> (Option<ParamExpr>, Option<ParamExpr>) {
    let mut lower: Option<ParamExpr> = None;
    let mut upper: Option<ParamExpr> = None;
    
    for c in &sys.constraints {
        let coeff = &c.coeffs[var];
        let cv = match coeff.simplify().as_const() {
            Some(v) => v,
            None => continue,
        };
        if cv == 0 { continue; }
        
        let mut sum_other = ParamExpr::Const(0);
        for (i, term) in c.coeffs.iter().enumerate() {
            if i != var {
                sum_other = sum_other.add(term);
            }
        }
        
        let rhs = c.constant.sub(&sum_other);
        let bound = ParamExpr::Div(Box::new(rhs), cv).simplify();
        
        if cv > 0 {
            upper = Some(bound);
        } else {
            lower = Some(bound);
        }
    }
    (lower, upper)
}

// D2: Helper to substitute a variable with its reconstructed expression
fn substitute(expr: &ParamExpr, var_idx: usize, replacement: &ParamExpr) -> ParamExpr {
    let var_name = format!("x_{}", var_idx);
    substitute_name(expr, &var_name, replacement)
}

fn substitute_name(expr: &ParamExpr, name: &str, replacement: &ParamExpr) -> ParamExpr {
    match expr {
        ParamExpr::Const(v) => ParamExpr::Const(*v),
        ParamExpr::Param(n) => {
            if n == name { replacement.clone() } else { expr.clone() }
        }
        ParamExpr::Mul(c, e) => ParamExpr::Mul(*c, Box::new(substitute_name(e, name, replacement))).simplify(),
        ParamExpr::Div(e, c) => ParamExpr::Div(Box::new(substitute_name(e, name, replacement)), *c).simplify(),
        ParamExpr::Add(a, b) => ParamExpr::Add(Box::new(substitute_name(a, name, replacement)), Box::new(substitute_name(b, name, replacement))).simplify(),
        ParamExpr::Sub(a, b) => ParamExpr::Sub(Box::new(substitute_name(a, name, replacement)), Box::new(substitute_name(b, name, replacement))).simplify(),
    }
}

fn combine_and_eliminate(
    sys: &SymbolicSystem,
    pos: &[SymbolicConstraint],
    neg: &[SymbolicConstraint],
    zero: &[SymbolicConstraint],
    var: usize,
) -> SymbolicSystem {
    let new_nvars = sys.nvars - 1;
    let mut new_constraints: Vec<SymbolicConstraint> = zero
        .iter()
        .map(|c| {
            let mut nc = c.clone();
            nc.coeffs.truncate(new_nvars);
            nc.simplify()
        })
        .collect();

    for p in pos {
        for q in neg {
            let ap = &p.coeffs[var];
            let aq = &q.coeffs[var];

            let mut coeffs = Vec::with_capacity(new_nvars);
            for k in 0..new_nvars {
                let ap_q = mul_expr(ap, &q.coeffs[k]);
                let aq_p = mul_expr(aq, &p.coeffs[k]);
                let diff = ParamExpr::Sub(Box::new(ap_q), Box::new(aq_p)).simplify();
                coeffs.push(diff);
            }

            let ap_c = mul_expr(ap, &q.constant);
            let aq_c = mul_expr(aq, &p.constant);
            let constant = ParamExpr::Sub(Box::new(ap_c), Box::new(aq_c)).simplify();

            new_constraints.push(SymbolicConstraint { coeffs, constant }.simplify());
        }
    }

    SymbolicSystem {
        nvars: new_nvars,
        params: sys.params.clone(),
        constraints: new_constraints,
        distance_expr: sys.distance_expr.clone(), // D2: Carry over distance expression
    }
}

fn mul_expr(a: &ParamExpr, b: &ParamExpr) -> ParamExpr {
    let a_s = a.simplify();
    let b_s = b.simplify();

    if let Some(av) = a_s.as_const() {
        if av == 0 { return ParamExpr::Const(0); }
        if av == 1 { return b_s; }
        return ParamExpr::Mul(av, Box::new(b_s)).simplify();
    }
    if let Some(bv) = b_s.as_const() {
        if bv == 0 { return ParamExpr::Const(0); }
        if bv == 1 { return a_s; }
        return ParamExpr::Mul(bv, Box::new(a_s)).simplify();
    }
    ParamExpr::Const(i64::MIN)
}

pub fn feautrier(sys: &SymbolicSystem) -> FeautrierResult {
    // D1: If any coefficient is genuinely parametric, we cannot eliminate safely.
    for c in &sys.constraints {
        for coeff in &c.coeffs {
            if coeff.simplify().as_const().is_none() {
                return FeautrierResult::Unknown("parametric coefficient".into());
            }
        }
    }

    if sys.constraints.iter().all(|c| c.is_const()) {
        if let Some(const_sys) = sys.as_const_system() {
            use crate::solver::{solve, SolveResult};
            return match solve(&const_sys) {
                SolveResult::Sat(w) => FeautrierResult::Const(w),
                SolveResult::Unsat(_) => FeautrierResult::None,
                SolveResult::Unknown(r) => FeautrierResult::Unknown(r),
            };
        }
    }

    let splits = eliminate_variable(sys);
    if splits.is_empty() {
        return FeautrierResult::None;
    }

    let mut pieces: Vec<Piece> = Vec::new();
    for split in &splits {
        if split.system.nvars == 0 {
            let feasible = split.system.constraints.iter().all(|c| {
                c.as_const().map(|(_, k)| k >= 0).unwrap_or(true)
            });
            if feasible {
                pieces.push(Piece {
                    domain: split.condition.clone(),
                    distance: split.system.distance_expr.clone(), // D2: Use reconstructed distance!
                });
            }
        } else {
            match feautrier(&split.system) {
                FeautrierResult::Const(w) => {
                    pieces.push(Piece {
                        domain: split.condition.clone(),
                        distance: w.into_iter().map(ParamExpr::Const).collect(),
                    });
                }
                FeautrierResult::Piecewise(inner) => {
                    for p in inner {
                        pieces.push(Piece {
                            domain: format!("({}) && ({})", split.condition, p.domain),
                            distance: p.distance,
                        });
                    }
                }
                FeautrierResult::None => {}
                FeautrierResult::Unknown(r) => return FeautrierResult::Unknown(r),
            }
        }
    }

    if pieces.is_empty() {
        FeautrierResult::None
    } else if pieces.len() == 1 && pieces[0].domain == "true" {
        let all_const: Option<Vec<i64>> = pieces[0].distance.iter().map(|e| e.as_const()).collect();
        match all_const {
            Some(v) => FeautrierResult::Const(v),
            None => FeautrierResult::Piecewise(pieces),
        }
    } else {
        FeautrierResult::Piecewise(pieces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d1_parametric_constant_coeff() {
        let c = SymbolicConstraint {
            coeffs: vec![ParamExpr::Const(1), ParamExpr::Const(-1)],
            constant: ParamExpr::Param("n".into()),
        };
        let sys = SymbolicSystem {
            nvars: 2,
            params: vec!["n".into()],
            constraints: vec![c],
            distance_expr: vec![],
        };
        let result = feautrier(&sys);
        assert!(!matches!(result, FeautrierResult::Unknown(ref s) if s.contains("parametric coefficient")));
    }

    #[test]
    fn test_d2_distance_substitution() {
        let expr = ParamExpr::Sub(
            Box::new(ParamExpr::Param("x_1".into())),
            Box::new(ParamExpr::Param("x_0".into()))
        );
        let replaced = substitute(&expr, 1, &ParamExpr::Const(5));
        assert_eq!(replaced, ParamExpr::Sub(Box::new(ParamExpr::Const(5)), Box::new(ParamExpr::Param("x_0".into()))));
    }
}