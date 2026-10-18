//! Phase 2 end-to-end: M1 emits a `Claim::Parametric(DecisionTree)`
//! certificate and the **real** `m0-check` binary verifies it.
//!
//! Unit tests can only prove that the emitter and the checker agree with each
//! other. This file proves they agree with the compiled binary that ships, by
//! serialising a certificate to JSON, running `m0-check` over it, and asserting
//! ACCEPT or REJECT. The rejected cases are the point: they show the checker
//! refuses a forged terminal index, an environment that cannot be evaluated,
//! and a loop file that is not the one the certificate was built for, instead
//! of trusting the claim it was handed.
//!
//! The accepted cases show the other half of what makes a parametric
//! certificate worth having: the *same* certificate JSON validates at many
//! different parameter environments, which no concrete `IntSolution` witness can
//! do.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::{Certificate, Claim, Witness};
use m0::decide::m1::{decide_parametric, ParamEnv};
use m0::decide::Decision;
use m0::ir::{Access, AccessKind, Bound, IndexKind, IVParam, LoopParam, OffsetValue};
use m0::param_expr::ParamExpr;

/// `for (i = 0; i < N; i++) { A[i + P] = A[i]; }` — same stride, parametric
/// distance `P`, so the dependence exists exactly when `P` divides (trivially)
/// and fits inside the range.
fn param_loop() -> LoopParam {
    let acc = |kind: AccessKind, offset: OffsetValue| Access {
        kind,
        base: "A".into(),
        index: vec![1],
        elem_size: 8,
        offset,
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    };
    LoopParam {
        depth: 1,
        ivs: vec![IVParam {
            name: "i".into(),
            lower: Bound::Const(0),
            upper: Bound::Param("N".into()),
            step: 1,
        }],
        body: vec![
            acc(
                AccessKind::Write,
                OffsetValue::Param(ParamExpr::Param("P".into())),
            ),
            acc(AccessKind::Read, OffsetValue::Const(0)),
        ],
    }
}

fn env(n: i64, p: i64) -> ParamEnv {
    vec![("N".to_string(), n), ("P".to_string(), p)]
}

/// Emit the parametric certificate for one environment.
fn emit(lp: &LoopParam, e: &ParamEnv) -> (Decision, Certificate) {
    let (d, c) = decide_parametric(lp, e, 0, 1).expect("this shape is certifiable");
    (d, c.expect("a certifiable case always carries a certificate"))
}

