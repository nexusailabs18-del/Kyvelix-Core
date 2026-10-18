//! M1 — parametric affine loops.
//!
//! Level 0: resolve params + delegate to M0.
//! Level 1: parametric distance analysis (constant distance).
//! Level 2: Feautrier + Diophantine heuristic (parametric distance).
//!
//! Key insight: the distance between two accesses in a parametric loop
//! is either constant or a function of the parameters. Level 1 handles
//! the constant case directly. Level 2 uses a Diophantine heuristic
//! (search small integer di, solve for dj) and falls back to Feautrier.

use super::{Decision, m0};
use crate::certificate::{
    Certificate, Claim, DecisionLeaf, DecisionNode, DecisionTree, ParamCond, Terminal, Witness,
};
use crate::ir::*;
use crate::param_expr::ParamExpr;
use crate::symbolic::{feautrier, FeautrierResult, SymbolicConstraint, SymbolicSystem};

pub type ParamEnv = Vec<(String, i64)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParametricDistance {
    Const(Vec<i64>),
    Function(String),
    None,
    Unknown(String),
}

// ============================================================================
// Level 1 — parametric distance (constant)
// ============================================================================

pub fn analyze_distance(loop_param: &LoopParam, p: usize, q: usize) -> ParametricDistance {
    let ap = &loop_param.body[p];
    let aq = &loop_param.body[q];

    if ap.base != aq.base { return ParametricDistance::None; }
    if ap.kind == AccessKind::Read && aq.kind == AccessKind::Read {
        return ParametricDistance::None;
    }

    // Offsets must be concrete for the parametric distance analysis.
    let ap_off = match ap.offset.as_const() {
        Some(v) => v,
        None => return ParametricDistance::Unknown("symbolic offset".into()),
    };
    let aq_off = match aq.offset.as_const() {
        Some(v) => v,
        None => return ParametricDistance::Unknown("symbolic offset".into()),
    };

    let d = loop_param.depth;
    let mut g: i64 = 0;
    for k in 0..d {
        g = gcd_i64(g, ap.index[k]);
        g = gcd_i64(g, aq.index[k]);
    }
    let diff = aq_off - ap_off;
    if g > 1 && diff % g != 0 { return ParametricDistance::None; }

    // Realizability check: a candidate distance vector `dist` is a
    // real witness iff there exist IV values i_k, j_k in the ranges
    // [lo_k, hi_k) with j_k - i_k = dist[k]. For depth-1 loops and
    // const bounds this reduces to |dist[0]| < hi - lo. For parametric
    // bounds, we cannot decide without an env, so we return Unknown.
    let feasible = |dist: &[i64]| -> Option<bool> {
        for k in 0..d {
            let (lo, hi) = match (&loop_param.ivs[k].lower, &loop_param.ivs[k].upper) {
                (Bound::Const(lo), Bound::Const(hi)) => (*lo, *hi),
                _ => return None,  // parametric
            };
            if lo >= hi { return Some(false); }
            let delta = dist[k];
            let ok = if delta >= 0 {
                match lo.checked_add(delta) {
                    Some(v) => v < hi,
                    None => false,
                }
            } else {
                match lo.checked_sub(delta) {
                    Some(v) => v < hi,
                    None => false,
                }
            };
            if !ok { return Some(false); }
        }
        Some(true)
    };

    let same_stride = (0..d).all(|k| ap.index[k] == aq.index[k]);
    if same_stride {
        for k in 0..d {
            if ap.index[k] != 0 {
                let neg_diff = match diff.checked_neg() {
                    Some(v) => v,
                    None => continue,
                };
                if neg_diff % ap.index[k] == 0 {
                    let mut dist = vec![0i64; d];
                    dist[k] = neg_diff / ap.index[k];
                    match feasible(&dist) {
                        Some(true) => return ParametricDistance::Const(dist),
                        Some(false) => continue,  // try another k
                        None => return ParametricDistance::Unknown("parametric bounds".into()),
                    }
                }
            }
        }
        if d >= 2 {
            for k1 in 0..d {
                if ap.index[k1] == 0 { continue; }
                for k2 in 0..d {
                    if k1 == k2 || ap.index[k2] == 0 { continue; }
                    for dk1 in -100i64..=100 {
                        let idx_k1_dk1 = match ap.index[k1].checked_mul(dk1) {
                            Some(v) => v,
                            None => continue,
                        };
                        let neg_diff = match diff.checked_neg() {
                            Some(v) => v,
                            None => continue,
                        };
                        let rem = match neg_diff.checked_sub(idx_k1_dk1) {
                            Some(v) => v,
                            None => continue,
                        };
                        if rem % ap.index[k2] == 0 {
                            let dk2 = rem / ap.index[k2];
                            let mut dist = vec![0i64; d];
                            dist[k1] = dk1;
                            dist[k2] = dk2;
                            match feasible(&dist) {
                                Some(true) => return ParametricDistance::Const(dist),
                                Some(false) => continue,
                                None => return ParametricDistance::Unknown("parametric bounds".into()),
                            }
                        }
                    }
                }
            }
        }
        if diff == 0 {
            let zero = vec![0i64; d];
            match feasible(&zero) {
                Some(true) => return ParametricDistance::Const(zero),
                Some(false) => return ParametricDistance::None,
                None => return ParametricDistance::Unknown("parametric bounds".into()),
            }
        }
        return ParametricDistance::None;
    }
    ParametricDistance::Unknown("different strides, no Level 1 path".into())
}

// ============================================================================
// Level 2 — Feautrier + Diophantine (parametric distance)
// ============================================================================

