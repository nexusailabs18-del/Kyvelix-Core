//! Symbolic parametric expressions for M1 Level 2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamExpr {
    Const(i64),
    Param(String),
    Mul(i64, Box<ParamExpr>),
    Div(Box<ParamExpr>, i64),
    Add(Box<ParamExpr>, Box<ParamExpr>),
    Sub(Box<ParamExpr>, Box<ParamExpr>),
}

impl ParamExpr {
    pub fn is_const(&self) -> bool {
        matches!(self, ParamExpr::Const(_))
    }

    pub fn as_const(&self) -> Option<i64> {
        match self {
            ParamExpr::Const(v) => Some(*v),
            _ => None,
        }
    }

    pub fn eval(&self, env: &[(String, i64)]) -> Option<i64> {
        match self {
            ParamExpr::Const(v) => Some(*v),
            ParamExpr::Param(name) => env.iter().find(|(n, _)| n == name).map(|(_, v)| *v),
            ParamExpr::Mul(c, e) => e.eval(env).and_then(|v| v.checked_mul(*c)),
            ParamExpr::Div(e, c) => {
                if *c == 0 { return None; }
                // checked_rem/checked_div guard `MIN / -1`, which panics.
                let v = e.eval(env)?;
                match v.checked_rem(*c) {
                    Some(0) => v.checked_div(*c),
                    _ => None,
                }
            }
            ParamExpr::Add(a, b) => Some(a.eval(env)?.checked_add(b.eval(env)?)?),
            ParamExpr::Sub(a, b) => Some(a.eval(env)?.checked_sub(b.eval(env)?)?),
        }
    }

    pub fn simplify(&self) -> ParamExpr {
        match self {
            ParamExpr::Const(v) => ParamExpr::Const(*v),
            ParamExpr::Param(s) => ParamExpr::Param(s.clone()),
            ParamExpr::Mul(c, e) => {
                let es = e.simplify();
                if let ParamExpr::Const(v) = es {
                    ParamExpr::Const(v * c)
                } else {
                    ParamExpr::Mul(*c, Box::new(es))
                }
            }
            ParamExpr::Div(e, c) => {
                let es = e.simplify();
                // x / 0 — treat as 0 (shouldn't happen)
                if *c == 0 {
                    return ParamExpr::Const(0);
                }
                // x / 1 = x
                if *c == 1 {
                    return es;
                }
                // constant / constant
                if let ParamExpr::Const(v) = &es {
                    if v % c == 0 {
                        return ParamExpr::Const(v / c);
                    }
                }
                // (x / a) / b = x / (a*b)
                if let ParamExpr::Div(inner, a) = &es {
                    if *c != 0 {
                        return ParamExpr::Div(inner.clone(), *a * *c).simplify();
                    }
                }
                ParamExpr::Div(Box::new(es), *c)
            }
            ParamExpr::Add(a, b) => {
                let as_ = a.simplify();
                let bs = b.simplify();
                match (&as_, &bs) {
                    (ParamExpr::Const(x), ParamExpr::Const(y)) => ParamExpr::Const(x + y),
                    (ParamExpr::Const(0), _) => bs,
                    (_, ParamExpr::Const(0)) => as_,
                    _ => ParamExpr::Add(Box::new(as_), Box::new(bs)),
                }
            }
            ParamExpr::Sub(a, b) => {
                let as_ = a.simplify();
                let bs = b.simplify();
                match (&as_, &bs) {
                    (ParamExpr::Const(x), ParamExpr::Const(y)) => ParamExpr::Const(x - y),
                    (_, ParamExpr::Const(0)) => as_,
                    _ => ParamExpr::Sub(Box::new(as_), Box::new(bs)),
                }
            }
        }
    }

    pub fn sub(&self, other: &ParamExpr) -> ParamExpr {
        ParamExpr::Sub(Box::new(self.clone()), Box::new(other.clone())).simplify()
    }

    pub fn add(&self, other: &ParamExpr) -> ParamExpr {
        ParamExpr::Add(Box::new(self.clone()), Box::new(other.clone())).simplify()
    }

    pub fn mul(&self, other: &ParamExpr) -> ParamExpr {
        if let Some(v) = self.as_const() {
            ParamExpr::Mul(v, Box::new(other.clone())).simplify()
        } else if let Some(v) = other.as_const() {
            ParamExpr::Mul(v, Box::new(self.clone())).simplify()
        } else {
            ParamExpr::Const(0)
        }
    }

    pub fn to_string_repr(&self) -> String {
        match self {
            ParamExpr::Const(v) => v.to_string(),
            ParamExpr::Param(s) => s.clone(),
            ParamExpr::Mul(c, e) => format!("{}*{}", c, e.to_string_repr()),
            ParamExpr::Div(e, c) => format!("{}/{}", e.to_string_repr(), c),
            ParamExpr::Add(a, b) => format!("({} + {})", a.to_string_repr(), b.to_string_repr()),
            ParamExpr::Sub(a, b) => format!("({} - {})", a.to_string_repr(), b.to_string_repr()),
        }
    }
}
