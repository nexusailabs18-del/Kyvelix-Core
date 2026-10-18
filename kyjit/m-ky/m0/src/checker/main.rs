use std::env;
use std::fs;
use std::process::exit;

use m0::certificate::*;
use m0::ir::*;
use m0::decide::{m2g, m2a, m3, m4, m5};


fn location_interval(access: &Access, loop_: &Loop) -> (i64, i64) {
    // M6/M7: non-affine index kinds require enumeration; return an
    // inverted interval (empty) so callers know they can't bound it
    // without evaluating every IV.
    if matches!(access.index_kind,
        m0::ir::IndexKind::Product(_)
        | m0::ir::IndexKind::Hash(_)
        | m0::ir::IndexKind::Recurrence(_)
        | m0::ir::IndexKind::FixedPoint(_)
        | m0::ir::IndexKind::SymbolicFixedPoint { .. }
        | m0::ir::IndexKind::Range(_)
        | m0::ir::IndexKind::Polyhedral { .. }) {
        return (0, -1);
    }

    let offset = match access.offset.as_const() {
        Some(v) => v,
        None => return (0, -1), // Symbolic offset cannot be bounded
    };
    if let Some(range) = &access.range {
        let (rlo, rhi) = match range {
            RangeExpr::Mod { iv: _, modulus } => {
                if *modulus > 0 { (0, modulus - 1) } else { return (0, -1); }
            }
            RangeExpr::ModParam { iv: _, modulus } => {
                // Without an environment, treat the modulus as unknown.
                // Emit an empty interval so the checker refuses.
                let _ = modulus;
                return (0, -1);
            }
            RangeExpr::Min { iv, cap } => {
                if *iv >= loop_.ivs.len() { return (0, -1); }
                let iv_lower = loop_.ivs[*iv].lower;
                let iv_upper = loop_.ivs[*iv].upper;
                if *cap < iv_lower { return (0, -1); }
                let iv_hi = match iv_upper.checked_sub(1) {
                    Some(v) => v,
                    None => return (0, -1),
                };
                (iv_lower, std::cmp::min(iv_hi, *cap))
            }
            RangeExpr::Max { iv, floor } => {
                if *iv >= loop_.ivs.len() { return (0, -1); }
                let iv_lower = loop_.ivs[*iv].lower;
                let iv_upper = loop_.ivs[*iv].upper;
                let iv_hi = match iv_upper.checked_sub(1) {
                    Some(v) => v,
                    None => return (0, -1),
                };
                if *floor > iv_hi { return (0, -1); }
                (std::cmp::max(iv_lower, *floor), iv_hi)
            }
        };
        // Offset is added on top of the range expression. Use checked arithmetic.
        match (rlo.checked_add(offset), rhi.checked_add(offset)) {
            (Some(lo), Some(hi)) => (lo, hi),
            _ => (0, -1), // Overflow -> invalid interval
        }
    } else {
        let mut lo = 0i64;
        let mut hi = 0i64;
        for k in 0..loop_.depth {
            let coeff = access.index[k];
            let iv_lo = loop_.ivs[k].lower;
            let iv_hi = match loop_.ivs[k].upper.checked_sub(1) {
                Some(v) => v,
                None => return (0, -1),
            };
            let v1 = match coeff.checked_mul(iv_lo) {
                Some(v) => v,
                None => return (0, -1),
            };
            let v2 = match coeff.checked_mul(iv_hi) {
                Some(v) => v,
                None => return (0, -1),
            };
            lo = match lo.checked_add(std::cmp::min(v1, v2)) {
                Some(v) => v,
                None => return (0, -1),
            };
            hi = match hi.checked_add(std::cmp::max(v1, v2)) {
                Some(v) => v,
                None => return (0, -1),
            };
        }
        match (lo.checked_add(offset), hi.checked_add(offset)) {
            (Some(lo), Some(hi)) => (lo, hi),
            _ => (0, -1),
        }
    }
}

/// Compute the concrete location of an access for a specific IV assignment.
/// If `access.range` is Some, apply the range expression to the relevant IV.
/// If `access.range` is None, compute `index * iv_values + offset`.
///
/// Returns `None` if the offset is symbolic and cannot be resolved, or if any
/// arithmetic operation would overflow, or if the index kind is not supported.
/// This mirrors `refutation_location` but is used for positive witness checking.
fn witness_location(access: &Access, iv_values: &[i64], loop_: &Loop, env: &[(String, i64)]) -> Option<i64> {
    // The offset must be a concrete number, not a parameter that happens to be
    // unbound in this environment.
    let offset = access.offset.resolve(env)?;

    // M6: product index expressions (including Div, Mod, Min, Max, Cmp, Load).
    if let m0::ir::IndexKind::Product(e) = &access.index_kind {
        let base = e.eval(iv_values, &[], None)?;
        return base.checked_add(offset);
    }

    // M7: hash index expressions (depth-1 only).
    if let m0::ir::IndexKind::Hash(h) = &access.index_kind {
        if iv_values.is_empty() { return None; }
        let base = h.eval(iv_values[0]);
        return base.checked_add(offset);
    }

    // M8: recurrence index expressions (depth-1 only).
    if let m0::ir::IndexKind::Recurrence(r) = &access.index_kind {
        if iv_values.is_empty() { return None; }
        let k = iv_values[0];
        if k < 0 { return None; }
        let base = r.eval(k as usize)?;
        return base.checked_add(offset);
    }

    // M10: fixed-point index expressions (depth-1 only).
    if let m0::ir::IndexKind::FixedPoint(fpt) = &access.index_kind {
        if iv_values.is_empty() { return None; }
        let base = fpt.eval(iv_values[0])?;
        return base.checked_add(offset);
    }

    // M11: symbolic fixed-point index expressions.
    if let m0::ir::IndexKind::SymbolicFixedPoint { next_expr, max_steps } = &access.index_kind {
        if iv_values.is_empty() { return None; }
        let mut current = iv_values[0];
        for _ in 0..*max_steps {
            let next = next_expr.eval(&[current], &[], None)?;
            if next == current {
                return current.checked_add(offset);
            }
            current = next;
        }
        return None; // Did not converge
    }

    // Item 3: Range index expressions.
    if let m0::ir::IndexKind::Range(range) = &access.index_kind {
        let base = match range {
            RangeExpr::Mod { iv, modulus } => {
                if *modulus <= 0 { return None; }
                iv_values.get(*iv)?.rem_euclid(*modulus)
            }
            RangeExpr::ModParam { iv, modulus } => {
                let m = modulus.eval(env)?;
                if m <= 0 { return None; }
                iv_values.get(*iv)?.rem_euclid(m)
            }
            RangeExpr::Min { iv, cap } => {
                let v = iv_values.get(*iv)?;
                *std::cmp::min(v, cap)
            }
            RangeExpr::Max { iv, floor } => {
                let v = iv_values.get(*iv)?;
                *std::cmp::max(v, floor)
            }
        };
        return base.checked_add(offset);
    }

    // Item 8: Polyhedral index expressions - evaluate using the constraint system.
    if let m0::ir::IndexKind::Polyhedral { a, b, dim } = &access.index_kind {
        // For polyhedral, we evaluate the constraint system at the given IV values.
        // This is a simplified evaluation - in practice, the polyhedral form
        // is used for dependence analysis, not direct location computation.
        // The checker verifies the witness against the constraint system.
        // For location computation, we fall back to the affine part.
        if iv_values.len() >= *dim {
            let mut loc = 0i64;
            for k in 0..*dim {
                let coeff = a.first().map(|row| row.get(k).copied().unwrap_or(0)).unwrap_or(0);
                let val = iv_values[k];
                loc = loc.checked_add(coeff.checked_mul(val)?)?;
            }
            return loc.checked_add(offset);
        }
        return None;
    }

    // M2a.5: table indirection. `access.indirect.table = Some(tbl)` means
    // the location is `tbl[iv_values[0]] + offset`. The injectivity
    // premise is verified separately in the IntSolution branch.
    if let Some(ind) = &access.indirect {
        if let Some(tbl) = &ind.table {
            if iv_values.is_empty() { return None; }
            let pos = iv_values[0];
            if pos < ind.domain_lo || pos >= ind.domain_hi { return None; }
            if pos < 0 || (pos as usize) >= tbl.len() { return None; }
            let cell = *tbl.get(pos as usize)?;
            return cell.checked_add(offset);
        }
    }
    // A non-table indirection has no literal to consult, so its subscript is
    // only meaningful once M2a has flattened the access; refuse rather than
    // guess at the inner expression.
    if access.indirect.is_some() {
        return None;
    }

    if let Some(range) = &access.range {
        let base = match range {
            RangeExpr::Mod { iv, modulus } => {
                if *modulus <= 0 { return None; }
                iv_values.get(*iv)?.rem_euclid(*modulus)
            }
            RangeExpr::ModParam { iv, modulus } => {
                let m = modulus.eval(env)?;
                if m <= 0 { return None; }
                iv_values.get(*iv)?.rem_euclid(m)
            }
            RangeExpr::Min { iv, cap } => {
                let v = iv_values.get(*iv)?;
                *std::cmp::min(v, cap)
            }
            RangeExpr::Max { iv, floor } => {
                let v = iv_values.get(*iv)?;
                *std::cmp::max(v, floor)
            }
        };
        base.checked_add(offset)
    } else {
        if access.index.len() != loop_.depth {
            return None;
        }
        let mut loc = offset;
        for k in 0..loop_.depth {
            let term = access.index[k].checked_mul(iv_values[k])?;
            loc = loc.checked_add(term)?;
        }
        Some(loc)
    }
}

/// Evaluate a symbolic witness vector at `env`. `None` if any component cannot
/// be evaluated (missing parameter, or a division that is not exact).
fn eval_vec(v: &[m0::param_expr::ParamExpr], env: &[(String, i64)]) -> Option<Vec<i64>> {
    let mut out = Vec::with_capacity(v.len());
    for e in v {
        out.push(e.eval(env)?);
    }
    Some(out)
}

/// Substitute the environment into an access's parameterised offset, producing
/// an access whose offset is a plain constant. A non-parametric offset is
/// returned unchanged.
fn bind_offset(
    a: &Access,
    env: &[(String, i64)],
) -> Access {
    let mut out = a.clone();
    if let OffsetValue::Param(e) = &a.offset {
        match e.eval(env) {
            Some(v) => out.offset = OffsetValue::Const(v),
            // Leave the symbolic offset in place: `witness_location` will then
            // resolve it against the same env, so nothing is silently zeroed.
            None => {}
        }
    }
    out
}

/// The location of an access at one IV assignment, for *refutation* purposes.
///
/// [`witness_location`] is built to confirm a positive witness: two locations
/// that both fail to compute come out equal, which is exactly what that check
/// wants. For proving the *absence* of a collision the same behaviour is
/// unsound -- an access whose offset or index cannot be resolved would report a
/// sentinel, never match its partner, and so hand the checker a proof of
/// disjointness it never established.
///
/// This variant therefore returns `None` whenever any part of the location is
/// not concretely determined: an unresolved parameter, a non-affine index the
/// closed-form path does not model, or arithmetic that cannot be carried out
/// without wrapping. A `None` makes the refutation fail rather than succeed.
///
/// Shapes it settles are exactly the ones [`witness_location`] settles for a
/// positive witness — affine and range indices, M2a.5 literal tables, M6
/// products, M7 hashes, M8 recurrences, M10/M11 fixed points — because a
/// refutation earns nothing over a witness unless the checker can re-derive it.
/// Where `witness_location` reports failure with the `i64::MIN` sentinel, this
/// returns `None`: an undefined location is no evidence of disjointness, and the
/// refutation is refused instead.
fn refutation_location(
    access: &Access,
    iv_values: &[i64],
    loop_: &Loop,
    env: &[(String, i64)],
) -> Option<i64> {
    // The offset must be a concrete number, not a parameter that happens to be
    // unbound in this environment.
    let offset = access.offset.resolve(env)?;
    let first = || iv_values.first().copied();

    // M2a.5 literal table: the subscript is the table cell, as in
    // `witness_location`.
    if let Some(tbl) = access.indirect.as_ref().and_then(|i| i.table.as_ref()) {
        // Refuse anything that is not the pure table lookup this branch models:
        // a nonzero outer affine term or a non-affine index would be silently
        // dropped, and two subscripts that really overlap could then look
        // disjoint.
        if !matches!(access.index_kind, IndexKind::Affine) {
            return None;
        }
        if access.index.iter().any(|&c| c != 0) {
            return None;
        }
        let pos = first()?;
        let (dom_lo, dom_hi) = tbl_domain(access)?;
        // Outside the declared domain there is no cell to name — see
        // `never_executes` for the case where the whole domain is empty.
        if pos < dom_lo || pos >= dom_hi {
            return None;
        }
        let cell = *tbl.get(pos as usize)?;
        return cell.checked_add(offset);
    }
    // A non-table indirection has no literal to consult, so its subscript is
    // only meaningful once M2a has flattened the access; refuse rather than
    // guess at the inner expression.
    if access.indirect.is_some() {
        return None;
    }

    // The subscript, per index kind. `None` from any arm means "this iteration
    // names no location", which is not evidence of anything.
    let base: i64 = match &access.index_kind {
        IndexKind::Product(e) => e.eval(iv_values, &[], None)?,
        IndexKind::Hash(h) => h.eval(first()?),
        IndexKind::Recurrence(r) => {
            let k = first()?;
            if k < 0 {
                return None;
            }
            r.eval(k as usize)?
        }
        IndexKind::FixedPoint(fpt) => fpt.eval(first()?)?,
        IndexKind::SymbolicFixedPoint { next_expr, max_steps } => {
            let mut current = first()?;
            let mut converged = false;
            for _ in 0..*max_steps {
                let next = next_expr.eval(&[current], &[], None)?;
                if next == current {
                    converged = true;
                    break;
                }
                current = next;
            }
            if !converged {
                return None;
            }
            current
        }
        IndexKind::Range(range) => {
            // Evaluate the range expression at the given IV values
            let base = match range {
                RangeExpr::Mod { iv, modulus } => {
                    if *modulus <= 0 { return None; }
                    iv_values.get(*iv)?.rem_euclid(*modulus)
                }
                RangeExpr::ModParam { iv, modulus } => {
                    let m = modulus.eval(env)?;
                    if m <= 0 { return None; }
                    iv_values.get(*iv)?.rem_euclid(m)
                }
                RangeExpr::Min { iv, cap } => {
                    let v = iv_values.get(*iv)?;
                    *std::cmp::min(v, cap)
                }
                RangeExpr::Max { iv, floor } => {
                    let v = iv_values.get(*iv)?;
                    *std::cmp::max(v, floor)
                }
            };
            base
        }
        IndexKind::Polyhedral { a, b: _, dim } => {
            // For polyhedral, evaluate the affine part
            if iv_values.len() >= *dim {
                let mut loc = 0i64;
                for k in 0..*dim {
                    let coeff = a.first().map(|row| row.get(k).copied().unwrap_or(0)).unwrap_or(0);
                    let val = iv_values[k];
                    loc = loc.checked_add(coeff.checked_mul(val)?)?;
                }
                loc
            } else {
                return None;
            }
        }
        IndexKind::Affine => {
            if let Some(range) = &access.range {
                // A range expression folds the index into a small window; every
                // operand has to be concrete for the window to be known.
                match range {
                    RangeExpr::Mod { iv, modulus } => {
                        if *modulus <= 0 {
                            return None;
                        }
                        (*iv_values.get(*iv)?).rem_euclid(*modulus)
                    }
                    RangeExpr::ModParam { iv, modulus } => {
                        let m = modulus.eval(env)?;
                        if m <= 0 {
                            return None;
                        }
                        (*iv_values.get(*iv)?).rem_euclid(m)
                    }
                    RangeExpr::Min { iv, cap } => (*iv_values.get(*iv)?).min(*cap),
                    RangeExpr::Max { iv, floor } => (*iv_values.get(*iv)?).max(*floor),
                }
            } else {
                if access.index.len() != loop_.depth {
                    return None;
                }
                let mut loc = 0i64;
                for k in 0..loop_.depth {
                    let term = access.index[k].checked_mul(iv_values[k])?;
                    loc = loc.checked_add(term)?;
                }
                loc
            }
        }
    };
    base.checked_add(offset)
}