pub fn build_symbolic_system(loop_param: &LoopParam, p: usize, q: usize) -> SymbolicSystem {
    let ap = &loop_param.body[p];
    let aq = &loop_param.body[q];
    let d = loop_param.depth;
    let n = 2 * d;

    let mut constraints: Vec<SymbolicConstraint> = Vec::new();

    let mut c1 = vec![ParamExpr::Const(0); n];
    for k in 0..d {
        c1[k] = ParamExpr::Const(ap.index[k]);
        c1[d + k] = ParamExpr::Const(-aq.index[k]);
    }
    let rhs1 = aq.offset.to_param_expr().sub(&ap.offset.to_param_expr());
    constraints.push(SymbolicConstraint { coeffs: c1.clone(), constant: rhs1 });

    let mut c2 = vec![ParamExpr::Const(0); n];
    for k in 0..d {
        c2[k] = ParamExpr::Const(-ap.index[k]);
        c2[d + k] = ParamExpr::Const(aq.index[k]);
    }
    let rhs2 = ap.offset.to_param_expr().sub(&aq.offset.to_param_expr());
    constraints.push(SymbolicConstraint { coeffs: c2, constant: rhs2 });

    for k in 0..d {
        let hi_expr = match &loop_param.ivs[k].upper {
            Bound::Const(v) => ParamExpr::Const(*v),
            Bound::Param(name) => ParamExpr::Param(name.clone()),
        };
        let lo_expr = match &loop_param.ivs[k].lower {
            Bound::Const(v) => ParamExpr::Const(*v),
            Bound::Param(name) => ParamExpr::Param(name.clone()),
        };

        let one = ParamExpr::Const(1);
        let minus_one = ParamExpr::Const(-1);
        let zero = ParamExpr::Const(0);

        let mut c = vec![ParamExpr::Const(0); n];
        c[k] = one.clone();
        constraints.push(SymbolicConstraint { coeffs: c, constant: hi_expr.sub(&one) });

        let mut c = vec![ParamExpr::Const(0); n];
        c[k] = minus_one.clone();
        constraints.push(SymbolicConstraint { coeffs: c, constant: zero.sub(&lo_expr) });

        let mut c = vec![ParamExpr::Const(0); n];
        c[d + k] = one.clone();
        constraints.push(SymbolicConstraint { coeffs: c, constant: hi_expr.sub(&one) });

        let mut c = vec![ParamExpr::Const(0); n];
        c[d + k] = minus_one.clone();
        constraints.push(SymbolicConstraint { coeffs: c, constant: zero.sub(&lo_expr) });
    }

    let params: Vec<String> = loop_param.ivs.iter().flat_map(|iv| {
        let mut names = Vec::new();
        if let Bound::Param(n) = &iv.upper { names.push(n.clone()); }
        if let Bound::Param(n) = &iv.lower { names.push(n.clone()); }
        names
    }).collect();

    // D2: Initialize distance expression (j_k - i_k)
    let mut distance_expr = Vec::with_capacity(d);
    for k in 0..d {
        let j_k = ParamExpr::Param(format!("x_{}", d + k));
        let i_k = ParamExpr::Param(format!("x_{}", k));
        distance_expr.push(ParamExpr::Sub(Box::new(j_k), Box::new(i_k)).simplify());
    }

    SymbolicSystem { nvars: n, params, constraints, distance_expr }
}

pub fn feautrier_to_distance(result: FeautrierResult) -> ParametricDistance {
    match result {
        FeautrierResult::Const(v) => ParametricDistance::Const(v),
        FeautrierResult::None => ParametricDistance::None,
        FeautrierResult::Unknown(r) => ParametricDistance::Unknown(r),
        FeautrierResult::Piecewise(pieces) => {
            if pieces.is_empty() { return ParametricDistance::None; }
            let mut s = String::new();
            for (i, p) in pieces.iter().enumerate() {
                if i > 0 { s.push_str("; "); }
                let dist_str = p.distance.iter().map(|e| e.to_string_repr()).collect::<Vec<_>>().join(", ");
                s.push_str(&format!("if {}: [{}]", p.domain, dist_str));
            }
            ParametricDistance::Function(s)
        }
    }
}

pub fn extract_param_distance_1d(loop_param: &LoopParam, p: usize, q: usize) -> Option<ParametricDistance> {
    if loop_param.depth != 1 { return None; }
    let ap = &loop_param.body[p];
    let aq = &loop_param.body[q];
    let stride = ap.index[0];
    if stride == 0 || aq.index[0] != stride { return None; }

    let diff = ap.offset.to_param_expr().sub(&aq.offset.to_param_expr()).simplify();
    if let Some(d) = diff.as_const() {
        if d % stride == 0 { return Some(ParametricDistance::Const(vec![d / stride])); }
        return Some(ParametricDistance::None);
    }
    let dist_expr = ParamExpr::Div(Box::new(diff), stride).simplify();
    if let Some(v) = dist_expr.as_const() { return Some(ParametricDistance::Const(vec![v])); }
    Some(ParametricDistance::Function(dist_expr.to_string_repr()))
}

