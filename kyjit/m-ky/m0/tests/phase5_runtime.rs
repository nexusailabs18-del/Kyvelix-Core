//! Phase 5 §5.9 (Ch. 7.6) — run-time dependence testing, against the **real**
//! `m0-check` binary.
//!
//! A run-time test is the one §5 output that is *not* a plain independence
//! claim: `A[i] = A[i-1]` is not independent, so no static certificate may say
//! it is. What can be certified is that the emitter identified the pair as a
//! versioning candidate and derived the right run-time predicate for it. These
//! tests pin exactly that: an accepted certificate is re-derived by the checker
//! from the loop's own coefficients, and a forged or out-of-shape one is
//! rejected — there is no arm that reads the claim as "independent".

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::emit;
use m0::certificate::{Certificate, Claim, RuntimeCheckLeg, Witness};
use m0::ir::{Access, AccessKind, IndexKind, OffsetValue, ScalarTy, ScalarVar, IV, Loop};

fn acc(kind: AccessKind, base: &str, index: i64, offset: i64) -> Access {
    Access {
        kind,
        base: base.into(),
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

fn var(n: &str) -> ScalarVar {
    ScalarVar { name: n.into(), ty: ScalarTy::I64 }
}

/// `for (i = 1; i < 9; i++) { A[i] = A[i-1]; }` — the reference's canonical
/// run-time-testing kernel (`A[I] = A[I-K] + B[I]` with `K = 1`).
fn race_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 1, 9, 1)],
        body: vec![
            acc(AccessKind::Write, "A", 1, 0),
            acc(AccessKind::Read, "A", 1, -1),
        ],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i")],
        stmts: None,
    }
}

/// `A[i] = A[i+1]` — a copy whose two accesses share a base and coefficient, so
/// the §5.9 shape still holds.
fn copy_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 1, 9, 1)],
        body: vec![
            acc(AccessKind::Write, "A", 1, 0),
            acc(AccessKind::Read, "A", 1, 1),
        ],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i")],
        stmts: None,
    }
}

/// Two different bases: no run-time test of *this* pair can exist, because the
/// anti-dependence the test would police is between the same array.
fn distinct_base_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 1, 9, 1)],
        body: vec![
            acc(AccessKind::Write, "A", 1, 0),
            acc(AccessKind::Read, "B", 1, 0),
        ],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i")],
        stmts: None,
    }
}

fn run_checker(tag: &str, loop_: &Loop, cert: &Certificate) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("m0-p5rt-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lp = dir.join("loop.json");
    let cp = dir.join("cert.json");
    std::fs::write(&lp, loop_.canonical_json()).unwrap();
    std::fs::write(&cp, serde_json::to_string(cert).unwrap()).unwrap();
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-check"));
    let out = Command::new(bin).arg(&lp).arg(&cp).output().expect("run m0-check");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let last = if text.is_empty() { err } else { text };
    std::fs::remove_dir_all(&dir).ok();
    (out.status.success(), last)
}

// ---------------------------------------------------------------------------
// The emitter and the canonical legs agree.
// ---------------------------------------------------------------------------

#[test]
fn the_canonical_run_time_check_is_non_straddling_or_byte_equal() {
    let l = race_loop();
    let legs = emit::runtime_check_legs(&l, 0, 1).expect("A[i]=A[i-1] is a §5.9 shape");
    assert_eq!(
        legs,
        vec![RuntimeCheckLeg::NonStraddling, RuntimeCheckLeg::ByteEqual],
        "the two conjunctive legs of `i >= k || A[i*es] == A[(i-k)*es]`"
    );
    // The certificate carries exactly those legs.
    let c = emit::runtime_check_certificate(&l, 0, 1, "RUNTIME").unwrap();
    match &c.claim {
        Claim::RuntimeCheck { legs: cl } => assert_eq!(cl, &legs),
        other => panic!("expected a RuntimeCheck claim, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Accept: the checker re-derives the same test from the loop.
// ---------------------------------------------------------------------------

#[test]
fn a_runtime_check_certificate_is_accepted() {
    let l = race_loop();
    let c = emit::runtime_check_certificate(&l, 0, 1, "RUNTIME")
        .expect("the anti-dependence is a run-time versioning candidate");
    let (ok, out) = run_checker("ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

// ---------------------------------------------------------------------------
// Reject: the asserted legs are not what the loop's accesses induce.
// ---------------------------------------------------------------------------

#[test]
fn a_runtime_check_with_a_dropped_leg_is_rejected() {
    let l = race_loop();
    let mut c = emit::runtime_check_certificate(&l, 0, 1, "RUNTIME").unwrap();
    // Assert only the non-straddling leg: a check that would license a
    // parallelism the byte-equality leg is needed to justify.
    if let Claim::RuntimeCheck { legs } = &mut c.claim {
        legs.retain(|k| *k == RuntimeCheckLeg::NonStraddling);
    }
    let (ok, out) = run_checker("dropped", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_runtime_check_the_pair_does_not_admit_is_rejected() {
    // A read of `B` and a write of `A`: the pair is not a same-array
    // anti-dependence, so the checker's rebuild finds no §5.9 shape.
    let l = distinct_base_loop();
    let c = Certificate {
        level: "RUNTIME".into(),
        loop_hash: l.hash(),
        pair: (0, 1),
        claim: Claim::RuntimeCheck {
            legs: vec![RuntimeCheckLeg::NonStraddling, RuntimeCheckLeg::ByteEqual],
        },
        witness: Witness::RuntimeCheckEvidence { pair: (0, 1) },
        checker: "m0-check".into(),
    };
    let (ok, out) = run_checker("noshape", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_witness_naming_a_different_pair_is_rejected() {
    let l = copy_loop();
    let mut c = emit::runtime_check_certificate(&l, 0, 1, "RUNTIME").unwrap();
    // The witness's pair must match the certificate's: a mismatch is a broken
    // certificate, not a claim about the loop.
    c.witness = Witness::RuntimeCheckEvidence { pair: (1, 0) };
    let (ok, out) = run_checker("pairmismatch", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_pair_outside_the_shape_is_not_a_candidate() {
    assert!(
        emit::runtime_check_certificate(&distinct_base_loop(), 0, 1, "RUNTIME").is_none(),
        "different bases are not a run-time versioning candidate"
    );
    assert!(emit::runtime_check_legs(&distinct_base_loop(), 0, 1).is_none());
}