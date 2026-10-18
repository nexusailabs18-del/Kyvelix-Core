//! M12 — dynamic bounds.
//!
//! Handles loops whose IV bounds are runtime parameters:
//!
//! ```text
//! for (i = f; i < g; i++) { A[i] = A[i + 1]; }
//! ```
//!
//! where `f` and `g` are parameters resolved from an environment. The
//! dispatcher routes to `decide_with_env`, which:
//!   1. Resolves the `DynamicIV` bounds against `env` to standard `IV`s.
//!   2. Builds a standard `Loop`.
//!   3. Delegates to `decide` (the existing dispatcher) — which runs
//!      M0–M11 as normal on the resolved loop.
//!   4. Wraps the certificate in `Witness::WithEnv` so the checker can
//!      replay the resolution against the same environment.
//!
//! **No changes to `IV`, `Access`, `Loop`, or any existing level.** This
//! module is purely additive: a new `DynamicLoop` type resolved before
//! dispatch, a new witness wrapper already present from M9.

use super::{decide, Decision};
use crate::certificate::{Certificate, Witness};
use crate::ir::*;

/// M12 entry point. Resolves dynamic bounds against `env`, then delegates
/// to the standard dispatcher on the resolved loop.
pub fn decide_with_env(
    dyn_loop: &DynamicLoop,
    env: &[(String, i64)],
    p: usize,
    q: usize,
) -> (Decision, Option<Certificate>) {
    if !dyn_loop.has_parametric_bounds() {
        return (
            Decision::Unknown("M12: no parametric bounds".into()),
            None,
        );
    }

    let resolved = match dyn_loop.resolve(env) {
        Some(l) => l,
        None => {
            return (
                Decision::Unknown("M12: failed to resolve bounds against env".into()),
                None,
            )
        }
    };

    // Sanity: resolved bounds must be non-empty.
    for iv in &resolved.ivs {
        if iv.lower >= iv.upper {
            return (Decision::Unknown("M12: resolved empty domain".into()), None);
        }
    }

    // Delegate to the standard dispatcher on the resolved loop.
    let (d, c) = decide(&resolved, p, q);

    // Wrap the certificate in WithEnv so the checker can replay.
    (d, c.map(|cert| wrap_with_env(cert, env)))
}

fn wrap_with_env(mut cert: Certificate, env: &[(String, i64)]) -> Certificate {
    cert.level = "M12".to_string();
    cert.witness = Witness::WithEnv {
        env: env.to_vec(),
        inner: Box::new(cert.witness),
    };
    cert
}