pub fn extract_param_distance_2d(loop_param: &LoopParam, p: usize, q: usize) -> Option<ParametricDistance> {
    if loop_param.depth != 2 { return None; }
    let ap = &loop_param.body[p];
    let aq = &loop_param.body[q];
    if ap.index != aq.index { return None; }

    let c0 = ap.index[0];
    let c1 = ap.index[1];
    if c0 == 0 && c1 == 0 { return None; }

    let diff = ap.offset.to_param_expr().sub(&aq.offset.to_param_expr()).simplify();

    // D2: Try to solve c0 * di + c1 * dj = diff for small integer di or dj
    // This elegantly handles cases like A[i][j] = A[i+1][j-1] where diff = C - 1, c0 = C, c1 = 1
    for di in -3..=3 {
        let rhs = diff.clone().sub(&ParamExpr::Mul(c0, Box::new(ParamExpr::Const(di)))).simplify();
        if c1 != 0 {
            let dj_expr = ParamExpr::Div(Box::new(rhs.clone()), c1).simplify();
            if dj_expr.as_const().is_some() {
                return Some(ParametricDistance::Const(vec![di, dj_expr.as_const().unwrap()]));
            }
            let dj_str = dj_expr.to_string_repr();
            if !dj_str.contains('/') || dj_str.ends_with("/1") || dj_str.ends_with("/ 1") {
                return Some(ParametricDistance::Function(format!("{}, {}", di, dj_str)));
            }
        }
    }
    for dj in -3..=3 {
        let rhs = diff.clone().sub(&ParamExpr::Mul(c1, Box::new(ParamExpr::Const(dj)))).simplify();
        if c0 != 0 {
            let di_expr = ParamExpr::Div(Box::new(rhs.clone()), c0).simplify();
            if di_expr.as_const().is_some() {
                return Some(ParametricDistance::Const(vec![di_expr.as_const().unwrap(), dj]));
            }
            let di_str = di_expr.to_string_repr();
            if !di_str.contains('/') || di_str.ends_with("/1") || di_str.ends_with("/ 1") {
                return Some(ParametricDistance::Function(format!("{}, {}", di_str, dj)));
            }
        }
    }

    // Fallback to original single-dimension logic
    if c0 != 0 {
        let dist0 = ParamExpr::Div(Box::new(diff.clone()), c0).simplify();
        if let Some(v) = dist0.as_const() { return Some(ParametricDistance::Const(vec![v, 0])); }
        if c1 == 0 || c0.abs() < c1.abs() {
            return Some(ParametricDistance::Function(format!("{}, 0", dist0.to_string_repr())));
        }
    }
    if c1 != 0 {
        let dist1 = ParamExpr::Div(Box::new(diff), c1).simplify();
        if let Some(v) = dist1.as_const() { return Some(ParametricDistance::Const(vec![0, v])); }
        return Some(ParametricDistance::Function(format!("0, {}", dist1.to_string_repr())));
    }
    None
}

pub fn extract_param_distance_2d_general(loop_param: &LoopParam, p: usize, q: usize) -> Option<ParametricDistance> {
    if loop_param.depth != 2 { return None; }
    let ap = &loop_param.body[p];
    let aq = &loop_param.body[q];
    if ap.index == aq.index { return None; } // Handled by extract_param_distance_2d

    let c0_a = ap.index[0]; let c1_a = ap.index[1];
    let c0_q = aq.index[0]; let c1_q = aq.index[1];

    if c0_a == c0_q && c1_a != c1_q {
        let diff = ap.offset.to_param_expr().sub(&aq.offset.to_param_expr()).simplify();
        if c1_q != 0 {
            let dist1 = ParamExpr::Div(Box::new(diff), c1_q).simplify();
            return Some(ParametricDistance::Function(format!("0, {}", dist1.to_string_repr())));
        }
    }
    if c1_a == c1_q && c0_a != c0_q {
        let diff = ap.offset.to_param_expr().sub(&aq.offset.to_param_expr()).simplify();
        if c0_q != 0 {
            let dist0 = ParamExpr::Div(Box::new(diff), c0_q).simplify();
            return Some(ParametricDistance::Function(format!("{}, 0", dist0.to_string_repr())));
        }
    }
    None
}

pub fn analyze_distance_level2(loop_param: &LoopParam, p: usize, q: usize) -> ParametricDistance {
    let ap = &loop_param.body[p];
    let aq = &loop_param.body[q];
    if ap.base != aq.base { return ParametricDistance::None; }
    if ap.kind == AccessKind::Read && aq.kind == AccessKind::Read { return ParametricDistance::None; }

    if ap.offset.is_const() && aq.offset.is_const() {
        return analyze_distance(loop_param, p, q);
    }

    if let Some(d) = extract_param_distance_1d(loop_param, p, q) { return d; }
    if let Some(d) = extract_param_distance_2d(loop_param, p, q) { return d; }
    if let Some(d) = extract_param_distance_2d_general(loop_param, p, q) { return d; }

    let sys = build_symbolic_system(loop_param, p, q);
    feautrier_to_distance(feautrier(&sys))
}

// ============================================================================
// Level 0 — resolve params and delegate to M0
// ============================================================================

pub fn decide_param(loop_param: &LoopParam, env: &ParamEnv, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    if !loop_param.is_parametric() { return (Decision::Unknown("not parametric".into()), None); }
    // Level 3 first: a parametric decision tree certifies the whole parameter
    // region, which a resolved instance cannot. It only applies to same-stride
    // depth-1 pairs and only when the exact integer solver agrees with it, so
    // falling through to Level 0 is the normal case, not an error.
    if let Some(out) = decide_parametric(loop_param, env, p, q) {
        return out;
    }
    match loop_param.resolve(env) {
        Some(resolved) => {
            if resolved.body.iter().any(|a| a.expr.is_some()) {
                super::m2g::decide(&resolved, p, q)
            } else {
                m0::decide(&resolved, p, q)
            }
        }
        None => (Decision::Unknown("unresolved params".into()), None),
    }
}

/// Emit a `Claim::Parametric(DecisionTree)` certificate for a depth-1
/// same-stride parametric pair.
///
/// `None` means this level does not apply -- wrong shape, the exact solver
/// disagreed, or a witness component is not evaluable -- and the caller falls
/// back to Level 0. A `Decision::None` here is a *certified* absence: the tree
/// reached a refutation terminal and the exact integer solver independently
/// proved no colliding pair exists.
pub fn decide_parametric(
    loop_param: &LoopParam,
    env: &ParamEnv,
    p: usize,
    q: usize,
) -> Option<(Decision, Option<Certificate>)> {
    let (tree, leaf, idx) = solve_param_system(loop_param, env, p, q)?;
    let resolved = loop_param.resolve(env)?;
    let decision = match &leaf {
        Some(l) => {
            let dist = l.distance.iter().map(|e| e.eval(env)).collect::<Option<Vec<i64>>>()?;
            Decision::Exact(dist)
        }
        None => Decision::None,
    };
    let cert = Certificate {
        level: "M1".into(),
        loop_hash: resolved.hash(),
        pair: (p, q),
        claim: Claim::Parametric(tree),
        witness: Witness::ParametricSolution {
            env: env.to_vec(),
            leaf: idx,
        },
        checker: "m0-check-v1".into(),
    };
    Some((decision, Some(cert)))
}
 