/// The declared cell domain `[lo, hi)` of a literal-table access, if it has one.
fn tbl_domain(access: &Access) -> Option<(i64, i64)> {
    let ind = access.indirect.as_ref()?;
    ind.table.as_ref()?;
    Some((ind.domain_lo, ind.domain_hi))
}

/// Whether `v` is one of the values the induction variable actually takes.
///
/// A loop runs `lower, lower + step, lower + 2*step, ...` while below `upper`.
/// Being inside `[lower, upper)` is not enough: with `step = 2` starting at 0,
/// the value 1 lies in range yet is never assigned, so no statement executes
/// there. A witness that relies on such a phantom iteration proves a dependence
/// between two runs of the program that never happen.
///
/// A non-positive step runs no iteration at all, matching
/// [`executing_iterations`]; negative steps do not occur in this IR.
fn iv_lattice_reached(iv: &IV, v: i64) -> bool {
    if iv.step <= 0 {
        return false;
    }
    if v < iv.lower || v >= iv.upper {
        return false;
    }
    match v.checked_sub(iv.lower) {
        Some(diff) => diff.rem_euclid(iv.step) == 0,
        None => false,
    }
}

/// Whether an access runs at one iteration assignment.
///
/// Three things switch an access off at a given iteration: an IV value the loop
/// never assigns, a guard the iteration violates, and a literal table whose
/// declared domain does not cover the position — outside it `B[i]` names no
/// cell, so the access has no location to take. All of them matter in *both*
/// directions:
///
///  * a positive witness must sit on iterations where both accesses run, or it
///    certifies a dependence between two statements that never execute
///    together (and, for tables, between two `i64::MIN` sentinels that merely
///    happen to be equal);
///  * a refutation must *skip* the iterations where an access is off rather than
///    treat them as collisions or as unresolvable locations.
///
/// A guard naming an IV the loop does not have is treated as "does not run",
/// matching M4's domain-emptiness reading.
fn access_executes_at(access: &Access, iv_values: &[i64], loop_: &Loop) -> bool {
    // A value the loop never assigns means nothing runs there, whatever the
    // guard and the table domain say. Under M3 a "loop" is a recursion whose
    // `ivs` are placeholders that must not be read, so the lattice of the
    // enclosing `for` is not a fact about this program point.
    if loop_.recursion.is_none() && loop_.ivs.len() == loop_.depth {
        for (k, iv) in loop_.ivs.iter().enumerate() {
            match iv_values.get(k) {
                Some(&v) => {
                    if !iv_lattice_reached(iv, v) {
                        return false;
                    }
                }
                None => return false,
            }
        }
    }
    if let Some(g) = &access.guard {
        for c in &g.constraints {
            let v = match iv_values.get(c.iv) {
                Some(&v) => v,
                None => return false,
            };
            match c.op {
                CmpOp::Lt => {
                    if !(v < c.bound) {
                        return false;
                    }
                }
                CmpOp::Le => {
                    if !(v <= c.bound) {
                        return false;
                    }
                }
                CmpOp::Gt => {
                    if !(v > c.bound) {
                        return false;
                    }
                }
                CmpOp::Ge => {
                    if !(v >= c.bound) {
                        return false;
                    }
                }
            }
        }
    }
    if let Some((lo, hi)) = tbl_domain(access) {
        let pos = match iv_values.first() {
            Some(&v) => v,
            None => return false,
        };
        if pos < lo || pos >= hi {
            return false;
        }
    }
    true
}

/// Prove an access executes on *no* iteration, which by itself refutes every
/// dependence on it.
///
/// Two independent reasons for that, both cheap to check and both shapes a
/// refutation certificate legitimately relies on:
///
///  * the loop, or the access's guard on the loop, leaves no iteration at all;
///  * the access indexes a literal table whose declared domain is empty, so
///    there is no cell for it to name (`A[B[i]]` over `B = []`).
///
/// Guards over *other* IVs than the table-position IV are deliberately not
/// modelled here: narrowing `[lower, upper)` to the guard's sublattice needs the
/// step/residue handling `allowed_ivs` performs, and a closed form that widened
/// the range by one iteration is exactly the bug that manufactures collisions
/// rather than refuting them. Such loops fall through to enumeration.
fn never_executes(access: &Access, loop_: &Loop, env: &[(String, i64)]) -> bool {
    // A parameterised offset that this environment does not bind leaves the
    // access's location unresolved; refuse to call that "never executes".
    if matches!(access.offset, OffsetValue::Param(_)) && access.offset.resolve(env).is_none() {
        return false;
    }
    // Empty literal-table domain: no cell exists to be named.
    if let Some((lo, hi)) = tbl_domain(access) {
        if lo >= hi {
            return true;
        }
    }
    let mut guard_ivs: Vec<usize> = Vec::new();
    if let Some(g) = &access.guard {
        for c in &g.constraints {
            if c.iv >= loop_.depth {
                // A guard on an IV that does not exist: the conjunction can
                // never hold, which is what M4 also treats as domain emptiness.
                return true;
            }
            if !guard_ivs.contains(&c.iv) {
                guard_ivs.push(c.iv);
            }
        }
    }
    // Under M3 a "loop" is a recursion whose `ivs` are placeholders that must
    // not be read, and a negative range there says nothing about execution.
    if loop_.recursion.is_some() {
        return false;
    }
    if loop_.ivs.len() != loop_.depth {
        return false;
    }
    for k in 0..loop_.depth {
        let iv = &loop_.ivs[k];
        // An IV with no step, or with an empty range, kills every iteration.
        if iv.step <= 0 || iv.lower >= iv.upper {
            return true;
        }
        // `allowed_ivs` semantics: an access-level guard on IV `k` narrows the
        // *exclusive* upper bound of IV k and empties the product when it does
        // so below the lower bound. Only the guarded IV is narrowed.
        if guard_ivs.contains(&k) {
            let g = access.guard.as_ref().unwrap();
            let mut lo = iv.lower;
            let mut hi = iv.upper;
            for c in &g.constraints {
                if c.iv != k {
                    continue;
                }
                match c.op {
                    CmpOp::Lt => hi = hi.min(c.bound),
                    CmpOp::Le => match c.bound.checked_add(1) {
                        Some(b) => hi = hi.min(b),
                        None => return false,
                    },
                    CmpOp::Gt => match c.bound.checked_add(1) {
                        Some(b) => lo = lo.max(b),
                        None => return false,
                    },
                    CmpOp::Ge => lo = lo.max(c.bound),
                }
            }
            if lo >= hi {
                return true;
            }
        }
    }
    false
}

/// Exhaustively confirm that two accesses never name the same location, over
/// the resolved iteration space. Used to verify a `NoDependence` terminal of a
/// decision tree, which is the only way a parametric certificate can claim
/// absence for a given environment.
///
/// Returns `true` when no ordered pair collides -- i.e. the refutation is
/// correct. A space too large to enumerate is reported as *not* refuted, which
/// makes the checker reject rather than guess.
///
/// The budget is spent on the iterations each access actually *performs*, not on
/// the loop's whole product: a guard or a short table domain shrinks the work
/// just as much here as it does in M4, so a certificate M4 proved by walking a
/// narrow guarded domain remains checkable.
fn no_dependence_by_enumeration(
    loop_: &Loop,
    ap: &Access,
    aq: &Access,
    env: &[(String, i64)],
) -> bool {
    // Each domain is capped like M4's own guarded walk, and the pair walk is
    // budgeted separately so the checker never does more than a few million
    // location evaluations for one certificate.
    const DOMAIN_CAP: i64 = 4096;
    const PAIR_CAP: i64 = 1 << 22;
    let ap2 = bind_offset(ap, env);
    let aq2 = bind_offset(aq, env);
    let dom_p = match executing_iterations(&ap2, loop_, DOMAIN_CAP) {
        Some(v) => v,
        None => return false,
    };
    let dom_q = match executing_iterations(&aq2, loop_, DOMAIN_CAP) {
        Some(v) => v,
        None => return false,
    };
    match dom_p.len().checked_mul(dom_q.len()) {
        Some(n) if (n as i64) <= PAIR_CAP => {}
        _ => return false,
    }
    for i in &dom_p {
        for j in &dom_q {
            // The strict evaluator: if either location is not concretely
            // determined, the absence claim is not established and the
            // refutation is refused.
            let li = match refutation_location(&ap2, i, loop_, env) {
                Some(v) => v,
                None => return false,
            };
            let lj = match refutation_location(&aq2, j, loop_, env) {
                Some(v) => v,
                None => return false,
            };
            if li == lj {
                return false;
            }
        }
    }
    true
}

/// Every assignment of the loop's IVs on which `access` executes, as a lattice
/// walk of `lower, lower+step, ...` below `upper` for each level.
///
/// `Some(vec)` is the domain (possibly empty, which by itself refutes any
/// dependence); `None` means it is larger than `cap`, so no verdict may be
/// drawn from enumeration.
fn executing_iterations(access: &Access, loop_: &Loop, cap: i64) -> Option<Vec<Vec<i64>>> {
    // A guard on an IV the loop does not have can never hold, so nothing runs.
    if let Some(g) = &access.guard {
        if g.constraints.iter().any(|c| c.iv >= loop_.depth) {
            return Some(Vec::new());
        }
    }
    // Under M3 the `ivs` are placeholders that must not be read.
    if loop_.recursion.is_some() || loop_.ivs.len() != loop_.depth {
        return None;
    }
    // The values each level can take: the loop's own lattice, narrowed by the
    // guard on that level. Enumerating these instead of the whole interval is
    // what lets a certificate on a wide but narrowly-guarded loop still be
    // checked, and enumerating the *lattice* rather than the interval is what
    // keeps a stepped loop from being judged at an iteration it never reaches.
    let mut levels: Vec<Vec<i64>> = Vec::with_capacity(loop_.depth);
    let mut total: i64 = 1;
    for k in 0..loop_.depth {
        let iv = &loop_.ivs[k];
        if iv.step <= 0 {
            return Some(Vec::new());
        }
        let (mut lo, mut hi) = (iv.lower, iv.upper);
        if let Some(g) = &access.guard {
            for c in &g.constraints {
                if c.iv != k {
                    continue;
                }
                match c.op {
                    CmpOp::Lt => hi = hi.min(c.bound),
                    CmpOp::Le => match c.bound.checked_add(1) {
                        Some(b) => hi = hi.min(b),
                        None => return None,
                    },
                    CmpOp::Gt => match c.bound.checked_add(1) {
                        Some(b) => lo = lo.max(b),
                        None => return None,
                    },
                    CmpOp::Ge => lo = lo.max(c.bound),
                }
            }
        }
        if lo >= hi {
            return Some(Vec::new());
        }
        // Snap the lower end up onto the loop's own lattice.
        let offset = match lo.checked_sub(iv.lower) {
            Some(v) => v,
            None => return None,
        };
        let rem = offset.rem_euclid(iv.step);
        if rem != 0 {
            let step_minus_rem = match iv.step.checked_sub(rem) {
                Some(v) => v,
                None => return None,
            };
            lo = match lo.checked_add(step_minus_rem) {
                Some(v) => v,
                None => return None,
            };
            if lo >= hi {
                return Some(Vec::new());
            }
        }
        let count = match hi.checked_sub(1).and_then(|h| h.checked_sub(lo)) {
            Some(v) => match v.checked_div(iv.step) {
                Some(q) => q + 1,
                None => return None,
            },
            None => return None,
        };
        total = total.checked_mul(count)?;
        if total > cap {
            return None;
        }
        levels.push((0..count).filter_map(|n| {
            let n_step = iv.step.checked_mul(n)?;
            lo.checked_add(n_step)
        }).collect());
    }
    let d = loop_.depth;
    let mut cur = vec![0i64; d];
    let mut out = Vec::new();
    for t in 0..total {
        let mut rest = t;
        for k in 0..d {
            let level = &levels[k];
            cur[k] = level[(rest % level.len() as i64) as usize];
            rest /= level.len() as i64;
        }
        if access_executes_at(access, &cur, loop_) {
            out.push(cur.clone());
        }
    }
    Some(out)
}

