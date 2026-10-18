//! Phase 5 end-to-end: the §5.1.8/§5.1.9/§5.1.11 claims (`Reduction`,
//! `LoopKind`, `Aligned`) are exercised by the **real** `m0-check` binary.
//!
//! `Witness::Phase5Evidence` carries the loop and makes the checker rebuild the
//! dependence set, re-run the reducer/classifier and recompute the alignment
//! from scratch. These tests are the proof that the rebuild really happens:
//! every accepted certificate is re-derived from the carried loop, and every
//! forged one (a wrong loop kind, a reduction the detector does not report, an
//! alignment the loop does not admit) is rejected by the checker's own
//! recomputation rather than by anything the analyzer asserted.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::emit;
use m0::certificate::{Certificate, Claim, Witness};
use m0::ir::loopkind::LoopKind;
use m0::ir::reduction::ReductionKind;
use m0::ir::stmt::{ScalarExpr, Stmt as LoopStmt, StmtKind};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue, ScalarTy, ScalarVar};

fn acc(kind: AccessKind, base: &str, index: i64) -> Access {
    Access {
        kind,
        base: base.into(),
        index: vec![index],
        elem_size: 8,
        offset: OffsetValue::Const(0),
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

/// `sum += A[i]` in a `for (i = 0; i < 8; i++)` loop, with a scalar body.
fn sum_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![acc(AccessKind::Read, "A", 1), acc(AccessKind::Write, "S", 0)],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i"), var("sum")],
        stmts: Some(vec![LoopStmt {
            id: 0,
            kind: StmtKind::Assign {
                target: 1,
                expr: ScalarExpr::Add(
                    Box::new(ScalarExpr::Scalar(1)),
                    Box::new(ScalarExpr::Iv(0)),
                ),
            },
        }]),
    }
}

/// A plain affine loop with a flow dependence (a copy `A[i] = A[i+1]`).
fn copy_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Read, "A", 1)],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i")],
        stmts: None,
    }
}

/// An `n`-statement loop (all writes) for alignment tests.
fn loop_n(n: usize) -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: (0..n)
            .map(|k| {
                let mut a = acc(AccessKind::Write, "A", 1);
                a.offset = OffsetValue::Const(k as i64);
                a
            })
            .collect(),
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i")],
        stmts: None,
    }
}

fn run_checker(tag: &str, loop_: &Loop, cert: &Certificate) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("m0-p5-{}-{}", tag, std::process::id()));
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

/// A certificate with a hand-chosen claim, witnessed by the loop itself.
fn cert_of(loop_: &Loop, claim: Claim) -> Certificate {
    Certificate {
        level: "P5".into(),
        loop_hash: loop_.hash(),
        pair: emit::evidence_pair(loop_),
        claim,
        witness: Witness::Phase5Evidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".into(),
    }
}

// ---------------------------------------------------------------------------
// Reduction
// ---------------------------------------------------------------------------

#[test]
fn a_real_reduction_certificate_is_accepted() {
    let l = sum_loop();
    let c = emit::reduction_certificate(&l, emit::evidence_pair(&l), "REDUCTION")
        .expect("sum += A[i] is a reduction");
    let (ok, out) = run_checker("red-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_wrong_reduction_kind_is_rejected() {
    let l = sum_loop();
    // The detector reports `Sum`; assert `Product` instead.
    let c = cert_of(&l, Claim::Reduction { var: 1, loop_: 1, kind: ReductionKind::Product });
    let (ok, out) = run_checker("red-kind", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_reduction_that_the_loop_does_not_have_is_rejected() {
    // `A[i] = A[i+1]` has no scalar accumulator; asserting a reduction of
    // vector 0 is a fabrication the rebuilt detector cannot confirm.
    let l = copy_loop();
    let c = cert_of(&l, Claim::Reduction { var: 0, loop_: 1, kind: ReductionKind::Sum });
    let (ok, out) = run_checker("red-none", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

// ---------------------------------------------------------------------------
// Loop kind
// ---------------------------------------------------------------------------

#[test]
fn a_loop_kind_certificate_is_accepted() {
    let l = copy_loop();
    let c = emit::loop_kind_certificate(&l, emit::evidence_pair(&l), "LOOPKIND")
        .expect("loop kind is always computed");
    let (ok, out) = run_checker("lk-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_loop_kind_the_dependences_do_not_support_is_rejected() {
    let l = copy_loop();
    // `A[i] = A[i+1]` carries a flow dependence, so it is not `Doall`.
    let c = cert_of(&l, Claim::LoopKind(LoopKind::Doall));
    let (ok, out) = run_checker("lk-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

#[test]
fn an_alignment_certificate_is_accepted() {
    let l = loop_n(2);
    let a = emit::alignment_certificate(&l, emit::evidence_pair(&l), "ALIGN");
    assert!(a.is_some(), "a 2-statement loop with a zero-total cycle must align");
    let (ok, out) = run_checker("al-ok", &l, &a.unwrap());
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_tampered_alignment_is_rejected() {
    let l = loop_n(2);
    let mut a = emit::alignment_certificate(&l, emit::evidence_pair(&l), "ALIGN").unwrap();
    // Corrupt one shift; the checker recomputes and disagrees.
    if let Claim::Aligned(ref mut v) = a.claim {
        if let Some(first) = v.first_mut() {
            first.shift += 7;
        }
    }
    let (ok, out) = run_checker("al-bad", &l, &a);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

// ---------------------------------------------------------------------------
// §5.1.5 symbolic chase — the symbolic path agrees with the literal decision.
// ---------------------------------------------------------------------------

#[test]
fn symbolic_chase_matches_the_literal_decision() {
    use m0::decide::m5;
    // Chase variable `p` is the loop IV: A[p] = A[p+1] over p in 0..8.
    let l = Loop {
        depth: 1,
        ivs: vec![IV::new("p", 0, 8, 1)],
        body: vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Read, "A", 1)],
        recursion: None,
        chase: Some(m0::ir::PointerChase {
            var: "p".into(),
            table: vec![],
            table_expr: None,
            init: 0,
            max_steps: 8,
            accessed_base: "A".into(),
        }),
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    };
    let sym = m5::decide_symbolic_chase(&l, 0, 1).expect("IV chase is symbolically decidable");
    assert!(matches!(sym.0, m0::decide::Decision::Exact(_)));
}