pub fn can_handle(loop_param: &LoopParam) -> bool {
    loop_param.is_parametric()
}

// ============================================================================
// Level 3 — parametric decision tree (Phase 2, reference §2.8 / §2.9)
// ============================================================================
//
// M1's Level 1 and 2 *compute* a parametric distance but cannot certify it:
// `ParametricDistance::Function("i/2")` says nothing the checker can verify, so
// `remaining.md` records "Full Diophantine -> UNKNOWN". This section emits the
// missing piece, `Claim::Parametric(DecisionTree)`, for the case the reference
// calls out -- same-stride accesses whose distance is a quotient of the
// parameter difference -- and every tree it builds is validated against the
// exact integer solver of `crate::solver::integer` before it is handed out.

/// A depth-1 same-stride pair reduced to the one linear condition a colliding
/// iteration pair must satisfy.
///
/// For `A[c·i + o0]` and `A[c·j + o1]` the locations are equal exactly when
///
/// ```text
///     c·i + o0 = c·j + o1      <=>      c·(j - i) = o0 - o1
/// ```
///
/// so the *distance* is pinned to the single value `(o0 - o1)/c`, while the base
/// iteration is free. The dependence therefore exists iff that quotient is an
/// integer and its magnitude fits inside the iteration range -- which is why a
/// finite case split over the parameters is possible here, and why the witness
/// for a whole region of parameter space is `j = lo + (o0 - o1)/c`, not one
/// concrete pair.
///
/// `Unsupported` covers everything else (different strides, non-affine index
/// kinds, ranges, indirections): the caller must fall back to the existing
/// levels, and this module never answers "no dependence" on its own behalf.
pub enum SameStrideEquation {
    /// `c·(j - i) = n`, with `c != 0`.
    FixedDistance { c: i64, n: ParamExpr },
    Unsupported,
}

/// Extract the [`SameStrideEquation`] for a depth-1 pair, or `Unsupported`.
pub fn build_param_system(lp: &LoopParam, p: usize, q: usize) -> SameStrideEquation {
    if lp.depth != 1 || p >= lp.body.len() || q >= lp.body.len() || p == q {
        return SameStrideEquation::Unsupported;
    }
    let (ap, aq) = (&lp.body[p], &lp.body[q]);
    if ap.base != aq.base {
        return SameStrideEquation::Unsupported;
    }
    if !matches!(ap.index_kind, IndexKind::Affine)
        || !matches!(aq.index_kind, IndexKind::Affine)
    {
        return SameStrideEquation::Unsupported;
    }
    // Bounded ranges and table indirection carry their own semantics, decided
    // by M2b/M2a.5; a parametric tree must not reinterpret them.
    if ap.range.is_some()
        || aq.range.is_some()
        || ap.indirect.is_some()
        || aq.indirect.is_some()
    {
        return SameStrideEquation::Unsupported;
    }
    // Two reads are not a dependence, whatever the subscripts do -- that is
    // M0's Stage 2, and this level must not contradict it.
    if ap.kind == AccessKind::Read && aq.kind == AccessKind::Read {
        return SameStrideEquation::Unsupported;
    }
    // A non-unit step makes the admissible iterations a sublattice of the
    // interval, so the range guards below (which are interval guards) would be
    // too generous. Leave those to the resolved-instance path.
    if lp.ivs[0].step != 1 {
        return SameStrideEquation::Unsupported;
    }
    let c0 = ap.index[0];
    if c0 == 0 || aq.index[0] != c0 {
        // Different strides make the collision condition a two-variable
        // Diophantine equation with a free parameter, whose feasible set is not
        // a finite union of these guard shapes. M0's FM path (with the exact
        // bounds test of Stage 4b) decides the resolved instance instead.
        return SameStrideEquation::Unsupported;
    }
    SameStrideEquation::FixedDistance {
        c: c0,
        n: ap.offset.to_param_expr().sub(&aq.offset.to_param_expr()),
    }
}