/// Run the real checker binary on (loop, cert). Returns (exit_ok, last line).
fn run_checker(tag: &str, loop_json: &str, cert_json: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("m0-p2-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lp = dir.join("loop.json");
    let cp = dir.join("cert.json");
    std::fs::write(&lp, loop_json).unwrap();
    std::fs::write(&cp, cert_json).unwrap();
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-check"));
    let out = Command::new(bin).arg(&lp).arg(&cp).output().expect("run m0-check");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let last = if text.is_empty() { err } else { text };
    std::fs::remove_dir_all(&dir).ok();
    (out.status.success(), last)
}

#[test]
fn parametric_dependence_is_accepted() {
    let lp = param_loop();
    let e = env(1000, 4);
    let (d, cert) = emit(&lp, &e);
    assert!(
        matches!(&d, Decision::Exact(v) if v == &vec![4]),
        "expected distance 4, got {d:?}"
    );
    let resolved = lp.resolve(&e).unwrap();
    let (ok, out) = run_checker(
        "accept",
        &resolved.canonical_json(),
        &serde_json::to_string(&cert).unwrap(),
    );
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn one_tree_validates_at_many_environments() {
    // The property a concrete witness cannot have: one *tree* carries a whole
    // region of parameter space. Its leaves are symbolic, so the identical
    // claim -- byte for byte -- is verified at many different distances, each
    // with its own colliding pair.
    //
    // The region is the set of P values for one fixed loop instance: the range
    // guards compare the distance against `hi - lo`, which is a property of the
    // resolved loop, and the certificate's hash already binds it to exactly one
    // such instance. So N stays put here and P varies -- which is also the only
    // way a `loop_hash` can be reused.
    let lp = param_loop();
    let n = 1000i64;
    let resolved = lp.resolve(&env(n, 4)).unwrap();
    let loop_json = resolved.canonical_json();

    let (_, base) = emit(&lp, &env(n, 4));
    let tree_json = serde_json::to_string(&base.claim).unwrap();

    for p in [4i64, 9, -7, 1, 999, -999] {
        let e = env(n, p);
        let (d, cert) = emit(&lp, &e);
        // the same tree, unchanged, must be what this certificate claims
        assert_eq!(serde_json::to_string(&cert.claim).unwrap(), tree_json, "P={p}");
        assert!(
            matches!(&d, Decision::Exact(v) if v == &vec![p]),
            "P={p}: {d:?}"
        );
        let idx = match &cert.witness {
            Witness::ParametricSolution { leaf, .. } => *leaf,
            other => panic!("{other:?}"),
        };
        // Re-issue the claim with a different P in the witness environment.
        let retargeted = Certificate {
            witness: Witness::ParametricSolution {
                env: e.clone(),
                leaf: idx,
            },
            ..cert.clone()
        };
        let (ok, out) = run_checker(
            &format!("multi{p}"),
            &loop_json,
            &serde_json::to_string(&retargeted).unwrap(),
        );
        assert!(ok && out.starts_with("ACCEPT"), "P={p}: {out}");
    }

    // A P whose distance cannot fit the range reaches a refutation terminal,
    // and the checker confirms it by enumeration -- same tree, same loop file.
    let e_far = env(n, 1001);
    let (d_far, cert_far) = emit(&lp, &e_far);
    assert!(matches!(d_far, Decision::None), "P=1001: {d_far:?}");
    let (ok, out) = run_checker(
        "multi_far",
        &loop_json,
        &serde_json::to_string(&cert_far).unwrap(),
    );
    assert!(ok && out.starts_with("ACCEPT"), "refutation at P=1001: {out}");
}

#[test]
fn parametric_refutation_is_accepted() {
    // P = 5000 cannot fit a range of 300 iterations, so the tree reaches a
    // NoDependence terminal. The checker confirms it by enumeration before
    // accepting, so this is not a rubber stamp.
    let lp = param_loop();
    let e = env(300, 5000);
    let (d, cert) = emit(&lp, &e);
    assert!(matches!(d, Decision::None), "expected None, got {d:?}");
    let resolved = lp.resolve(&e).unwrap();
    let (ok, out) = run_checker(
        "refute",
        &resolved.canonical_json(),
        &serde_json::to_string(&cert).unwrap(),
    );
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn forged_terminal_index_is_rejected() {
    // The certificate is valid apart from the witness naming a different
    // terminal than its own environment selects. The checker resolves the tree
    // itself, so a renamed terminal cannot redirect verification to a leaf that
    // does not apply -- the mismatch has to be reported.
    let lp = param_loop();
    let e = env(1000, 4);
    let (_d, cert) = emit(&lp, &e);
    let real = match &cert.witness {
        Witness::ParametricSolution { leaf, .. } => *leaf,
        other => panic!("{other:?}"),
    };
    let forged = Certificate {
        witness: Witness::ParametricSolution {
            env: e.clone(),
            leaf: real + 1,
        },
        ..cert.clone()
    };
    let resolved = lp.resolve(&e).unwrap();
    let (ok, out) = run_checker(
        "forge",
        &resolved.canonical_json(),
        &serde_json::to_string(&forged).unwrap(),
    );
    assert!(!ok && out.starts_with("REJECT"), "expected REJECT, got: {out}");
    assert!(
        out.contains("terminal"),
        "should name the terminal mismatch: {out}"
    );
}

#[test]
fn unevaluable_environment_is_rejected_not_guessed() {
    // Dropping P from the environment makes the tree's conditions impossible to
    // evaluate. That must be a rejection, never a silent fall-through to the
    // else-branch of every test, which would pick an arbitrary witness.
    let lp = param_loop();
    let e = env(1000, 4);
    let (_d, cert) = emit(&lp, &e);
    let stripped = Certificate {
        witness: Witness::ParametricSolution {
            env: vec![("N".to_string(), 1000i64)],
            leaf: 0,
        },
        ..cert.clone()
    };
    let resolved = lp.resolve(&e).unwrap();
    let (ok, out) = run_checker(
        "partial",
        &resolved.canonical_json(),
        &serde_json::to_string(&stripped).unwrap(),
    );
    assert!(!ok && out.starts_with("REJECT"), "expected REJECT, got: {out}");
    assert!(out.contains("no terminal"), "should say why: {out}");
}

#[test]
fn mismatched_claim_and_witness_are_rejected() {
    // A ParametricSolution witness cannot carry an Exact claim: the two shapes
    // are verified by different code paths, and pairing them would let one
    // path's checks stand in for the other's.
    let lp = param_loop();
    let e = env(1000, 4);
    let (_d, cert) = emit(&lp, &e);
    let mixed = Certificate {
        claim: Claim::Exact(vec![4]),
        ..cert.clone()
    };
    let resolved = lp.resolve(&e).unwrap();
    let (ok, out) = run_checker(
        "mixed",
        &resolved.canonical_json(),
        &serde_json::to_string(&mixed).unwrap(),
    );
    assert!(!ok && out.starts_with("REJECT"), "expected REJECT, got: {out}");
}

#[test]
fn refutation_pointed_at_a_colliding_loop_is_rejected() {
    // The sharpest negative, and the one the tree's own logic cannot catch: a
    // genuine refutation certificate (its tree really does reach a
    // NoDependence terminal for its environment) attached to a *different*
    // resolved instance of the same loop, one in which the two accesses do
    // collide. The hash is recomputed so the hash gate passes; only
    // enumeration can see through it. Were it not for that check, a parametric
    // "no dependence" would be accepted on the tree's say-so alone.
    let lp = param_loop();
    // N = 5, P = 8: a distance of 8 cannot fit four iterations, so this is a
    // real refutation.
    let (d_refute, cert) = emit(&lp, &env(5, 8));
    assert!(
        matches!(d_refute, Decision::None),
        "precondition: expected a refutation, got {d_refute:?}"
    );

    // N = 10, same P: now j - i = 8 *is* attainable (i = 0, j = 8).
    let colliding = lp.resolve(&env(10, 8)).unwrap();
    let lying = Certificate {
        loop_hash: colliding.hash(),
        ..cert.clone()
    };
    let (ok, out) = run_checker(
        "lie",
        &colliding.canonical_json(),
        &serde_json::to_string(&lying).unwrap(),
    );
    assert!(
        !ok && out.starts_with("REJECT"),
        "a refutation must not pass against a colliding loop: {out}"
    );
    assert!(
        out.contains("colliding pair exists"),
        "should report the collision it found: {out}"
    );
}