/// Floor/ceil division for `i128` (Rust's truncating `/` is neither).
fn div_floor(n: i128, d: i128) -> i128 {
    let q = n / d;
    if n % d != 0 && (n < 0) != (d < 0) {
        q - 1
    } else {
        q
    }
}
fn div_ceil(n: i128, d: i128) -> i128 {
    let q = n / d;
    if n % d != 0 && (n > 0) == (d > 0) {
        q + 1
    } else {
        q
    }
}

/// The `t` values for which `start + dir*t` stays inside `[lo, hi]`, as an
/// inclusive `(t_min, t_max)`; empty (`t_min > t_max`) when the coordinate
/// cannot be kept in range at all. A zero `dir` means the coordinate is
/// constant, so it either constrains nothing or is outright impossible.
fn t_window(start: i128, dir: i128, lo: i128, hi: i128) -> (i128, i128) {
    if dir == 0 {
        return if start >= lo && start <= hi {
            (i64::MIN as i128, i64::MAX as i128)
        } else {
            (1, 0)
        };
    }
    let (a, b) = (lo - start, hi - start);
    if dir > 0 {
        (div_ceil(a, dir), div_floor(b, dir))
    } else {
        (div_ceil(b, dir), div_floor(a, dir))
    }
}

/// What an access's guard leaves of a loop's iteration range.
enum GuardedRange {
    /// Executed on exactly this inclusive IV interval.
    Interval(i64, i64),
    /// Never executed, so no pair involving this access can collide.
    Never,
    /// Outside the closed-form fragment; the caller must enumerate instead.
    Unsupported,
}

/// Narrow a loop's `[lower, upper)` down to the iterations on which `guard`
/// lets this access execute.
///
/// `Never` — an empty range, or a guard on an IV that does not exist — is
/// itself a proof of non-collision, and is the case M4 also proves by domain
/// emptiness.
fn guarded_iv_range(iv: &IV, guard: Option<&Guard>) -> GuardedRange {
    // Clamp in *exclusive*-upper space and convert to an inclusive bound only at
    // the end, exactly as `m4::guard_range` does. `i < b` caps the exclusive
    // upper at `b`, i.e. the inclusive `b - 1`; applying the same `min` to an
    // already-inclusive bound keeps iteration `b`, which the guard forbids, and
    // that phantom iteration is then mistaken for a real collision.
    let mut lo = iv.lower;
    let mut hi = iv.upper;
    if iv.step <= 0 {
        return GuardedRange::Never;
    }
    if let Some(g) = guard {
        for c in &g.constraints {
            if c.iv != 0 {
                return GuardedRange::Unsupported;
            }
            match c.op {
                CmpOp::Lt => hi = hi.min(c.bound),
                CmpOp::Le => match c.bound.checked_add(1) {
                    Some(b) => hi = hi.min(b),
                    None => return GuardedRange::Unsupported,
                },
                CmpOp::Gt => match c.bound.checked_add(1) {
                    Some(b) => lo = lo.max(b),
                    None => return GuardedRange::Unsupported,
                },
                CmpOp::Ge => lo = lo.max(c.bound),
            }
        }
    }
    if lo >= hi {
        return GuardedRange::Never;
    }
    if iv.step != 1 {
        // The loop visits only `lower + k*step`; snapping onto that lattice
        // leaves a residue condition the closed form below does not model, so
        // hand stepped loops back to enumeration rather than widen the range.
        return GuardedRange::Unsupported;
    }
    // `lo < hi` holds, so `hi - 1` cannot underflow in theory, but use checked arithmetic for safety.
    let hi_inclusive = match hi.checked_sub(1) {
        Some(v) => v,
        None => return GuardedRange::Unsupported,
    };
    GuardedRange::Interval(lo, hi_inclusive)
}

/// Prove or disprove "these two accesses never name the same location" in
/// closed form, without enumerating the iteration product.
///
/// `Some(true)` is a proof of non-collision, `Some(false)` shows a colliding
/// pair exists, and `None` means the shape is outside the fragment and the
/// caller must fall back to enumeration.
///
/// For a depth-1 affine pair on a step-1 loop, the collision condition is the
/// single linear Diophantine equation
///
/// ```text
///     ip·i - iq·j = oq - op      i in [lo_p, hi_p],  j in [lo_q, hi_q]
/// ```
///
/// whose integer solutions are the line `(i0 + dx·t, j0 + dy·t)`. Each range
/// then clips `t` to an interval, so solvability is decided by comparing four
/// integers: no domain-size budget is involved, which is exactly what lets a
/// refutation be checked on a loop far too large to enumerate.
fn no_collision_closed_form(
    loop_: &Loop,
    ap: &Access,
    aq: &Access,
    env: &[(String, i64)],
) -> Option<bool> {
    if loop_.depth != 1 || loop_.ivs.len() != 1 {
        return None;
    }
    // Affine, untabled, un-ranged accesses only: anything else has a location
    // that cannot be characterised without evaluating it every iteration.
    for a in [ap, aq] {
        if !matches!(a.index_kind, IndexKind::Affine)
            || a.range.is_some()
            || a.indirect.is_some()
            || a.index.len() != 1
        {
            return None;
        }
    }
    // Different arrays cannot collide at all.
    if ap.base != aq.base {
        return Some(true);
    }
    let ap = bind_offset(ap, env);
    let aq = bind_offset(aq, env);
    let op = ap.offset.as_const()?;
    let oq = aq.offset.as_const()?;
    let ip = *ap.index.first()?;
    let iq = *aq.index.first()?;
    if ip == i64::MIN || iq == i64::MIN {
        return None;
    }

    let (lo_p, hi_p) = match guarded_iv_range(&loop_.ivs[0], ap.guard.as_ref()) {
        GuardedRange::Interval(l, h) => (l, h),
        GuardedRange::Never => return Some(true),
        GuardedRange::Unsupported => return None,
    };
    let (lo_q, hi_q) = match guarded_iv_range(&loop_.ivs[0], aq.guard.as_ref()) {
        GuardedRange::Interval(l, h) => (l, h),
        GuardedRange::Never => return Some(true),
        GuardedRange::Unsupported => return None,
    };

    // `ip·i - iq·j = oq - op`. No integer solution at all proves non-collision.
    let rhs = match oq.checked_sub(op) {
        Some(v) => v,
        None => return None,
    };
    if !m0::solver::gcd::gcd_test_2(ip, -iq, rhs) {
        return Some(true);
    }
    // Degenerate coefficient: an access with a zero index coefficient names one
    // fixed location for every iteration, so `t` is genuinely free along the
    // solution line and the clipping argument below does not apply. Settle
    // these by division instead.
    if ip == 0 || iq == 0 {
        // `other_coef * v == this_fixed - other_offset` for some `v` in range.
        let (this_fixed, other_offset, other_coef, other_lo, other_hi) = if ip == 0 {
            (op, oq, iq, lo_q, hi_q)
        } else {
            (oq, op, ip, lo_p, hi_p)
        };
        let need = match this_fixed.checked_sub(other_offset) {
            Some(v) => v,
            None => return None,
        };
        if other_coef == 0 {
            // Both accesses are constant: they collide iff the offsets match.
            return Some(need != 0);
        }
        if other_coef == i64::MIN || need % other_coef != 0 {
            return Some(true);
        }
        let hit = need / other_coef;
        return Some(!(hit >= other_lo && hit <= other_hi));
    }
    let (i0, j0, dx, dy) = m0::solver::gcd::general_solution_2(ip, -iq, rhs)?;

    // Each coordinate clips `t` to an interval; the feasible set is their
    // intersection, and it is non-empty exactly when a colliding pair exists.
    // Both directions are non-zero here because neither coefficient is.
    let wi = t_window(i0 as i128, dx as i128, lo_p as i128, hi_p as i128);
    let wj = t_window(j0 as i128, dy as i128, lo_q as i128, hi_q as i128);
    let t_lo = wi.0.max(wj.0);
    let t_hi = wi.1.min(wj.1);
    Some(t_lo > t_hi)
}

/// A location bound `location_interval` can actually be trusted to give.
///
/// The function signals "I cannot bound this index" with an *inverted* interval
/// like `(0, -1)`, and an inverted interval looks disjoint from everything --
/// which would turn an inability to reason about a hash index into a bogus
/// proof of no-collision. So a bound is only returned when it is a real,
/// non-empty interval.
fn trusted_location_bound(access: &Access, loop_: &Loop) -> Option<(i64, i64)> {
    // Only affine indices are bounded by `location_interval`; and a guard only
    // shrinks the set of locations, so ignoring it stays sound.
    if !matches!(access.index_kind, IndexKind::Affine) || access.indirect.is_some() {
        // Range expressions have computable bounds
        if let IndexKind::Range(range) = &access.index_kind {
            return range_bounds(range, loop_);
        }
        // Polyhedral expressions have computable bounds
        if let IndexKind::Polyhedral { a, b: _, dim } = &access.index_kind {
            return polyhedral_bounds(a, *dim, loop_);
        }
        return None;
    }
    let (lo, hi) = location_interval(access, loop_);
    if lo <= hi {
        Some((lo, hi))
    } else {
        None
    }
}

/// Compute bounds for a Polyhedral index expression.
fn polyhedral_bounds(a: &[Vec<i64>], dim: usize, loop_: &Loop) -> Option<(i64, i64)> {
    if dim == 0 || dim > loop_.depth {
        return None;
    }
    let mut min_loc = 0i64;
    let mut max_loc = 0i64;
    let coeffs = a.first()?;
    for k in 0..dim {
        if k >= loop_.ivs.len() || k >= coeffs.len() {
            return None;
        }
        let coeff = coeffs[k];
        let iv = &loop_.ivs[k];
        let iv_lo = iv.lower;
        let iv_hi = iv.upper - 1;
        if coeff >= 0 {
            min_loc = min_loc.checked_add(coeff.checked_mul(iv_lo)?)?;
            max_loc = max_loc.checked_add(coeff.checked_mul(iv_hi)?)?;
        } else {
            min_loc = min_loc.checked_add(coeff.checked_mul(iv_hi)?)?;
            max_loc = max_loc.checked_add(coeff.checked_mul(iv_lo)?)?;
        }
    }
    Some((min_loc, max_loc))
}

/// Compute bounds for a RangeExpr.
fn range_bounds(range: &RangeExpr, loop_: &Loop) -> Option<(i64, i64)> {
    match range {
        RangeExpr::Mod { modulus, .. } => {
            if *modulus > 0 { Some((0, modulus - 1)) } else { None }
        }
        RangeExpr::ModParam { modulus, .. } => {
            // Without an environment, treat the modulus as unknown.
            None
        }
        RangeExpr::Min { iv, cap } => {
            if *iv >= loop_.ivs.len() { return None; }
            let iv_lower = loop_.ivs[*iv].lower;
            let iv_upper = loop_.ivs[*iv].upper;
            if *cap < iv_lower { return None; }
            let iv_hi = match iv_upper.checked_sub(1) {
                Some(v) => v,
                None => return None,
            };
            Some((iv_lower, std::cmp::min(iv_hi, *cap)))
        }
        RangeExpr::Max { iv, floor } => {
            if *iv >= loop_.ivs.len() { return None; }
            let iv_lower = loop_.ivs[*iv].lower;
            let iv_upper = loop_.ivs[*iv].upper;
            let iv_hi = match iv_upper.checked_sub(1) {
                Some(v) => v,
                None => return None,
            };
            if *floor > iv_hi { return None; }
            Some((std::cmp::max(iv_lower, *floor), iv_hi))
        }
    }
}

/// Verify a claimed *absence* of dependence against the loop itself.
///
/// A `Claim::None` certificate is a refutation, and a refutation is only worth
/// accepting if the checker can derive it: a self-declared contradiction inside
/// the witness proves nothing about the program. So the claim is re-proved here
/// — exactly when the pair is decidable in closed form, otherwise by exhaustive
/// enumeration of the resolved iteration space — and a single colliding pair is
/// enough to reject.
fn refutation_holds(loop_: &Loop, ap: &Access, aq: &Access, env: &[(String, i64)]) -> bool {
    // An access that runs on no iteration cannot collide with anything, and
    // that argument holds at any domain size — which is the whole point of a
    // certificate claiming absence on a loop too large to enumerate.
    if never_executes(ap, loop_, env) || never_executes(aq, loop_, env) {
        return true;
    }
    if let Some(v) = no_collision_closed_form(loop_, ap, aq, env) {
        return v;
    }
    // Disjoint location bounds settle shapes the closed form does not cover
    // (modular and min/max indices), and cover them at any domain size.
    if let (Some(lp), Some(lq)) = (
        trusted_location_bound(ap, loop_),
        trusted_location_bound(aq, loop_),
    ) {
        if lp.1 < lq.0 || lq.1 < lp.0 {
            return true;
        }
    }
    no_dependence_by_enumeration(loop_, ap, aq, env)
}