/// Build the decision tree for a depth-1 same-stride parametric pair, together
/// with the terminal the tree reaches at `env`.
///
/// Returns `None` when this construction does not apply, which the caller must
/// treat as "fall back to the other levels" and never as "no dependence".
///
/// # The split
///
/// `c·(j - i) = n` has an integer solution exactly when `c` divides `n`, and
/// then the distance `d = n / c` is unique. So the tree asks three questions,
/// in the order the arithmetic makes them available:
///
/// ```text
///     n ≡ 0 (mod c) ?   no -> NoDependence
///     d >= 0        ?   yes -> need d <= hi - lo
///                        no -> need -d <= hi - lo
/// ```
///
/// and each range question ends in a leaf or a refutation. The witness for a
/// dependence leaf slides to whichever end of the range keeps both iterations
/// inside: `i = lo, j = lo + d` when `d >= 0`, `i = lo - d, j = lo` when
/// `d < 0`. That is why one leaf covers a whole region of parameter space:
/// the *distance* is fixed, the *base iteration* is free.
///
/// # Self-checking against the exact integer solver
///
/// Every terminal the walk reaches is re-decided from scratch by
/// [`crate::solver::integer::solve_integer_mixed`], which solves the concrete
/// collision equation over the integers by Hermite normal form and the range
/// box by integer Fourier-Motzkin. If that oracle disagrees with the tree --
/// or cannot decide -- this function returns `None`. The tree is therefore
/// never the sole authority for the certificate it produces: an arithmetic
/// slip in the guards above turns into "no certificate", not into a wrong one.
pub fn solve_param_system(
    lp: &LoopParam,
    env: &ParamEnv,
    p: usize,
    q: usize,
) -> Option<(DecisionTree, Option<DecisionLeaf>, usize)> {
    let SameStrideEquation::FixedDistance { c, n } = build_param_system(lp, p, q) else {
        return None;
    };
    let resolved = lp.resolve(env)?;
    let lo = resolved.ivs[0].lower;
    let hi = resolved.ivs[0].upper.saturating_sub(1);

    // An empty range admits no pair at all, whatever the equation says.
    if hi < lo {
        let tree = DecisionTree {
            root: DecisionNode::NoDependence,
        };
        return Some((tree, None, 0));
    }

    // d = n / c, exact only when c | n; ParamExpr::Div refuses to round.
    let d = ParamExpr::Div(Box::new(n.clone()), c);
    let span = match hi.checked_sub(lo) {
        Some(s) => s,
        // A range so wide that its span leaves i64 cannot be guarded by the
        // interval conditions below; decline rather than wrap.
        None => return None,
    };
    let base = ParamExpr::Const(lo);

    let pos_leaf = DecisionNode::Leaf(DecisionLeaf {
        i: vec![base.clone()],
        j: vec![base.add(&d)],
        distance: vec![d.clone()],
    });
    let neg_leaf = DecisionNode::Leaf(DecisionLeaf {
        i: vec![base.sub(&d)],
        j: vec![base],
        distance: vec![d.clone()],
    });

    let root = DecisionTree::branch(
        ParamCond::Mod(n.clone(), c),
        DecisionTree::branch(
            ParamCond::Ge(d.clone(), 0),
            DecisionTree::branch(
                ParamCond::Le(d.clone(), span),
                pos_leaf,
                DecisionNode::NoDependence,
            ),
            DecisionTree::branch(
                ParamCond::Ge(d.clone(), -span),
                neg_leaf,
                DecisionNode::NoDependence,
            ),
        ),
        DecisionNode::NoDependence,
    );
    let tree = DecisionTree { root };

    let terminal = tree.root.resolve_terminal(env)?;
    // Validate against the exact integer solver before handing the tree out.
    // The oracle decides the *concrete* instance from scratch -- HNF for the
    // collision equation, integer Fourier-Motzkin for the range box -- so a
    // slip in the guards above shows up as a disagreement rather than as a
    // certificate.
    let oracle = integer_oracle(&resolved, p, q, env);
    let agrees = match (&terminal, &oracle) {
        (Terminal::Dependence(_, leaf), Some(Some(dist))) => {
            leaf.distance.first().and_then(|e| e.eval(env)) == Some(*dist)
        }
        (Terminal::Absent(_), Some(None)) => true,
        // disagreement, or the exact solver could not decide: not certifiable
        _ => false,
    };
    if !agrees {
        return None;
    }

    match terminal {
        Terminal::Dependence(idx, leaf) => Some((tree, Some(leaf), idx)),
        Terminal::Absent(idx) => Some((tree, None, idx)),
    }
}

/// Re-decide a *resolved* depth-1 pair with the exact integer machinery: the
/// collision equation `c·i - c·j = o1 - o0` solved by HNF, the range box by
/// integer Fourier-Motzkin.
///
/// `Some(Some(d))` = a colliding integer pair exists and its distance `j - i`
/// is `d` (the same-stride equation pins it to a single value), `Some(None)` =
/// proven that none exists, `None` = the exact solver could not decide.
fn integer_oracle(loop_: &Loop, p: usize, q: usize, env: &ParamEnv) -> Option<Option<i64>> {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];
    let c = ap.index[0];
    let o0 = ap.offset.resolve(env)?;
    let o1 = aq.offset.resolve(env)?;
    let lo = loop_.ivs[0].lower;
    let hi = loop_.ivs[0].upper.saturating_sub(1);
    if hi < lo {
        return Some(None);
    }

    // A·x = b with x = (i, j): c·i - c·j = o1 - o0.
    let a = crate::solver::inverse::Matrix::from_rows(&[vec![c, -c]])?;
    let b = [o1 - o0];
    // C·x <= d: the range box.
    let cc = crate::solver::inverse::Matrix::from_rows(&[
        vec![1, 0],
        vec![-1, 0],
        vec![0, 1],
        vec![0, -1],
    ])?;
    let dd = [hi, -lo, hi, -lo];
    match crate::solver::integer::solve_integer_mixed(&a, &b, &cc, &dd) {
        crate::solver::integer::IntSolveResult::Solutions { particular, .. } => {
            Some(Some(particular[1] - particular[0]))
        }
        crate::solver::integer::IntSolveResult::NoSolution => Some(None),
        crate::solver::integer::IntSolveResult::Unknown => None,
    }
}