/// Phase 5 §5.1.6 — dynamic bounds (Wolfe Ch. 7.5 extension).
///
/// The §5.1.6 algorithm:
///
/// ```text
/// procedure DynamicBounds(loop, env):
///     lower_expr = parse_bound(loop.lower)
///     upper_expr = parse_bound(loop.upper)
///     l = eval(lower_expr, env)
///     u = eval(upper_expr, env)
///     ... bounded enumeration for the dependence decision ...
/// ```
///
/// That is exactly [`decide_with_env`]'s behaviour — resolve the `DynamicIV`
/// bounds against `env`, then decide on the resulting ordinary loop — so this
/// entry point is the spec's name for it. It exists separately because §5.1.6
/// is specified against arbitrary `Loop::ivs[k].lower/upper` expressions, and
/// naming it lets a caller reach the dynamic path without knowing M12's
/// internal `DynamicLoop` type.
///
/// Returns the same `(Decision, Option<Certificate>)` pair, with the
/// certificate wrapped in [`Witness::WithEnv`] so `m0-check` replays the
/// substitution against the identical environment.
pub fn decide_dynamic(
    dyn_loop: &DynamicLoop,
    env: &[(String, i64)],
    p: usize,
    q: usize,
) -> (Decision, Option<Certificate>) {
    decide_with_env(dyn_loop, env, p, q)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param_expr::ParamExpr;

    fn dyn_iv(name: &str, lo: BoundValue, hi: BoundValue) -> DynamicIV {
        DynamicIV { name: name.into(), lower: lo, upper: hi, step: 1 }
    }

    fn acc(kind: AccessKind, index: i64, offset: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![index],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn make_dyn_loop(lo: BoundValue, hi: BoundValue, body: Vec<Access>) -> DynamicLoop {
        DynamicLoop {
            depth: 1,
            ivs: vec![dyn_iv("i", lo, hi)],
            body,
        }
    }

    #[test]
    fn dyn_iv_resolve_const() {
        let iv = dyn_iv("i", BoundValue::Const(0), BoundValue::Const(32));
        let env = vec![];
        let r = iv.resolve(&env).unwrap();
        assert_eq!((r.lower, r.upper, r.step), (0, 32, 1));
    }

    #[test]
    fn dyn_iv_resolve_param() {
        let iv = dyn_iv("i", BoundValue::Const(0), BoundValue::Param("N".into()));
        let env = vec![("N".into(), 32)];
        let r = iv.resolve(&env).unwrap();
        assert_eq!((r.lower, r.upper, r.step), (0, 32, 1));
    }

    #[test]
    fn dyn_iv_resolve_missing_param() {
        let iv = dyn_iv("i", BoundValue::Const(0), BoundValue::Param("N".into()));
        let env: Vec<(String, i64)> = vec![];
        assert!(iv.resolve(&env).is_none());
    }

    #[test]
    fn decide_exact_on_resolved_loop() {
        // for (i = 0; i < N; i++) A[i] = A[i+1] with N = 32.
        // Resolved: [0, 32), distance -1.
        let l = make_dyn_loop(
            BoundValue::Const(0),
            BoundValue::Param("N".into()),
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
        );
        let env = vec![("N".into(), 32)];
        let (d, cert) = decide_with_env(&l, &env, 0, 1);
        match d {
            Decision::Exact(v) => assert_eq!(v, vec![-1]),
            other => panic!("expected Exact, got {:?}", other),
        }
        let c = cert.expect("cert");
        assert_eq!(c.level, "M12");
        assert!(matches!(c.witness, Witness::WithEnv { .. }));
    }

    #[test]
    fn decide_unknown_on_missing_env() {
        let l = make_dyn_loop(
            BoundValue::Const(0),
            BoundValue::Param("N".into()),
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
        );
        let env: Vec<(String, i64)> = vec![];
        let (d, _) = decide_with_env(&l, &env, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn decide_none_on_resolved_disjoint() {
        // for (i = 0; i < N; i++) A[i] = A[i + 100] with N = 10.
        // Resolved: [0, 10) vs [100, 110) — disjoint → None.
        let l = make_dyn_loop(
            BoundValue::Const(0),
            BoundValue::Param("N".into()),
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 100),
            ],
        );
        let env = vec![("N".into(), 10)];
        let (d, cert) = decide_with_env(&l, &env, 0, 1);
        assert!(matches!(d, Decision::None));
        // M0 may return None without a certificate (the fast-reject path
        // for disjoint affine accesses). M12 accepts this: a None claim
        // without a witness is the M-KY convention. If a certificate is
        // produced, it must be labelled M12.
        if let Some(c) = cert {
            assert_eq!(c.level, "M12");
        }
    }

    #[test]
    fn decide_unknown_on_empty_domain() {
        // for (i = N; i < 0; i++) — never executes.
        let l = make_dyn_loop(
            BoundValue::Param("N".into()),
            BoundValue::Const(0),
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
        );
        let env = vec![("N".into(), 32)];
        let (d, _) = decide_with_env(&l, &env, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }
    // ================================================================
    // M12 property fuzz: random DynamicLoop with parametric bounds,
    // random env, compare decide_with_env against brute force over
    // the resolved IV range.
    // ================================================================

    fn xorshift12(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn rand_bound(state: &mut u64) -> BoundValue {
        if xorshift12(state) % 2 == 0 {
            BoundValue::Const((xorshift12(state) % 16) as i64)
        } else {
            BoundValue::Param("n".into())
        }
    }

    #[test]
    fn fuzz_m12_property() {
        let mut state: u64 = 0xa5a5a5a5_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..2000 {
            // Random env value for "n".
            let n_val = (xorshift12(&mut state) % 24) as i64;
            let env = vec![("n".to_string(), n_val)];

            let lower = rand_bound(&mut state);
            let upper = rand_bound(&mut state);

            // Skip if fully const (M12 would return Unknown for no parametric bounds).
            let has_param = !lower.is_const() || !upper.is_const();
            if !has_param { continue; }

            let off_p = (xorshift12(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift12(&mut state) % 5) as i64 - 2;

            let dl = DynamicLoop {
                depth: 1,
                ivs: vec![DynamicIV {
                    name: "i".into(),
                    lower: lower.clone(),
                    upper: upper.clone(),
                    step: 1,
                }],
                body: vec![
                    Access {
                        kind: AccessKind::Write, base: "A".into(),
                        index: vec![1], elem_size: 8,
                        offset: OffsetValue::Const(off_p),
                        expr: None, indirect: None, guard: None, range: None,
                        index_kind: IndexKind::Affine,
                    },
                    Access {
                        kind: AccessKind::Read, base: "A".into(),
                        index: vec![1], elem_size: 8,
                        offset: OffsetValue::Const(off_q),
                        expr: None, indirect: None, guard: None, range: None,
                        index_kind: IndexKind::Affine,
                    },
                ],
            };

            // Resolve bounds for the ground truth.
            let resolved_lo = match lower.resolve(&env) { Some(v) => v, None => continue };
            let resolved_hi = match upper.resolve(&env) { Some(v) => v, None => continue };

            // Ground truth.
            let expected = if resolved_lo >= resolved_hi {
                // Empty domain → Unknown.
                None
            } else {
                let mut wp = std::collections::HashSet::new();
                for i in resolved_lo..resolved_hi {
                    wp.insert(1 * i + off_p);
                }
                let mut found = false;
                for j in resolved_lo..resolved_hi {
                    if wp.contains(&(1 * j + off_q)) {
                        found = true;
                        break;
                    }
                }
                Some(found)
            };

            let (d, _) = decide_with_env(&dl, &env, 0, 1);
            tested += 1;

            let ok = match (expected, &d) {
                (None, Decision::Unknown(_)) => true,
                (None, _) => false,
                (Some(true), Decision::Exact(_)) => true,
                (Some(true), Decision::Unknown(_)) => { conservative += 1; true }
                (Some(true), _) => false,
                (Some(false), Decision::None) => true,
                (Some(false), Decision::Unknown(_)) => { conservative += 1; true }
                (Some(false), _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "lower={:?} upper={:?} n={} off_p={} off_q={} expected={:?} got={:?}",
                        lower, upper, n_val, off_p, off_q, expected, d
                    ));
                }
            }
        }

        println!("M12 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M12 fuzz found {} unsound cases", unsound);
    }

}