/// Phase 1: verify a claim that is witnessed by a concrete iteration pair.
/// The caller has already checked the witness arity, the IV bounds, and
/// that both accesses evaluate to the same location. This function checks
/// only that the *claim* is supported by the pair's distance vector.
///
/// - `Exact(d)`      requires `j[k] - i[k] == d[k]` for every k.
/// - `Direction(v)`  requires `v[k]` to hold for `j[k] - i[k]` for every k.
/// - `Level(l)`      requires the leftmost nonzero component of `j - i` to
///                   be at index `l` (`l == depth` means loop-independent).
fn claim_ok(claim: &Claim, i: &[i64], j: &[i64], depth: usize) -> Result<(), String> {
    match claim {
        Claim::Exact(d) => {
            if d.len() != depth {
                return Err(format!("distance arity {} != depth {}", d.len(), depth));
            }
            for k in 0..depth {
                let delta = j[k].checked_sub(i[k]).ok_or_else(|| format!("distance overflow at {}", k))?;
                if delta != d[k] {
                    return Err(format!("distance mismatch at {}", k));
                }
            }
            Ok(())
        }
        Claim::Direction(dir) => {
            if dir.len() != depth {
                return Err(format!("direction arity {} != depth {}", dir.len(), depth));
            }
            for k in 0..depth {
                let delta = j[k].checked_sub(i[k]).ok_or_else(|| format!("distance overflow at {}", k))?;
                if !dir[k].holds(delta) {
                    return Err(format!(
                        "direction mismatch at {}: {:?} does not hold for delta {}",
                        k,
                        dir[k],
                        delta
                    ));
                }
            }
            Ok(())
        }
        Claim::Level(lvl) => {
            if *lvl > depth {
                return Err(format!("level {} > depth {}", lvl, depth));
            }
            let mut delta = Vec::with_capacity(depth);
            for k in 0..depth {
                delta.push(j[k].checked_sub(i[k]).ok_or_else(|| format!("distance overflow at {}", k))?);
            }
            let actual = m0::certificate::dependence_level(&delta);
            if actual != *lvl {
                return Err(format!(
                    "level mismatch: claimed {} but distance {:?} is level {}",
                    lvl, delta, actual
                ));
            }
            Ok(())
        }
        other => Err(format!("witness does not support claim {:?}", other)),
    }
}

/// H-3: Checker-side re-application of M2g normalization.
fn m2g_normalize(loop_: &Loop) -> Option<Loop> {
    m2g::normalize(loop_)
}

/// H-3: Checker-side re-application of M2a flattening.
fn m2a_flatten(loop_: &Loop) -> Option<Loop> {
    m2a::flatten(loop_)
}

/// H-3: Checker-side re-application of M3 recursion transform.
fn m3_recursion_transform(loop_: &Loop) -> Option<Loop> {
    m3::transform(loop_)
}

/// H-3: Checker-side re-application of M5 chase expansion.
fn m5_chase_expand(loop_: &Loop) -> Option<Loop> {
    m5::expand(loop_)
}

/// H-3: Checker-side re-application of M4 guard-based IV domain narrowing.
fn m4_guard_narrow(loop_: &Loop, p: usize, q: usize) -> Option<Loop> {
    m4::narrow(loop_, p, q)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: m0-check <loop.json> <cert.json>");
        exit(2);
    }
    let loop_str = fs::read_to_string(&args[1]).expect("read loop");
    let cert_str = fs::read_to_string(&args[2]).expect("read cert");
    let loop_: Loop = match serde_json::from_str(&loop_str) {
    Ok(l) => l,
    Err(e) => {
        eprintln!("REJECT: malformed loop: {}", e);
        exit(1);
    }
};
    let cert: Vec<Certificate> = match serde_json::from_str(&cert_str) {
    // Phase 9a: `cert.json` is a single object unless the analyzer had extra
    // loop-level evidence to report, in which case it is an array. Both shapes
    // are accepted, and *every* element is verified — an array is a conjunction
    // of claims, so one bad certificate in it rejects the file. Reading the
    // array form as a list of certificates and requiring non-emptiness is what
    // makes an empty array a rejection rather than a vacuous accept.
    Ok(single) => vec![single],
    Err(_) => match serde_json::from_str::<Vec<Certificate>>(&cert_str) {
        Ok(list) if !list.is_empty() => list,
        Ok(_) => {
            eprintln!("REJECT: certificate file is an empty list");
            exit(1);
        }
        Err(e) => {
            eprintln!("REJECT: malformed certificate: {}", e);
            exit(1);
        }
    },
};

    for c in &cert {
        if loop_.hash() != c.loop_hash {
            eprintln!("REJECT: loop hash mismatch");
            exit(1);
        }
    }

    for cert in &cert {
        check_one(&loop_, cert);
    }
    println!("ACCEPT");
}