fn gcd_i64(mut a: i64, mut b: i64) -> i64 {
    a = a.abs(); b = b.abs();
    while b != 0 { let t = b; b = a % b; a = t; }
    a
}     

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_acc_affine(kind: AccessKind, index: i64, offset: OffsetValue) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![index],
            elem_size: 8,
            offset,
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn mk_loop_param(lo: Bound, hi: Bound, body: Vec<Access>) -> LoopParam {
        LoopParam {
            depth: 1,
            ivs: vec![IVParam {
                name: "i".into(),
                lower: lo,
                upper: hi,
                step: 1,
            }],
            body,
        }
    }

    fn xorshift1(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    // ----------------------------------------------------------------
    // Level 0: decide_param — resolve params, delegate to M0/M2g.
    // ----------------------------------------------------------------

    #[test]
    fn fuzz_m1_decide_param() {
        let mut state: u64 = 0x1_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Parametric loop: upper bound is param "n", lower is 0.
            let n_val = (xorshift1(&mut state) % 24) as i64;
            let env = vec![("n".to_string(), n_val)];

            let ip = (xorshift1(&mut state) % 5) as i64 - 2;
            let iq = (xorshift1(&mut state) % 5) as i64 - 2;
            let op_ = (xorshift1(&mut state) % 9) as i64 - 4;
            let oq = (xorshift1(&mut state) % 9) as i64 - 4;

            let lp = mk_loop_param(
                Bound::Const(0),
                Bound::Param("n".into()),
                vec![
                    mk_acc_affine(AccessKind::Write, ip, OffsetValue::Const(op_)),
                    mk_acc_affine(AccessKind::Read,  iq, OffsetValue::Const(oq)),
                ],
            );

            // Ground truth: resolve, then brute force over [0, n).
            if n_val <= 0 { continue; }
            let mut wp = std::collections::HashSet::new();
            for i in 0..n_val { wp.insert(ip * i + op_); }
            let mut expected_exact = false;
            for j in 0..n_val {
                if wp.contains(&(iq * j + oq)) { expected_exact = true; break; }
            }

            let (d, _) = decide_param(&lp, &env, 0, 1);
            tested += 1;
            let ok = match (expected_exact, &d) {
                (true, Decision::Exact(_)) => true,
                (true, Decision::Unknown(_)) => { conservative += 1; true }
                (true, _) => false,
                (false, Decision::None) => true,
                (false, Decision::Unknown(_)) => { conservative += 1; true }
                (false, _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "n={} ip={} op={} iq={} oq={} expected_exact={} got={:?}",
                        n_val, ip, op_, iq, oq, expected_exact, d
                    ));
                }
            }
        }

        println!("M1 Level-0 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M1 Level-0 found {} unsound", unsound);
    }

    // ----------------------------------------------------------------
    // Level 1: analyze_distance — parametric distance (const case).
    // ----------------------------------------------------------------

    #[test]
    fn fuzz_m1_analyze_distance() {
        let mut state: u64 = 0x1_abcdef01;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            let n_val = (xorshift1(&mut state) % 32) as i64;
            if n_val <= 1 { continue; }

            // Use constant offsets so analyze_distance can decide directly.
            let ip = (xorshift1(&mut state) % 5) as i64 - 2;
            let iq = (xorshift1(&mut state) % 5) as i64 - 2;
            let op_ = (xorshift1(&mut state) % 9) as i64 - 4;
            let oq = (xorshift1(&mut state) % 9) as i64 - 4;

            // analyze_distance works on LoopParam with Param bounds.
            // Use Const bounds for the IV range but a Param upper to
            // exercise the parametric path.
            let lp = mk_loop_param(
                Bound::Const(0),
                Bound::Const(n_val),
                vec![
                    mk_acc_affine(AccessKind::Write, ip, OffsetValue::Const(op_)),
                    mk_acc_affine(AccessKind::Read,  iq, OffsetValue::Const(oq)),
                ],
            );

            // Ground truth over [0, n_val).
            let mut wp = std::collections::HashSet::new();
            for i in 0..n_val { wp.insert(ip * i + op_); }
            let mut expected_exact = false;
            for j in 0..n_val {
                if wp.contains(&(iq * j + oq)) { expected_exact = true; break; }
            }

            let result = analyze_distance(&lp, 0, 1);
            tested += 1;
            let ok = match (&result, expected_exact) {
                (ParametricDistance::Const(_), true) => true,
                (ParametricDistance::None, false) => true,
                (ParametricDistance::Unknown(_), _) => true,  // conservative
                (ParametricDistance::Function(_), _) => true, // conservative/parametric
                _ => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "n={} ip={} op={} iq={} oq={} expected_exact={} got={:?}",
                        n_val, ip, op_, iq, oq, expected_exact, result
                    ));
                }
            }
        }

        println!("M1 Level-1 fuzz: tested={} unsound={}", tested, unsound);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M1 Level-1 found {} unsound", unsound);
    }

    // ----------------------------------------------------------------
    // Level 2: analyze_distance_level2 — Feautrier + Diophantine.
    // The highest-risk path: symbolic elimination, heuristics with
    // bounded search ranges, ParamExpr simplification.
    // ----------------------------------------------------------------

    #[test]
    fn fuzz_m1_analyze_distance_level2() {
        let mut state: u64 = 0x1_c0ffee01;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            let n_val = (xorshift1(&mut state) % 24) as i64;
            if n_val <= 1 { continue; }
            let p_val = (xorshift1(&mut state) % 8) as i64;

            // Parametric offset: A[i + p] vs A[i].
            // diff = offset_p - offset_q = p, stride = 1, so distance is p.
            let ip = (xorshift1(&mut state) % 3) as i64 - 1;
            let iq = ip;
            let param = ParamExpr::Param("p".into());
            let off_p = OffsetValue::Const((xorshift1(&mut state) % 5) as i64 - 2);
            let off_q = OffsetValue::Const((xorshift1(&mut state) % 5) as i64 - 2);

            // Also try a parametric-offset case: one side has a Param offset.
            let use_param_offset = xorshift1(&mut state) % 2 == 0;
            let (op_, oq) = if use_param_offset {
                (OffsetValue::Param(param.clone()), OffsetValue::Const(0))
            } else {
                (off_p, off_q)
            };

            let lp = mk_loop_param(
                Bound::Const(0),
                Bound::Param("n".into()),
                vec![
                    mk_acc_affine(AccessKind::Write, ip, op_.clone()),
                    mk_acc_affine(AccessKind::Read,  iq, oq.clone()),
                ],
            );

            // Ground truth: resolve offset params with env = [("n", n_val), ("p", p_val)].
            let env = vec![("n".to_string(), n_val), ("p".to_string(), p_val)];
            let op_resolved = op_.resolve(&env);
            let oq_resolved = oq.resolve(&env);
            let (op_val, oq_val) = match (op_resolved, oq_resolved) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };

            let mut wp = std::collections::HashSet::new();
            for i in 0..n_val { wp.insert(ip * i + op_val); }
            let mut expected_exact = false;
            for j in 0..n_val {
                if wp.contains(&(iq * j + oq_val)) { expected_exact = true; break; }
            }

            let result = analyze_distance_level2(&lp, 0, 1);
            tested += 1;
            // For the Level-2 analyzer, only Const/None are claims.
            // Function is a parametric description — the test treats
            // it as conservative (unverifiable without more work).
            let ok = match (&result, expected_exact) {
                (ParametricDistance::Const(_), true) => true,
                (ParametricDistance::None, false) => true,
                (ParametricDistance::Unknown(_), _) => { conservative += 1; true }
                (ParametricDistance::Function(_), _) => { conservative += 1; true }
                _ => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "n={} p={} ip={} iq={} op={:?} oq={:?} expected_exact={} got={:?}",
                        n_val, p_val, ip, iq, op_, oq, expected_exact, result
                    ));
                }
            }
        }

        println!("M1 Level-2 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M1 Level-2 found {} unsound", unsound);
    }
    // ----------------------------------------------------------------
    // Level 3: parametric decision tree (Phase 2).
    // ----------------------------------------------------------------

    /// A parametric offset access: `A[index*i + P]`.
    fn mk_acc_param(kind: AccessKind, index: i64, e: ParamExpr) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![index],
            elem_size: 8,
            offset: OffsetValue::Param(e),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn param_env(pairs: &[(&str, i64)]) -> ParamEnv {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn tree_parametric_distance_leaf() {
        // A[i + P] (write) vs A[i] (read), i in [0, N): the collision needs
        // j - i = P, so the distance is the *parameter* and the leaf must
        // carry it symbolically.
        let lp = mk_loop_param(
            Bound::Const(0),
            Bound::Param("N".into()),
            vec![
                mk_acc_param(AccessKind::Write, 1, ParamExpr::Param("P".into())),
                mk_acc_affine(AccessKind::Read, 1, OffsetValue::Const(0)),
            ],
        );
        let env = param_env(&[("N", 10), ("P", 3)]);
        let (d, cert) = decide_param(&lp, &env, 0, 1);
        assert!(
            matches!(d, Decision::Exact(ref v) if v == &vec![3]),
            "expected Exact([3]), got {d:?}"
        );
        let cert = cert.expect("parametric certificate");
        assert_eq!(cert.level, "M1");
        let tree = match &cert.claim {
            Claim::Parametric(t) => t,
            other => panic!("expected a Parametric claim, got {other:?}"),
        };
        // The same tree, walked at a different P, yields a different distance:
        // that is the whole point of a parametric certificate.
        let env7 = param_env(&[("N", 10), ("P", 7)]);
        let leaf = tree.root.resolve(&env7).expect("P=7 reaches a leaf");
        assert_eq!(leaf.distance[0].eval(&env7), Some(7));
        let i = leaf.i[0].eval(&env7).unwrap();
        let j = leaf.j[0].eval(&env7).unwrap();
        assert_eq!(j - i, 7);
        assert!(i >= 0 && i < 10 && j >= 0 && j < 10, "witness out of range");
        match cert.witness {
            Witness::ParametricSolution { leaf: idx, .. } => {
                let t = tree.root.resolve_terminal(&env).unwrap();
                let want = match t {
                    Terminal::Dependence(k, _) => k,
                    Terminal::Absent(k) => k,
                };
                assert_eq!(idx, want, "witness must name the terminal env selects");
            }
            other => panic!("expected ParametricSolution, got {other:?}"),
        }
    }

    #[test]
    fn tree_refutes_when_distance_does_not_divide() {
        // A[2i + P] vs A[2i]: 2*(j - i) = P, so an odd P has no integer
        // solution and the tree must reach a refutation terminal.
        let lp = mk_loop_param(
            Bound::Const(0),
            Bound::Param("N".into()),
            vec![
                mk_acc_param(AccessKind::Write, 2, ParamExpr::Param("P".into())),
                mk_acc_affine(AccessKind::Read, 2, OffsetValue::Const(0)),
            ],
        );
        let env_odd = param_env(&[("N", 20), ("P", 3)]);
        let (d, cert) = decide_param(&lp, &env_odd, 0, 1);
        assert!(matches!(d, Decision::None), "odd P should refute, got {d:?}");
        let cert = cert.expect("refutation certificate");
        let tree = match &cert.claim {
            Claim::Parametric(t) => t,
            other => panic!("{other:?}"),
        };
        assert!(tree.root.resolve(&env_odd).is_none());

        // an even P does collide, with distance P/2
        let env_even = param_env(&[("N", 20), ("P", 4)]);
        let (d2, _) = decide_param(&lp, &env_even, 0, 1);
        assert!(
            matches!(d2, Decision::Exact(ref v) if v == &vec![2]),
            "P=4 should give distance 2, got {d2:?}"
        );
    }

    #[test]
    fn tree_refutes_when_distance_out_of_range() {
        // With N = 10 the largest attainable |j - i| is 9.
        let lp = mk_loop_param(
            Bound::Const(0),
            Bound::Param("N".into()),
            vec![
                mk_acc_param(AccessKind::Write, 1, ParamExpr::Param("P".into())),
                mk_acc_affine(AccessKind::Read, 1, OffsetValue::Const(0)),
            ],
        );
        for (p, want_exists) in [(9i64, true), (10, false), (-9, true), (-10, false)] {
            let env = param_env(&[("N", 10), ("P", p)]);
            let (d, _) = decide_param(&lp, &env, 0, 1);
            match (want_exists, &d) {
                (true, Decision::Exact(v)) => assert_eq!(v, &vec![p], "P={p}"),
                (false, Decision::None) => {}
                (w, other) => panic!("P={p}: wanted existence={w}, got {other:?}"),
            }
        }
    }
    #[test]
    fn tree_declines_outside_its_shape() {
        // Different strides: not this level's equation shape.
        let lp = mk_loop_param(
            Bound::Const(0),
            Bound::Param("N".into()),
            vec![
                mk_acc_affine(AccessKind::Write, 2, OffsetValue::Const(0)),
                mk_acc_affine(AccessKind::Read, 3, OffsetValue::Const(1)),
            ],
        );
        let env = param_env(&[("N", 10)]);
        assert!(matches!(
            build_param_system(&lp, 0, 1),
            SameStrideEquation::Unsupported
        ));
        assert!(decide_parametric(&lp, &env, 0, 1).is_none());

        // Two reads are not a dependence.
        let lp2 = mk_loop_param(
            Bound::Const(0),
            Bound::Param("N".into()),
            vec![
                mk_acc_affine(AccessKind::Read, 1, OffsetValue::Const(0)),
                mk_acc_affine(AccessKind::Read, 1, OffsetValue::Const(1)),
            ],
        );
        assert!(matches!(
            build_param_system(&lp2, 0, 1),
            SameStrideEquation::Unsupported
        ));

        // A non-unit step makes the interval guards unsound: decline.
        let mut lp3 = mk_loop_param(
            Bound::Const(0),
            Bound::Param("N".into()),
            vec![
                mk_acc_affine(AccessKind::Write, 1, OffsetValue::Const(0)),
                mk_acc_affine(AccessKind::Read, 1, OffsetValue::Const(2)),
            ],
        );
        lp3.ivs[0].step = 2;
        assert!(matches!(
            build_param_system(&lp3, 0, 1),
            SameStrideEquation::Unsupported
        ));
    }

    /// Ground truth: the unique distance of a same-stride pair, by enumeration.
    fn brute_distance(n: i64, c: i64, p: i64, oq: i64) -> Option<i64> {
        for i in 0..n {
            for j in 0..n {
                if c * i + p == c * j + oq {
                    return Some(j - i);
                }
            }
        }
        None
    }

    #[test]
    fn fuzz_m1_parametric_tree_against_brute_force() {
        // Same-stride depth-1 pairs with a parametric offset, over random
        // environments. Ground truth is enumeration of the resolved instance.
        // Every terminal the tree reaches must agree with it, and every
        // certificate must be self-consistent when re-walked at its own env.
        let mut state: u64 = 0xf00d_face_1234_5678;
        let (mut tested, mut certified, mut wrong, mut declined) = (0u64, 0u64, 0u64, 0u64);
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            let n_val = 1 + (xorshift1(&mut state) % 12) as i64;
            let p_val = (xorshift1(&mut state) % 11) as i64 - 5;
            let c = 1 + (xorshift1(&mut state) % 3) as i64;
            let oq = (xorshift1(&mut state) % 5) as i64 - 2;
            let env = param_env(&[("N", n_val), ("P", p_val)]);

            let lp = mk_loop_param(
                Bound::Const(0),
                Bound::Param("N".into()),
                vec![
                    mk_acc_param(AccessKind::Write, c, ParamExpr::Param("P".into())),
                    mk_acc_affine(AccessKind::Read, c, OffsetValue::Const(oq)),
                ],
            );
            tested += 1;
            let truth = brute_distance(n_val, c, p_val, oq);

            match decide_parametric(&lp, &env, 0, 1) {
                None => declined += 1,
                Some((decision, cert)) => {
                    certified += 1;
                    let bad = match (&truth, &decision) {
                        (Some(d), Decision::Exact(v)) => v != &vec![*d],
                        (None, Decision::None) => false,
                        _ => true,
                    };
                    if bad {
                        wrong += 1;
                        if examples.len() < 10 {
                            examples.push(format!(
                                "N={} P={} c={} oq={} truth={:?} decision={:?}",
                                n_val, p_val, c, oq, truth, decision
                            ));
                        }
                        continue;
                    }
                    // Self-consistency: the tree walked at its own env selects
                    // exactly the terminal the witness names, and a dependence
                    // leaf lands on a real in-range collision.
                    let cert = cert.expect("certified case carries a certificate");
                    let tree = match &cert.claim {
                        Claim::Parametric(t) => t,
                        other => panic!("expected Parametric claim, got {other:?}"),
                    };
                    let idx = match &cert.witness {
                        Witness::ParametricSolution { env: we, leaf } => {
                            assert_eq!(*we, env, "witness env must match");
                            *leaf
                        }
                        other => panic!("expected ParametricSolution, got {other:?}"),
                    };
                    let t = tree
                        .root
                        .resolve_terminal(&env)
                        .expect("tree must resolve at its own env");
                    match (t, truth) {
                        (Terminal::Dependence(k, l), Some(d)) => {
                            assert_eq!(k, idx, "terminal index mismatch");
                            let i = l.i[0].eval(&env).unwrap();
                            let j = l.j[0].eval(&env).unwrap();
                            assert_eq!(j - i, d, "leaf distance != ground truth");
                            assert_eq!(l.distance[0].eval(&env), Some(d));
                            assert_eq!(c * i + p_val, c * j + oq, "not a collision");
                            assert!(
                                i >= 0 && i < n_val && j >= 0 && j < n_val,
                                "witness out of range: i={i} j={j} n={n_val}"
                            );
                        }
                        (Terminal::Absent(k), None) => {
                            assert_eq!(k, idx, "terminal index mismatch");
                        }
                        _ => {
                            wrong += 1;
                            if examples.len() < 10 {
                                examples.push(format!(
                                    "MISMATCH N={n_val} P={p_val} c={c} oq={oq} truth={truth:?}"
                                ));
                            }
                        }
                    }
                }
            }
        }
        println!("M1 tree fuzz: tested={tested} certified={certified} declined={declined} wrong={wrong}");
        for e in &examples {
            println!("  WRONG {e}");
        }
        assert_eq!(wrong, 0, "M1 parametric tree produced {wrong} wrong verdicts");
        assert!(certified > 100, "only {certified} certified");
    }
}