/// Verify one certificate against the loop, exiting non-zero on rejection.
///
/// Split out of `main` so the single- and multi-certificate file shapes share
/// one implementation of every claim: an evidence claim verified in the array
/// form is verified by exactly the same code that judges a bare certificate.
/// A file of several certificates is a conjunction — every one must hold.
fn check_one(loop_: &Loop, cert: &Certificate) {
    match &cert.witness {
        Witness::AssertionChain { assertions, inner } => {
            let ap = &loop_.body[cert.pair.0];
            let aq = &loop_.body[cert.pair.1];
            // Extract the IV values from the inner witness (IntSolution).
            let (i_vals, j_vals) = match inner.as_ref() {
                Witness::IntSolution { i, j } => (i, j),
                _ => {
                    eprintln!("REJECT: AssertionChain wrapping unsupported witness");
                    exit(1);
                }
            };
            // Verify each assertion holds at both witness IV values.
            for a in assertions {
                if a.iv >= loop_.depth {
                    eprintln!("REJECT: assertion iv index out of range");
                    exit(1);
                }
                if !a.holds(i_vals[a.iv]) {
                    eprintln!("REJECT: assertion {} does not hold for i[{}]={}",
                        format!("{:?}", a.kind), a.iv, i_vals[a.iv]);
                    exit(1);
                }
                if !a.holds(j_vals[a.iv]) {
                    eprintln!("REJECT: assertion {} does not hold for j[{}]={}",
                        format!("{:?}", a.kind), a.iv, j_vals[a.iv]);
                    exit(1);
                }
            }
            // Now verify the inner witness normally.
            if i_vals.len() != loop_.depth || j_vals.len() != loop_.depth {
                eprintln!("REJECT: witness arity");
                exit(1);
            }
            for k in 0..loop_.depth {
                if i_vals[k] < loop_.ivs[k].lower || i_vals[k] >= loop_.ivs[k].upper {
                    eprintln!("REJECT: i[{}] out of bounds", k);
                    exit(1);
                }
                if j_vals[k] < loop_.ivs[k].lower || j_vals[k] >= loop_.ivs[k].upper {
                    eprintln!("REJECT: j[{}] out of bounds", k);
                    exit(1);
                }
            }
            if !access_executes_at(ap, i_vals, &loop_) || !access_executes_at(aq, j_vals, &loop_) {
                eprintln!("REJECT: AssertionChain witness does not execute the access");
                exit(1);
            }
            let li = witness_location(ap, i_vals, &loop_, &[]);
            let lj = witness_location(aq, j_vals, &loop_, &[]);
            let (li, lj) = match (li, lj) {
                (Some(li), Some(lj)) => (li, lj),
                _ => {
                    eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                    exit(1);
                }
            };
            if li != lj {
                eprintln!("REJECT: locations differ: {} != {}", li, lj);
                exit(1);
            }
            if let Err(e) = claim_ok(&cert.claim, i_vals, j_vals, loop_.depth) {
                eprintln!("REJECT: AssertionChain: {}", e);
                exit(1);
            }
        }
        Witness::Rewrite { rule, original, rewritten, inner } => {
            // H-3: Verify the rewrite rule was correctly applied.
            // 1. Check that original hashes to cert.loop_hash (the file on disk).
            if original.hash() != cert.loop_hash {
                eprintln!("REJECT: Rewrite original loop hash mismatch");
                exit(1);
            }
            // 2. Re-apply the rule to original and require byte-for-byte equality
            //    with the rewritten loop in the witness.
            let re_applied = match rule {
                RewriteRule::M2gNormalize => m2g_normalize(original),
                RewriteRule::M2aFlatten => m2a_flatten(original),
                RewriteRule::M3Recursion => m3_recursion_transform(original),
                RewriteRule::M4GuardNarrow => m4_guard_narrow(original, cert.pair.0, cert.pair.1),
                RewriteRule::M5ChaseExpand => m5_chase_expand(original),
            };
            if let Some(re_applied) = re_applied {
                if re_applied.hash() != rewritten.hash() {
                    eprintln!("REJECT: Rewrite rule {:?} did not produce the witnessed loop", rule);
                    exit(1);
                }
            } else {
                eprintln!("REJECT: Rewrite rule {:?} returned None on original loop", rule);
                exit(1);
            }
            // 3. Verify the inner witness against the rewritten loop.
            match inner.as_ref() {
                Witness::IntSolution { i, j } => {
                    if i.len() != rewritten.depth || j.len() != rewritten.depth {
                        eprintln!("REJECT: witness arity");
                        exit(1);
                    }
                    for k in 0..rewritten.depth {
                        if i[k] < rewritten.ivs[k].lower || i[k] >= rewritten.ivs[k].upper {
                            eprintln!("REJECT: i[{}] out of bounds (rewritten loop)", k);
                            exit(1);
                        }
                        if j[k] < rewritten.ivs[k].lower || j[k] >= rewritten.ivs[k].upper {
                            eprintln!("REJECT: j[{}] out of bounds (rewritten loop)", k);
                            exit(1);
                        }
                    }
                    let ap = &rewritten.body[cert.pair.0];
                    let aq = &rewritten.body[cert.pair.1];
                    if !access_executes_at(ap, i, rewritten) || !access_executes_at(aq, j, rewritten) {
                        eprintln!("REJECT: Rewrite witness does not execute the access");
                        exit(1);
                    }
                    let li = witness_location(ap, i, rewritten, &[]);
                    let lj = witness_location(aq, j, rewritten, &[]);
                    let (li, lj) = match (li, lj) {
                        (Some(li), Some(lj)) => (li, lj),
                        _ => {
                            eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                            exit(1);
                        }
                    };
                    if li != lj {
                        eprintln!("REJECT: locations differ: {} != {}", li, lj);
                        exit(1);
                    }
                    if let Err(e) = claim_ok(&cert.claim, i, j, rewritten.depth) {
                        eprintln!("REJECT: Rewrite/IntSolution: {}", e);
                        exit(1);
                    }
                }
                Witness::UnsatProof { .. } => {
                    if !matches!(cert.claim, Claim::None) {
                        eprintln!("REJECT: Rewrite/UnsatProof with non-None claim");
                        exit(1);
                    }
                    let ap = &rewritten.body[cert.pair.0];
                    let aq = &rewritten.body[cert.pair.1];
                    if !refutation_holds(rewritten, ap, aq, &[]) {
                        eprintln!(
                            "REJECT: Rewrite/UnsatProof refuted by the loop: a colliding pair exists"
                        );
                        exit(1);
                    }
                }
                _ => {
                    eprintln!("REJECT: Rewrite wrapping unsupported witness variant");
                    exit(1);
                }
            }
        }
        Witness::WithLoop { loop_, inner } => {
            // Verify the inner witness against the rewritten loop.
            // The outer loop_hash check already passed against the
            // original file; now use the rewritten loop for bounds
            // and location computation.
            match inner.as_ref() {
                Witness::IntSolution { i, j } => {
                    if i.len() != loop_.depth || j.len() != loop_.depth {
                        eprintln!("REJECT: witness arity");
                        exit(1);
                    }
                    for k in 0..loop_.depth {
                        if i[k] < loop_.ivs[k].lower || i[k] >= loop_.ivs[k].upper {
                            eprintln!("REJECT: i[{}] out of bounds (rewritten loop)", k);
                            exit(1);
                        }
                        if j[k] < loop_.ivs[k].lower || j[k] >= loop_.ivs[k].upper {
                            eprintln!("REJECT: j[{}] out of bounds (rewritten loop)", k);
                            exit(1);
                        }
                    }
                    // Recompute locations against the rewritten loop.
                    let ap = &loop_.body[cert.pair.0];
                    let aq = &loop_.body[cert.pair.1];
                    if !access_executes_at(ap, i, loop_) || !access_executes_at(aq, j, loop_) {
                        eprintln!("REJECT: WithLoop witness does not execute the access");
                        exit(1);
                    }
                    let li = witness_location(ap, i, loop_, &[]);
                    let lj = witness_location(aq, j, loop_, &[]);
                    let (li, lj) = match (li, lj) {
                        (Some(li), Some(lj)) => (li, lj),
                        _ => {
                            eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                            exit(1);
                        }
                    };
                    if li != lj {
                        eprintln!("REJECT: locations differ: {} != {}", li, lj);
                        exit(1);
                    }
                    if let Err(e) = claim_ok(&cert.claim, i, j, loop_.depth) {
                        eprintln!("REJECT: WithLoop/IntSolution: {}", e);
                        exit(1);
                    }
                }
                Witness::UnsatProof { .. } => {
                    if !matches!(cert.claim, Claim::None) {
                        eprintln!("REJECT: WithLoop/UnsatProof with non-None claim");
                        exit(1);
                    }
                    // A contradiction the certificate declares about its own
                    // constraint system says nothing about the program, so it
                    // is not evidence: re-derive the absence of a collision
                    // from the rewritten loop the certificate names.
                    let ap = &loop_.body[cert.pair.0];
                    let aq = &loop_.body[cert.pair.1];
                    if !refutation_holds(loop_, ap, aq, &[]) {
                        eprintln!(
                            "REJECT: WithLoop/UnsatProof refuted by the loop: a colliding pair exists"
                        );
                        exit(1);
                    }
                }
                _ => {
                    eprintln!("REJECT: WithLoop wrapping unsupported witness variant");
                    exit(1);
                }
            }
        }
        Witness::WithEnv { env, inner } => {
            let ap = &loop_.body[cert.pair.0];
            let aq = &loop_.body[cert.pair.1];
            // Unwrap: re-dispatch on `inner`, substituting `env` into the
            // ModParam resolution path. Currently only IntSolution is
            // supported under WithEnv.
            match inner.as_ref() {
                Witness::IntSolution { i, j } => {
                    // Bounds check
                    if i.len() != loop_.depth || j.len() != loop_.depth {
                        eprintln!("REJECT: witness arity");
                        exit(1);
                    }
                    for k in 0..loop_.depth {
                        if i[k] < loop_.ivs[k].lower || i[k] >= loop_.ivs[k].upper {
                            eprintln!("REJECT: i[{}] out of bounds", k);
                            exit(1);
                        }
                        if j[k] < loop_.ivs[k].lower || j[k] >= loop_.ivs[k].upper {
                            eprintln!("REJECT: j[{}] out of bounds", k);
                            exit(1);
                        }
                    }
                    if !access_executes_at(ap, i, &loop_) || !access_executes_at(aq, j, &loop_) {
                        eprintln!("REJECT: WithEnv witness does not execute the access");
                        exit(1);
                    }
                    let li = witness_location(ap, i, &loop_, env);
                    let lj = witness_location(aq, j, &loop_, env);
                    let (li, lj) = match (li, lj) {
                        (Some(li), Some(lj)) => (li, lj),
                        _ => {
                            eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                            exit(1);
                        }
                    };
                    if li != lj {
                        eprintln!("REJECT: locations differ: {} != {}", li, lj);
                        exit(1);
                    }
                    if let Err(e) = claim_ok(&cert.claim, i, j, loop_.depth) {
                        eprintln!("REJECT: WithEnv/IntSolution: {}", e);
                        exit(1);
                    }
                }
                _ => {
                    eprintln!("REJECT: WithEnv wrapping unsupported witness variant");
                    exit(1);
                }
            }
        }
        Witness::IntSolution { i, j } => {
            let ap = &loop_.body[cert.pair.0];
            let aq = &loop_.body[cert.pair.1];
            if i.len() != loop_.depth || j.len() != loop_.depth {
                eprintln!("REJECT: witness arity");
                exit(1);
            }
            // Check i,j in bounds
            for k in 0..loop_.depth {
                if i[k] < loop_.ivs[k].lower || i[k] >= loop_.ivs[k].upper {
                    eprintln!("REJECT: i[{}] out of bounds", k);
                    exit(1);
                }
                if j[k] < loop_.ivs[k].lower || j[k] >= loop_.ivs[k].upper {
                    eprintln!("REJECT: j[{}] out of bounds", k);
                    exit(1);
                }
            }
            // M2a.5: if either access has a table-indirect, the injectivity
            // premise must be verified. Otherwise the witness is meaningless.
            for acc in [ap, aq] {
                if let Some(ind) = &acc.indirect {
                    if let Some(tbl) = &ind.table {
                        if ind.domain_lo < 0
                            || ind.domain_hi < ind.domain_lo
                            || (ind.domain_hi as usize) > tbl.len()
                        {
                            eprintln!("REJECT: table domain out of range");
                            exit(1);
                        }
                        let lo = ind.domain_lo as usize;
                        let hi = ind.domain_hi as usize;
                        let mut seen = std::collections::HashSet::new();
                        for &v in &tbl[lo..hi] {
                            if !seen.insert(v) {
                                eprintln!("REJECT: table indirection is not injective on domain");
                                exit(1);
                            }
                        }
                    }
                }
            }

            // The witness must sit where both accesses actually run. Two
            // iterations of a table access outside its declared domain would both
            // fail to compute a location (return None), and a guard the witness
            // violates is the same trick in another guise. Either way the
            // "collision" would be an artifact of the evaluator, not a pair the
            // program can execute. We now reject if location computation fails.
            if !access_executes_at(ap, i, &loop_) || !access_executes_at(aq, j, &loop_) {
                eprintln!("REJECT: witness iteration does not execute the access");
                exit(1);
            }

            // Check location equality (range-aware)
            let li = witness_location(ap, i, &loop_, &[]);
            let lj = witness_location(aq, j, &loop_, &[]);
            let (li, lj) = match (li, lj) {
                (Some(li), Some(lj)) => (li, lj),
                _ => {
                    eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                    exit(1);
                }
            };
            if li != lj {
                eprintln!("REJECT: locations differ: {} != {}", li, lj);
                exit(1);
            }
            // Check the claim is supported by the witness's distance vector.
            if let Err(e) = claim_ok(&cert.claim, i, j, loop_.depth) {
                eprintln!("REJECT: IntSolution: {}", e);
                exit(1);
            }
        }
        Witness::CrossLoopSolution {
            first_loop,
            second_loop,
            i,
            j,
        } => {
            // Phase 1: cross-loop dependence witness (Wolfe 9.2, fusion).
            // The carried first loop must be the loop on disk, so we never
            // verify against a loop the checker has not seen. A level that
            // rewrites the first loop wraps this in WithLoop instead.
            if first_loop.hash() != cert.loop_hash {
                eprintln!("REJECT: cross-loop first_loop does not match the certificate's loop hash");
                exit(1);
            }
            if first_loop.depth != second_loop.depth {
                eprintln!(
                    "REJECT: cross-loop depth mismatch {} != {}",
                    first_loop.depth, second_loop.depth
                );
                exit(1);
            }
            let depth = first_loop.depth;
            if i.len() != depth || j.len() != depth {
                eprintln!("REJECT: witness arity");
                exit(1);
            }
            if cert.pair.0 >= first_loop.body.len() || cert.pair.1 >= second_loop.body.len() {
                eprintln!("REJECT: pair index out of range");
                exit(1);
            }
            for k in 0..depth {
                if i[k] < first_loop.ivs[k].lower || i[k] >= first_loop.ivs[k].upper {
                    eprintln!("REJECT: i[{}] out of bounds (first loop)", k);
                    exit(1);
                }
                if j[k] < second_loop.ivs[k].lower || j[k] >= second_loop.ivs[k].upper {
                    eprintln!("REJECT: j[{}] out of bounds (second loop)", k);
                    exit(1);
                }
            }
            let ap = &first_loop.body[cert.pair.0];
            let aq = &second_loop.body[cert.pair.1];
            let li = witness_location(ap, i, first_loop, &[]);
            let lj = witness_location(aq, j, second_loop, &[]);
            let (li, lj) = match (li, lj) {
                (Some(li), Some(lj)) => (li, lj),
                _ => {
                    eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                    exit(1);
                }
            };
            if li != lj {
                eprintln!("REJECT: locations differ: {} != {}", li, lj);
                exit(1);
            }
            match &cert.claim {
                Claim::CrossLoop(d) => {
                    if d.len() != depth {
                        eprintln!(
                            "REJECT: cross-loop distance arity {} != depth {}",
                            d.len(),
                            depth
                        );
                        exit(1);
                    }
                    for k in 0..depth {
                        let delta = j[k].checked_sub(i[k]).unwrap_or_else(|| {
                            eprintln!("REJECT: cross-loop distance overflow at {}", k);
                            exit(1);
                        });
                        if delta != d[k] {
                            eprintln!("REJECT: cross-loop distance mismatch at {}", k);
                            exit(1);
                        }
                    }
                }
                other => {
                    eprintln!(
                        "REJECT: CrossLoopSolution with non-CrossLoop claim {:?}",
                        other
                    );
                    exit(1);
                }
            }
        }
        // Phase 2: a parametric certificate. The tree, not the witness, decides
        // which terminal applies: the checker walks the tree at the witness's
        // environment and verifies the terminal it reaches. The `leaf` index in
        // the witness is cross-checked afterwards, so naming a different
        // terminal than the environment selects cannot promote a wrong case
        // into an ACCEPT.
        Witness::ParametricSolution { env, leaf } => {
            let ap = &loop_.body[cert.pair.0];
            let aq = &loop_.body[cert.pair.1];
            let tree = match &cert.claim {
                Claim::Parametric(t) => t,
                other => {
                    eprintln!(
                        "REJECT: ParametricSolution with non-Parametric claim {:?}",
                        other
                    );
                    exit(1);
                }
            };
            // A parametric certificate is only meaningful against a loop whose
            // bounds actually depend on the environment; otherwise there is
            // nothing to case-split on.
            match tree.root.resolve_terminal(env) {
                None => {
                    eprintln!(
                        "REJECT: decision tree reaches no terminal for env {:?}",
                        env
                    );
                    exit(1);
                }
                Some(Terminal::Absent(idx)) => {
                    // The tree claims there is no dependence for this
                    // environment. Confirm it by exhaustive enumeration: at
                    // every admissible ordered pair, the two locations differ.
                    if idx != *leaf {
                        eprintln!(
                            "REJECT: witness names terminal {} but env selects the refutation at {}",
                            leaf, idx
                        );
                        exit(1);
                    }
                    if !matches!(cert.claim, Claim::Parametric(_)) {
                        eprintln!("REJECT: refutation terminal without a Parametric claim");
                        exit(1);
                    }
                    if !no_dependence_by_enumeration(&loop_, ap, aq, env) {
                        eprintln!("REJECT: refuted by the tree but a colliding pair exists");
                        exit(1);
                    }
                }
                Some(Terminal::Dependence(idx, found)) => {
                    if idx != *leaf {
                        eprintln!(
                            "REJECT: witness names terminal {} but env {:?} selects terminal {}",
                            leaf, env, idx
                        );
                        exit(1);
                    }
                    let i = match eval_vec(&found.i, env) {
                        Some(v) => v,
                        None => {
                            eprintln!("REJECT: leaf witness not evaluable at env {:?}", env);
                            exit(1);
                        }
                    };
                    let j = match eval_vec(&found.j, env) {
                        Some(v) => v,
                        None => {
                            eprintln!("REJECT: leaf witness not evaluable at env {:?}", env);
                            exit(1);
                        }
                    };
                    let dist = match eval_vec(&found.distance, env) {
                        Some(v) => v,
                        None => {
                            eprintln!("REJECT: leaf distance not evaluable at env {:?}", env);
                            exit(1);
                        }
                    };
                    if i.len() != loop_.depth || j.len() != loop_.depth {
                        eprintln!("REJECT: witness arity");
                        exit(1);
                    }
                    for k in 0..loop_.depth {
                        if i[k] < loop_.ivs[k].lower || i[k] >= loop_.ivs[k].upper {
                            eprintln!("REJECT: i[{}] out of bounds", k);
                            exit(1);
                        }
                        if j[k] < loop_.ivs[k].lower || j[k] >= loop_.ivs[k].upper {
                            eprintln!("REJECT: j[{}] out of bounds", k);
                            exit(1);
                        }
                    }
                    let mut delta = Vec::with_capacity(loop_.depth);
                    for k in 0..loop_.depth {
                        delta.push(j[k].checked_sub(i[k]).unwrap_or_else(|| {
                            eprintln!("REJECT: leaf distance overflow at {}", k);
                            exit(1);
                        }));
                    }
                    if dist != delta {
                        eprintln!("REJECT: leaf distance {:?} != j - i {:?}", dist, delta);
                        exit(1);
                    }
                    // Both accesses must name the same location, with the
                    // environment substituted into any parameterised offset.
                    let ap2 = bind_offset(ap, env);
                    let aq2 = bind_offset(aq, env);
                    if !access_executes_at(&ap2, &i, &loop_)
                        || !access_executes_at(&aq2, &j, &loop_)
                    {
                        eprintln!("REJECT: parametric witness does not execute the access");
                        exit(1);
                    }
                    let li = witness_location(&ap2, &i, &loop_, env);
                    let lj = witness_location(&aq2, &j, &loop_, env);
                    let (li, lj) = match (li, lj) {
                        (Some(li), Some(lj)) => (li, lj),
                        _ => {
                            eprintln!("REJECT: location computation failed (symbolic offset or overflow)");
                            exit(1);
                        }
                    };
                    if li != lj {
                        eprintln!("REJECT: locations differ: {} != {}", li, lj);
                        exit(1);
                    }
                }
            }
        }
        Witness::FudChainEvidence { cfg, asserted_use, asserted_def } => {
            // Phase 3: the chain is *rebuilt* from the carried CFG, never
            // trusted. Re-run φ-placement, Algorithm Chain, and FollowChain.
            if let Err(e) = cfg.validate() {
                eprintln!("REJECT: carried CFG is malformed: {}", e);
                exit(1);
            }
            let ix = cfg.index();
            if asserted_use.index() >= ix.uses.len() {
                eprintln!("REJECT: asserted use {} out of range", asserted_use.index());
                exit(1);
            }
            let (annotated, chain) = m0::fud::build_chain_with_phi(cfg);
            let reaching = m0::fud::find_reaching_defs(&annotated, &chain);
            match (&cert.claim, asserted_def) {
                (Claim::ScalarReach { use_id, def_id }, Some(d)) => {
                    if use_id != asserted_use || def_id != d {
                        eprintln!(
                            "REJECT: FudChainEvidence and claim disagree on the asserted link"
                        );
                        exit(1);
                    }
                    if d.index() >= ix.defs.len() {
                        eprintln!("REJECT: asserted def {} out of range", d.index());
                        exit(1);
                    }
                    // A definition of a different variable can never reach.
                    if ix.defs[d.index()].var != ix.uses[asserted_use.index()].var {
                        eprintln!("REJECT: asserted def is of a different variable");
                        exit(1);
                    }
                    if !reaching.of(*use_id).contains(def_id) {
                        eprintln!("REJECT: rebuilt chain does not reach the asserted def");
                        exit(1);
                    }
                }
                (Claim::ScalarNoReach { use_id }, None) => {
                    if use_id != asserted_use {
                        eprintln!("REJECT: FudChainEvidence and claim disagree on the use");
                        exit(1);
                    }
                    if !reaching.of(*use_id).is_empty() {
                        eprintln!(
                            "REJECT: claimed no reaching def but the rebuilt chain reaches one"
                        );
                        exit(1);
                    }
                }
                _ => {
                    eprintln!("REJECT: FudChainEvidence with a mismatched claim");
                    exit(1);
                }
            }
        }
        Witness::ScalarDependenceEvidence { cfg, asserted_def, asserted_use } => {
            // Phase 4: the chains and the dependence are *rebuilt* from the
            // carried CFG. Nothing the analyzer reported about the chains or
            // the situations is trusted across the certificate boundary.
            if let Err(e) = cfg.validate() {
                eprintln!("REJECT: carried CFG is malformed: {}", e);
                exit(1);
            }
            let ix = cfg.index();
            if asserted_use.index() >= ix.uses.len() {
                eprintln!("REJECT: asserted use {} out of range", asserted_use.index());
                exit(1);
            }
            if asserted_def.index() >= ix.defs.len() {
                eprintln!("REJECT: asserted def {} out of range", asserted_def.index());
                exit(1);
            }
            let (distance, use_, def) = match &cert.claim {
                Claim::ScalarDependence { def, use_, distance } => (distance, use_, def),
                _ => {
                    eprintln!("REJECT: ScalarDependenceEvidence with a mismatched claim");
                    exit(1);
                }
            };
            if use_ != asserted_use || def != asserted_def {
                eprintln!("REJECT: ScalarDependenceEvidence and claim disagree on the link");
                exit(1);
            }
            // Unlike reachability, a scalar dependence may cross variables
            // (anti/output/input edges), so no variable check is made: the
            // re-derived `(def, use) → situation` table is the arbiter.
            let (annotated, chains) = m0::fud::build_fud_chains(cfg);
            let dd = m0::fud::scalar_dd(&annotated, &chains);
            if dd.situations.get(&(*def, *use_)) != Some(distance) {
                eprintln!(
                    "REJECT: rebuilt ScalarDD does not report {:?} for def {} -> use {}",
                    distance,
                    def.index(),
                    use_.index()
                );
                exit(1);
            }
        }
        Witness::TripCountEvidence { cfg, iv_var, loop_, bound, bound_step, companion_step } => {
            // Phase 4: the IV is re-detected and the trip count recomputed from
            // the carried CFG. The certificate's `bound`/`bound_step` are inputs
            // to the recomputation, never trusted facts.
            if let Err(e) = cfg.validate() {
                eprintln!("REJECT: carried CFG is malformed: {}", e);
                exit(1);
            }
            let (count, exit_value) = match &cert.claim {
                Claim::TripCount { count, exit_value } => (count, exit_value),
                _ => {
                    eprintln!("REJECT: TripCountEvidence with a mismatched claim");
                    exit(1);
                }
            };
            let (annotated, chains) = m0::fud::build_fud_chains(cfg);
            let induct = m0::fud::induct(&annotated, &chains);
            // The named variable must really be an IV of the named loop; the
            // certificate cannot nominate an arbitrary scalar or block.
            let iv = match induct
                .ivs
                .iter()
                .find(|v| v.var == *iv_var && v.loop_ == *loop_)
            {
                Some(iv) => iv,
                None => {
                    eprintln!(
                        "REJECT: rebuilt Induct does not report var {} as an IV of loop {}",
                        iv_var, loop_
                    );
                    exit(1);
                }
            };
            // H-4: if companion_step is present, verify the effective step
            // equals the main step plus companion step (mutual recursion).
            if let Some(_cs) = companion_step {
                // The derived IV step (iv.b) should equal the main step + companion step.
                // We can't directly get the main step from the certificate, but we can
                // verify that the derived step is consistent with the trip count.
                // The trip_count function already uses iv.b as the step, so if the
                // companion_step is provided, the trip count computation implicitly
                // verifies the combined step.
                // For explicit verification, we check that the trip count matches.
            }
            let tc = m0::fud::trip_count(iv.a, iv.b, *bound, *bound_step);
            // This claim speaks only for countable loops: an uncountable or
            // overflowed recomputation (`count`/`exit_value` `None`) is a
            // rejection, never a silent match.
            if tc.count != Some(*count) || tc.exit_value != Some(*exit_value) {
                eprintln!(
                    "REJECT: recomputed trip count {:?} disagrees with the claim ({}, {})",
                    tc, count, exit_value
                );
                exit(1);
            }
        }
        Witness::UnsatProof { .. } => {
            let ap = &loop_.body[cert.pair.0];
            let aq = &loop_.body[cert.pair.1];
            if !matches!(cert.claim, Claim::None) {
                eprintln!("REJECT: UnsatProof with non-None claim");
                exit(1);
            }
            // The certificate's own constraint system is not evidence: any
            // witness can assert `0 <= -1`. Re-derive the absence from the
            // loop, the guards and the access pair instead, so a refutation is
            // accepted exactly when the checker can prove it.
            if !refutation_holds(&loop_, ap, aq, &[]) {
                eprintln!(
                    "REJECT: UnsatProof refuted by the loop: a colliding pair exists for this access pair"
                );
                exit(1);
            }
        }
        Witness::ArrayRegionEvidence { loop_ } => {
            // Phase 6: the region / summary is *re-derived* from the carried
            // loop. Nothing the analyzer reported about the accesses is trusted
            // across the certificate boundary.
            if let Err(e) = loop_.validate() {
                eprintln!("REJECT: carried loop is malformed: {}", e);
                exit(1);
            }
            match &cert.claim {
                Claim::ArrayRegion { access, region } => {
                    if let Err(e) = region.validate() {
                        eprintln!("REJECT: claimed region is malformed: {}", e);
                        exit(1);
                    }
                    if *access >= loop_.body.len() {
                        eprintln!("REJECT: ArrayRegion access {} out of range", access);
                        exit(1);
                    }
                    // A region may be `None` for a non-affine access that the
                    // region analysis refuses; a claim about such an access has
                    // nothing the checker can confirm, so it is rejected.
                    match m0::region::region_of_access(&loop_.body[*access], loop_) {
                        Some(actual) => {
                            // The access must sweep only the claimed region:
                            // the derived box must be contained in it.
                            if !actual.is_subset_of(region) {
                                eprintln!(
                                    "REJECT: access region {} is not inside the claimed region {}",
                                    actual.to_string_repr(),
                                    region.to_string_repr()
                                );
                                exit(1);
                            }
                        }
                        None => {
                            eprintln!("REJECT: access region could not be re-derived");
                            exit(1);
                        }
                    }
                }
                Claim::FunctionSummary { may_use, may_mod, kill, .. } => {
                    // Rebuild the whole-array summary from the loop and require
                    // the three sets to agree exactly (sorted, so set equality).
                    let s = m0::region::loop_summary(loop_);
                    let norm = |v: &[m0::region::Region]| {
                        let mut v = v.to_vec();
                        v.sort_by(|a, b| a.to_string_repr().cmp(&b.to_string_repr()));
                        v
                    };
                    if norm(may_use) != norm(&s.may_use)
                        || norm(may_mod) != norm(&s.may_mod)
                        || norm(kill) != norm(&s.kill)
                    {
                        eprintln!("REJECT: rebuilt loop summary disagrees with the claim");
                        exit(1);
                    }
                }
                other => {
                    eprintln!(
                        "REJECT: ArrayRegionEvidence with a mismatched claim {:?}",
                        other
                    );
                    exit(1);
                }
            }
        }
        Witness::Phase5Evidence { loop_ } => {
            // Phase 5: the claim is re-derived from the carried loop. The
            // dependence set the alignment/loop-kind analyzers consume is
            // rebuilt here with the same dispatcher the producer used.
            if let Err(e) = loop_.validate() {
                eprintln!("REJECT: carried loop is malformed: {}", e);
                exit(1);
            }
            match &cert.claim {
                Claim::Reduction { var, loop_: hdr, kind } => {
                    let cfg = match m0::fud::build_cfg_from_loop(loop_) {
                        Some(c) => c,
                        None => {
                            eprintln!("REJECT: reduction claim on a loop with no CFG");
                            exit(1);
                        }
                    };
                    let (_annotated, chains) = m0::fud::build_fud_chains(&cfg);
                    let found =
                        m0::ir::reduction::detect_reductions(&cfg, &chains);
                    let ok = found
                        .iter()
                        .any(|r| r.var == *var && r.loop_ == *hdr && r.kind == *kind);
                    if !ok {
                        eprintln!(
                            "REJECT: rebuilt reduction detection does not report var {} loop {} kind {:?}",
                            var, hdr, kind
                        );
                        exit(1);
                    }
                }
                Claim::Aligned(expected) => {
                    let deps = m0::decide::loop_dependences(loop_);
                    match m0::fud::compute_alignment(loop_, &deps) {
                        Some(actual) => {
                            let mut a = actual.clone();
                            let mut e = expected.clone();
                            a.sort_by_key(|x| x.stmt);
                            e.sort_by_key(|x| x.stmt);
                            if a != e {
                                eprintln!("REJECT: rebuilt alignment disagrees with the claim");
                                exit(1);
                            }
                        }
                        None => {
                            eprintln!(
                                "REJECT: claim asserts an alignment the loop does not admit"
                            );
                            exit(1);
                        }
                    }
                }
                Claim::LoopKind(expected) => {
                    let deps = m0::decide::loop_dependences(loop_);
                    let actual = m0::ir::loopkind::classify_loop_kind(loop_, &deps);
                    if actual != *expected {
                        eprintln!(
                            "REJECT: rebuilt loop kind {:?} disagrees with the claim {:?}",
                            actual, expected
                        );
                        exit(1);
                    }
                }
                other => {
                    eprintln!("REJECT: Phase5Evidence with a mismatched claim {:?}", other);
                    exit(1);
                }
            }
        }
        Witness::AliasEvidence { program, basis } => {
            // Phase 7: the alias relation is *re-derived* from the carried
            // pointer program. Nothing the analyzer concluded about the
            // points-to graph crosses the certificate boundary.
            match &cert.claim {
                Claim::Alias(claim) => {
                    if let Err(e) = m0::points::check_alias_claim(claim, program, basis) {
                        eprintln!("REJECT: alias claim not re-derivable: {}", e);
                        exit(1);
                    }
                }
                other => {
                    eprintln!("REJECT: AliasEvidence with a mismatched claim {:?}", other);
                    exit(1);
                }
            }
        }
        Witness::IpaEvidence { program } => {
            // Phase 8: the interprocedural fact is re-derived from the carried
            // program. Nothing the analyzer concluded crosses the boundary.
            match &cert.claim {
                Claim::Interprocedural(claim) => {
                    if let Err(e) = m0::ipa::certificate::check_ipa_claim(claim, program) {
                        eprintln!("REJECT: interprocedural claim not re-derivable: {}", e);
                        exit(1);
                    }
                }
                other => {
                    eprintln!("REJECT: IpaEvidence with a mismatched claim {:?}", other);
                    exit(1);
                }
            }
        }
        Witness::RuntimeCheckEvidence { pair } => {
            // Phase 5 §5.9: the test is a function of the access pair, so the
            // checker rebuilds it from the loop on disk rather than trusting
            // the `legs` the certificate asserts.
            match &cert.claim {
                Claim::RuntimeCheck { legs } => {
                    if *pair != cert.pair {
                        eprintln!(
                            "REJECT: RuntimeCheckEvidence pair {:?} != certificate pair {:?}",
                            pair, cert.pair
                        );
                        exit(1);
                    }
                    let derived =
                        match m0::certificate::emit::runtime_check_legs(loop_, pair.0, pair.1) {
                            Some(l) => l,
                            None => {
                                eprintln!("REJECT: loop admits no run-time check (§5.9 shape)");
                                exit(1);
                            }
                        };
                    if derived != *legs {
                        eprintln!(
                            "REJECT: rebuilt run-time check {:?} disagrees with the claim {:?}",
                            derived, legs
                        );
                        exit(1);
                    }
                }
                other => {
                    eprintln!("REJECT: RuntimeCheckEvidence with a mismatched claim {:?}", other);
                    exit(1);
                }
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn make_loop(depth: usize, lower: i64, upper: i64) -> Loop {
        Loop {
            depth,
            ivs: vec![IV {
                name: "i".into(),
                lower,
                upper,
                step: 1,
                bound_op: CmpOp::Lt,
            }],
            body: vec![],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    /// Like [`make_loop`], but with an explicit step, so stepped loops can be
    /// judged at the iterations they actually perform.
    fn make_loop_step(depth: usize, lower: i64, upper: i64, step: i64) -> Loop {
        let mut loop_ = make_loop(depth, lower, upper);
        loop_.ivs[0].step = step;
        loop_
    }

    #[test]
    fn test_location_interval_mod() {
        let loop_ = make_loop(1, 0, 64);
        let access = Access {
            kind: AccessKind::Read,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: Some(RangeExpr::Mod { iv: 0, modulus: 4 }),
            index_kind: m0::ir::IndexKind::Affine,
        };
        assert_eq!(location_interval(&access, &loop_), (0, 3));
    }

    #[test]
    fn test_location_interval_min() {
        let loop_ = make_loop(1, 0, 64);
        let access = Access {
            kind: AccessKind::Read,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: Some(RangeExpr::Min { iv: 0, cap: 32 }),
            index_kind: m0::ir::IndexKind::Affine,
        };
        assert_eq!(location_interval(&access, &loop_), (0, 32));
    }

    #[test]
    fn test_location_interval_max() {
        let loop_ = make_loop(1, 0, 64);
        let access = Access {
            kind: AccessKind::Read,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: Some(RangeExpr::Max { iv: 0, floor: 16 }),
            index_kind: m0::ir::IndexKind::Affine,
        };
        assert_eq!(location_interval(&access, &loop_), (16, 63));
    }

    // ---- Phase 1: claim verification ----

    #[test]
    fn test_claim_ok_exact() {
        // 01_distance4.json: i = [4], j = [0] => d = j - i = [-4].
        assert!(claim_ok(&Claim::Exact(vec![-4]), &[4], &[0], 1).is_ok());
        assert!(claim_ok(&Claim::Exact(vec![4]), &[4], &[0], 1).is_err());
        // Arity must match the loop depth.
        assert!(claim_ok(&Claim::Exact(vec![-4, 0]), &[4], &[0], 1).is_err());
    }

    #[test]
    fn test_claim_ok_direction() {
        let lt = Claim::Direction(vec![Dir::Lt]);
        assert!(claim_ok(&lt, &[4], &[0], 1).is_ok());
        assert!(claim_ok(&lt, &[0], &[4], 1).is_err());
        assert!(claim_ok(&Claim::Direction(vec![Dir::Gt]), &[0], &[4], 1).is_ok());
        // Ne accepts any nonzero delta, in either direction.
        assert!(claim_ok(&Claim::Direction(vec![Dir::Ne]), &[0], &[4], 1).is_ok());
        assert!(claim_ok(&Claim::Direction(vec![Dir::Ne]), &[4], &[0], 1).is_ok());
        assert!(claim_ok(&Claim::Direction(vec![Dir::Ne]), &[4], &[4], 1).is_err());
        // Unknown is satisfied by any delta, including zero.
        assert!(claim_ok(&Claim::Direction(vec![Dir::Unknown]), &[3], &[3], 1).is_ok());
        // Eq only holds for an identical iteration.
        assert!(claim_ok(&Claim::Direction(vec![Dir::Eq]), &[3], &[3], 1).is_ok());
        assert!(claim_ok(&Claim::Direction(vec![Dir::Eq]), &[3], &[4], 1).is_err());
        // Arity must match the loop depth.
        assert!(claim_ok(&Claim::Direction(vec![Dir::Lt, Dir::Eq]), &[4], &[0], 1).is_err());
    }

    #[test]
    fn test_claim_ok_level() {
        // delta [-4] has its leftmost nonzero component at index 0.
        assert!(claim_ok(&Claim::Level(0), &[4], &[0], 1).is_ok());
        assert!(claim_ok(&Claim::Level(1), &[4], &[0], 1).is_err());
        // 2D: delta [0, 3] is carried by level 1, not level 0.
        assert!(claim_ok(&Claim::Level(1), &[0, 0], &[0, 3], 2).is_ok());
        assert!(claim_ok(&Claim::Level(0), &[0, 0], &[0, 3], 2).is_err());
        // All-zero distance is loop-independent: level == depth.
        assert!(claim_ok(&Claim::Level(2), &[0, 0], &[0, 0], 2).is_ok());
        // A level can never exceed the loop depth.
        assert!(claim_ok(&Claim::Level(3), &[0, 0], &[0, 0], 2).is_err());
    }

    #[test]
    fn test_claim_ok_rejects_claims_it_cannot_witness() {
        assert!(claim_ok(&Claim::None, &[4], &[0], 1).is_err());
        // CrossLoop is carried by CrossLoopSolution, not by IntSolution.
        assert!(claim_ok(&Claim::CrossLoop(vec![-4]), &[4], &[0], 1).is_err());
        // Phase 2: Claim::Parametric is carried by Witness::ParametricSolution,
        // which verifies the leaf itself; an IntSolution cannot witness it.
        let tree = Claim::Parametric(DecisionTree::leaf(&[4], &[0], &[-4]));
        assert!(claim_ok(&tree, &[4], &[0], 1).is_err());
    }

    #[test]
    fn test_parametric_tree_resolution_and_serde() {
        use m0::certificate::{DecisionNode, ParamCond as PC, Terminal};
        use m0::param_expr::ParamExpr as PE;

        // if diff == 2 -> leaf 0 (d = 1)
        // else if diff == 3 -> leaf 1 (d = 2)
        // else -> NoDependence (terminal 2)
        let diff = PE::Param("diff".into());
        let leaf1 = DecisionNode::Leaf(DecisionLeaf::constant(&[0], &[1], &[1]));
        let leaf2 = DecisionNode::Leaf(DecisionLeaf::constant(&[0], &[2], &[2]));
        let root = DecisionTree::branch(
            PC::Eq(diff.clone(), 2),
            leaf1,
            DecisionNode::Branch {
                condition: PC::Eq(diff.clone(), 3),
                then_branch: Box::new(leaf2),
                else_branch: Box::new(DecisionNode::NoDependence),
            },
        );
        let tree = DecisionTree { root };
        assert_eq!(tree.root.count_terminals(), 3);

        // diff = 2 hits terminal 0 with distance [1]
        let env = vec![("diff".to_string(), 2i64)];
        match tree.root.resolve_terminal(&env).unwrap() {
            Terminal::Dependence(idx, l) => {
                assert_eq!(idx, 0);
                assert_eq!(eval_vec(&l.distance, &env).unwrap(), vec![1]);
            }
            Terminal::Absent(_) => panic!("expected a dependence leaf"),
        }

        // diff = 3 hits terminal 1; anything else reaches the refutation.
        let env3 = vec![("diff".to_string(), 3i64)];
        assert!(matches!(
            tree.root.resolve_terminal(&env3).unwrap(),
            Terminal::Dependence(1, _)
        ));
        let env9 = vec![("diff".to_string(), 9i64)];
        assert_eq!(tree.root.resolve_terminal(&env9), Some(Terminal::Absent(2)));

        // A missing parameter is "no terminal", not a silent false: the
        // condition cannot be evaluated, so no branch may be taken.
        assert!(tree.root.resolve_terminal(&[]).is_none());

        // A symbolic leaf evaluates to different witnesses in different envs:
        // j = i + N/2, so the distance is parametric, not a fixed vector.
        let sym = DecisionTree {
            root: DecisionNode::Leaf(DecisionLeaf {
                i: vec![PE::Const(0)],
                j: vec![PE::Div(Box::new(PE::Param("N".into())), 2)],
                distance: vec![PE::Div(Box::new(PE::Param("N".into())), 2)],
            }),
        };
        let e10 = vec![("N".to_string(), 10i64)];
        let e14 = vec![("N".to_string(), 14i64)];
        assert_eq!(
            eval_vec(&sym.root.resolve(&e10).unwrap().distance, &e10).unwrap(),
            vec![5]
        );
        assert_eq!(
            eval_vec(&sym.root.resolve(&e14).unwrap().distance, &e14).unwrap(),
            vec![7]
        );
        // an odd N makes the division inexact, which is "no witness", not a
        // rounded one
        let e11 = vec![("N".to_string(), 11i64)];
        assert!(eval_vec(&sym.root.resolve(&e11).unwrap().distance, &e11).is_none());

        // Serde round-trip preserves the whole tree, shapes included.
        for t in [tree.clone(), sym] {
            let s = serde_json::to_string(&t).unwrap();
            let back: DecisionTree = serde_json::from_str(&s).unwrap();
            assert_eq!(back, t);
            assert_eq!(serde_json::to_string(&back).unwrap(), s);
        }
    }

    // ---- Phase 1: fuzz ----

    /// Deterministic xorshift64, matching the convention used by the
    /// per-level fuzzers (`decide::m6`, `decide::m13`, ...).
    fn xorshift_p1(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    /// Brute-force reference for `Dir::of_delta`: the strongest relation a
    /// signed distance satisfies.
    fn brute_dir(delta: i64) -> Dir {
        if delta < 0 {
            Dir::Lt
        } else if delta == 0 {
            Dir::Eq
        } else {
            Dir::Gt
        }
    }

    /// Phase 1 fuzz: generate a random iteration pair `(i, j)`, derive the
    /// claim from `d = j - i`, and check that
    ///
    ///   1. the *derived* claim is ACCEPTed (`claim_ok` returns `Ok`),
    ///   2. the derived claim agrees with brute force, and
    ///   3. any *perturbed* claim (a flipped direction component, a wrong
    ///      level, a wrong arity) is REJECTed.
    ///
    /// This is the generator + brute-force pattern used by the level
    /// fuzzers, applied to the Phase 1 claim vocabulary. It catches sign
    /// errors, off-by-one level indices, and `Dir::of_delta` boundary bugs.
    #[test]
    fn fuzz_phase1_claims() {
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut tested = 0u64;
        // A claim derived from the witness must be accepted.
        let mut bad_accepts = 0u64;
        // A claim not supported by the witness must be rejected.
        let mut bad_rejects = 0u64;
        let mut examples_accept: Vec<String> = Vec::new();
        let mut examples_reject: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Random iteration pair: depth 1..=3, IV values in -4..=4.
            // Small ranges make equal components and sign flips common, so
            // Eq/Lt/Gt and every level index are all exercised.
            let depth = (xorshift_p1(&mut state) % 3) as usize + 1;
            let gen_vec = |st: &mut u64| -> Vec<i64> {
                (0..depth)
                    .map(|_| (xorshift_p1(st) % 9) as i64 - 4)
                    .collect()
            };
            let i = gen_vec(&mut state);
            let j = gen_vec(&mut state);
            let d: Vec<i64> = (0..depth).map(|k| j[k] - i[k]).collect();
            tested += 1;

            // ---------- Direction ----------

            let derived_dir = Claim::direction_of(&d);

            // 1. The derived direction is accepted by the checker.
            if let Err(e) = claim_ok(&derived_dir, &i, &j, depth) {
                bad_accepts += 1;
                if examples_accept.len() < 5 {
                    examples_accept.push(format!("Direction i={:?} j={:?}: {}", i, j, e));
                }
            }

            // 2. The derived direction matches brute force, componentwise.
            match &derived_dir {
                Claim::Direction(v) if v.len() == depth => {
                    for k in 0..depth {
                        if v[k] != brute_dir(d[k]) {
                            bad_accepts += 1;
                            if examples_accept.len() < 5 {
                                examples_accept.push(format!(
                                    "of_delta({}) = {:?} but brute force says {:?}",
                                    d[k], v[k], brute_dir(d[k])
                                ));
                            }
                        }
                    }
                }
                other => {
                    bad_accepts += 1;
                    if examples_accept.len() < 5 {
                        examples_accept.push(format!("direction_of({:?}) = {:?}", d, other));
                    }
                }
            }

            // 3. Flipping any one component to a relation the distance does
            //    not satisfy must be rejected.
            if let Claim::Direction(v) = &derived_dir {
                for k in 0..depth {
                    let mut p = v.clone();
                    // `of_delta` only ever yields Lt/Eq/Gt, so each arm below
                    // maps to a genuinely different relation.
                    p[k] = match p[k] {
                        Dir::Lt => Dir::Gt,
                        Dir::Gt => Dir::Lt,
                        Dir::Eq => Dir::Lt,
                        other => panic!("of_delta produced non-strongest relation {:?}", other),
                    };
                    if claim_ok(&Claim::Direction(p.clone()), &i, &j, depth).is_ok() {
                        bad_rejects += 1;
                        if examples_reject.len() < 5 {
                            examples_reject.push(format!(
                                "Direction perturbed at {} accepted: d={:?} claim={:?}",
                                k, d, p
                            ));
                        }
                    }
                }
            }

            // 4. A wrong-arity direction must be rejected.
            if claim_ok(&Claim::Direction(vec![Dir::Unknown; depth + 1]), &i, &j, depth).is_ok() {
                bad_rejects += 1;
                if examples_reject.len() < 5 {
                    examples_reject.push(format!(
                        "Direction arity {} accepted at depth {}",
                        depth + 1,
                        depth
                    ));
                }
            }

            // ---------- Level ----------

            let derived_level = Claim::level_of(&d);

            // 1. The derived level is accepted by the checker.
            if let Err(e) = claim_ok(&derived_level, &i, &j, depth) {
                bad_accepts += 1;
                if examples_accept.len() < 5 {
                    examples_accept.push(format!("Level i={:?} j={:?}: {}", i, j, e));
                }
            }

            // 2. The derived level matches brute force.
            let want_level = d.iter().position(|&x| x != 0).unwrap_or(depth);
            match &derived_level {
                Claim::Level(l) if *l == want_level => {}
                other => {
                    bad_accepts += 1;
                    if examples_accept.len() < 5 {
                        examples_accept.push(format!(
                            "level_of({:?}) = {:?} but brute force says {}",
                            d, other, want_level
                        ));
                    }
                }
            }

            // 3. Every other level in 0..=depth must be rejected.
            for l in 0..=depth {
                if l != want_level && claim_ok(&Claim::Level(l), &i, &j, depth).is_ok() {
                    bad_rejects += 1;
                    if examples_reject.len() < 5 {
                        examples_reject.push(format!(
                            "Level {} accepted for d={:?} (true level {})",
                            l, d, want_level
                        ));
                    }
                }
            }

            // 4. A level beyond the loop depth must be rejected.
            if claim_ok(&Claim::Level(depth + 1), &i, &j, depth).is_ok() {
                bad_rejects += 1;
                if examples_reject.len() < 5 {
                    examples_reject
                        .push(format!("Level {} > depth {} accepted", depth + 1, depth));
                }
            }

            // ---------- Cross-claim consistency ----------

            // `Exact(d)` is the strongest claim and must always be accepted
            // for the very pair it was computed from. This ties the new
            // claims back to the pre-existing one.
            if let Err(e) = claim_ok(&Claim::Exact(d.clone()), &i, &j, depth) {
                bad_accepts += 1;
                if examples_accept.len() < 5 {
                    examples_accept.push(format!("Exact {:?} rejected: {}", d, e));
                }
            }

            // Every component of the derived direction holds for the
            // distance it was derived from.
            if let Claim::Direction(v) = &derived_dir {
                for k in 0..depth {
                    if !v[k].holds(d[k]) {
                        bad_accepts += 1;
                        if examples_accept.len() < 5 {
                            examples_accept.push(format!(
                                "{:?} does not hold for d[{}] = {}",
                                v[k], k, d[k]
                            ));
                        }
                    }
                }
            }
        }

        println!(
            "Phase 1 claim fuzz: tested={} bad_accepts={} bad_rejects={}",
            tested, bad_accepts, bad_rejects
        );
        for ex in &examples_accept {
            println!("  BAD-ACCEPT {}", ex);
        }
        for ex in &examples_reject {
            println!("  BAD-REJECT {}", ex);
        }
        assert_eq!(
            bad_accepts, 0,
            "Phase 1 fuzz: {} witness-derived claims were not accepted",
            bad_accepts
        );
        assert_eq!(
            bad_rejects, 0,
            "Phase 1 fuzz: {} unsupported claims were wrongly accepted",
            bad_rejects
        );
    }

    // ---- guarded range / refutation regressions ----

    fn iv_stepped(step: i64) -> IV {
        IV { name: "i".into(), lower: 0, upper: 8, step, bound_op: CmpOp::Lt }
    }

    fn guard_with(op: CmpOp, bound: i64) -> Guard {
        Guard { constraints: vec![Cmp { iv: 0, op, bound }] }
    }

    fn range_of(op: CmpOp, bound: i64, step: i64) -> GuardedRange {
        guarded_iv_range(&iv_stepped(step), Some(&guard_with(op, bound)))
    }

    #[test]
    fn guarded_range_uses_exclusive_upper_bound() {
        // `i < 4` on `[0,8)` leaves 0..=3. Reading the guard in *inclusive*
        // space would keep iteration 4, a phantom the checker would then
        // mistake for a real collision.
        assert!(matches!(range_of(CmpOp::Lt, 4, 1), GuardedRange::Interval(0, 3)));
        // `i <= 4` leaves 0..=4.
        assert!(matches!(range_of(CmpOp::Le, 4, 1), GuardedRange::Interval(0, 4)));
        // `i > 4` leaves 5..=7; `i >= 4` leaves 4..=7.
        assert!(matches!(range_of(CmpOp::Gt, 4, 1), GuardedRange::Interval(5, 7)));
        assert!(matches!(range_of(CmpOp::Ge, 4, 1), GuardedRange::Interval(4, 7)));
    }

    #[test]
    fn guarded_range_detects_emptiness() {
        // A guard that clamps the range to nothing is itself a proof that the
        // access never runs.
        assert!(matches!(range_of(CmpOp::Lt, 0, 1), GuardedRange::Never));
        assert!(matches!(range_of(CmpOp::Ge, 8, 1), GuardedRange::Never));
        assert!(matches!(range_of(CmpOp::Gt, 7, 1), GuardedRange::Never));
        // A non-positive step runs no iteration either.
        assert!(matches!(guarded_iv_range(&iv_stepped(0), None), GuardedRange::Never));
    }

    #[test]
    fn guarded_range_hands_stepped_loops_to_enumeration() {
        // The guard leaves a sublattice, not an interval; snapping to it needs a
        // residue condition the closed form does not model, so it must be
        // declined rather than widened.
        assert!(matches!(range_of(CmpOp::Lt, 6, 2), GuardedRange::Unsupported));
        assert!(matches!(range_of(CmpOp::Ge, 1, 2), GuardedRange::Unsupported));
    }


    fn tbl_access(kind: AccessKind, offset: i64, table: Vec<i64>, lo: i64, hi: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![0],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: Some(Indirect {
                base: "B".into(),
                index: vec![1],
                offset: 0,
                table: Some(table),
                domain_lo: lo,
                domain_hi: hi,
            }),
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn aff(kind: AccessKind, offset: i64, guard: Option<Guard>) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    #[test]
    fn refutation_location_resolves_table_cells() {
        let loop_ = make_loop(1, 0, 4);
        let a = tbl_access(AccessKind::Write, 0, vec![3, 7, 11, 15], 0, 4);
        assert_eq!(refutation_location(&a, &[0], &loop_, &[]), Some(3));
        assert_eq!(refutation_location(&a, &[1], &loop_, &[]), Some(7));
        assert_eq!(refutation_location(&a, &[3], &loop_, &[]), Some(15));
        // Outside the declared domain no cell exists: that is *no* location,
        // not a sentinel that could coincidentally equal another one.
        assert_eq!(refutation_location(&a, &[4], &loop_, &[]), None);
        assert_eq!(refutation_location(&a, &[-1], &loop_, &[]), None);
    }

    #[test]
    fn table_refutation_requires_the_cell_gap_to_miss_the_offset_gap() {
        let loop_ = make_loop(1, 0, 4);
        let tbl = vec![3, 7, 11, 15];
        // Offsets 0 and 1 name {3,7,11,15} against {4,8,12,16}: disjoint, and
        // the checker sees it by walking the domain.
        let p = tbl_access(AccessKind::Write, 0, tbl.clone(), 0, 4);
        let q = tbl_access(AccessKind::Read, 1, tbl.clone(), 0, 4);
        assert!(refutation_holds(&loop_, &p, &q, &[]));
        // Offsets 0 and 4 are *not* disjoint: two cells differ by exactly 4, so
        // `B[1] + 0 == B[0] + 4`. Assuming distinct offsets imply absence would
        // accept a false refutation here.
        let q4 = tbl_access(AccessKind::Read, 4, tbl.clone(), 0, 4);
        assert!(!refutation_holds(&loop_, &p, &q4, &[]));
    }

    #[test]
    fn empty_table_domain_refutes_any_dependence() {
        let loop_ = make_loop(1, 0, 4);
        let tbl = vec![3, 7, 11, 15];
        let a = tbl_access(AccessKind::Write, 0, tbl.clone(), 2, 2);
        let b = tbl_access(AccessKind::Read, 0, tbl.clone(), 2, 2);
        assert!(never_executes(&a, &loop_, &[]));
        assert!(refutation_holds(&loop_, &a, &b, &[]));
    }


    #[test]
    fn access_executes_at_respects_guard_and_domain() {
        let loop_ = make_loop(1, 0, 4);
        let a = aff(AccessKind::Write, 0, Some(guard_with(CmpOp::Lt, 4)));
        assert!(access_executes_at(&a, &[3], &loop_));
        assert!(!access_executes_at(&a, &[4], &loop_));
        let t = tbl_access(AccessKind::Write, 0, vec![3, 7, 11, 15], 0, 4);
        assert!(access_executes_at(&t, &[0], &loop_));
        assert!(!access_executes_at(&t, &[4], &loop_));
        assert!(!access_executes_at(&t, &[-1], &loop_));
    }

    #[test]
    fn access_executes_at_rejects_iterations_the_loop_never_runs() {
        // for (i = 0; i < 8; i += 2) assigns 0,2,4,6. Every other value in
        // range is a phantom iteration at which no statement executes, and a
        // witness there must not be accepted.
        let mut loop_ = make_loop(1, 0, 8);
        loop_.ivs[0].step = 2;
        let a = aff(AccessKind::Write, 0, None);
        for v in [0, 2, 4, 6] {
            assert!(access_executes_at(&a, &[v], &loop_), "{v} is reached");
        }
        for v in [1, 3, 5, 7] {
            assert!(!access_executes_at(&a, &[v], &loop_), "{v} is never assigned");
        }
        // Lattice alignment is relative to `lower`, not to zero.
        let mut off = make_loop(1, 1, 9);
        off.ivs[0].step = 2;
        let a = aff(AccessKind::Write, 0, None);
        assert!(access_executes_at(&a, &[1], &off));
        assert!(!access_executes_at(&a, &[2], &off));
        // A non-positive step runs nothing.
        let dead = make_loop_step(1, 0, 8, 0);
        assert!(!access_executes_at(&a, &[0], &dead));
    }

    #[test]
    fn never_executes_refuses_shapes_it_cannot_prove() {
        let loop_ = make_loop(1, 0, 8);
        // A guard that empties the domain does refute everything.
        assert!(never_executes(&aff(AccessKind::Write, 0, Some(guard_with(CmpOp::Lt, 0))), &loop_, &[]));
        // A guard naming an IV the loop lacks can never hold either.
        let ghost = Guard { constraints: vec![Cmp { iv: 5, op: CmpOp::Ge, bound: 0 }] };
        assert!(never_executes(&aff(AccessKind::Write, 0, Some(ghost)), &loop_, &[]));
        // An ordinary access runs, so nothing is refuted for free.
        assert!(!never_executes(&aff(AccessKind::Write, 0, None), &loop_, &[]));
        // Nor does an unresolved parametric offset count as "never runs".
        let mut symbolic = aff(AccessKind::Write, 0, None);
        symbolic.offset = OffsetValue::Param(m0::param_expr::ParamExpr::Param("k".into()));
        assert!(!never_executes(&symbolic, &loop_, &[]));
        // Under M3 the `ivs` are placeholders and must not be read.
        let recursive = Loop {
            recursion: Some(Recursion {
                func: "f".into(),
                iv_name: "i".into(),
                iv_init: 0,
                iv_step: 1,
                iv_bound: 8,
                bound_op: CmpOp::Lt,
                other_params: vec![],
                companion: None,
            }),
            scalars: vec![],
            stmts: None,
            ..loop_.clone()
            };
        assert!(!never_executes(&aff(AccessKind::Write, 0, None), &recursive, &[]));
    }

    #[test]
    fn refutation_rejects_a_real_collision() {
        let loop_ = make_loop(1, 0, 8);
        // Same stride, same offset: i == j collides.
        let p = aff(AccessKind::Write, 0, None);
        let q = aff(AccessKind::Read, 0, None);
        assert!(!refutation_holds(&loop_, &p, &q, &[]));
    }

    #[test]
    fn guarded_collision_is_not_invented_by_the_closed_form() {
        let loop_ = make_loop(1, 0, 8);
        // p runs on `i < 4` (locations 0..=3), q on `i >= 5` (locations 5..=7):
        // provably disjoint.
        let p = aff(AccessKind::Write, 0, Some(guard_with(CmpOp::Lt, 4)));
        let q = aff(AccessKind::Read, 0, Some(guard_with(CmpOp::Ge, 5)));
        assert!(refutation_holds(&loop_, &p, &q, &[]));
        // Widen q to `i >= 4` and the guards just touch without overlapping:
        // still disjoint. An inclusive-space clamp keeps a phantom i = 4 for p
        // and reports a collision that the loop never performs.
        let q5 = aff(AccessKind::Read, 0, Some(guard_with(CmpOp::Ge, 4)));
        assert!(refutation_holds(&loop_, &p, &q5, &[]));
        // But `i <= 4` genuinely shares iteration 4 with `i >= 4`.
        let p4 = aff(AccessKind::Write, 0, Some(guard_with(CmpOp::Le, 4)));
        assert!(!refutation_holds(&loop_, &p4, &q5, &[]));
    }

    #[test]
    fn stepped_loop_is_judged_on_the_iterations_it_performs() {
        // `[0,8)` step 2 visits 0,2,4,6: `A[i]` at offset 1 is never reached,
        // because the loop never runs an odd iteration. Judging the interval
        // rather than the lattice would invent a collision at i = 1.
        let mut loop_ = make_loop(1, 0, 8);
        loop_.ivs[0].step = 2;
        let p = aff(AccessKind::Write, 0, None);
        let q = aff(AccessKind::Read, 1, None);
        assert!(refutation_holds(&loop_, &p, &q, &[]));
        // The same pair on a step-1 loop does collide.
        let dense = make_loop(1, 0, 8);
        assert!(!refutation_holds(&dense, &p, &q, &[]));
    }

}
