//! Phase 8 end-to-end: the §8.10 `Interprocedural` claim is exercised by the
//! **real** `m0-check` binary.
//!
//! `Witness::IpaEvidence` carries the whole program; the checker re-runs the
//! relevant interprocedural analysis and re-derives the fact. The accepted
//! certificates prove the rebuild happens; the forged ones (a purity, MOD set,
//! formal value or summary the program does not entail) are rejected.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::{Certificate, Claim, Witness};
use m0::ipa::certificate::{interprocedural_certificate, IpaClaim};
use m0::ipa::constprop::AbstractValue;
use m0::ipa::program::{Call, Callee, IpaExpr, IpaProgram, IpaStmt, Proc, ProcId, Slot};
use m0::ir::purity::Purity;
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue};

fn acc(kind: AccessKind, c: i64, k: i64) -> Access {
    Access {
        kind,
        base: "A".into(),
        index: vec![c],
        elem_size: 8,
        offset: OffsetValue::Const(k),
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    }
}

/// Any valid loop; Phase 8 facts ride on the certificate's `loop_hash`.
fn carrier_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    }
}

fn run_checker(tag: &str, loop_: &Loop, cert: &Certificate) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("m0-p8-{}-{}", tag, std::process::id()));
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

fn cert_of(loop_: &Loop, program: &IpaProgram, claim: IpaClaim) -> Certificate {
    Certificate {
        level: "P8".into(),
        loop_hash: loop_.hash(),
        pair: m0::certificate::emit::evidence_pair(loop_),
        claim: Claim::Interprocedural(claim),
        witness: Witness::IpaEvidence { program: Box::new(program.clone()) },
        checker: "m0-check".into(),
    }
}

/// `main()` calls `g(5)`; `g(x)` writes its local from `x`.
fn sample_program() -> IpaProgram {
    IpaProgram::new(
        vec![],
        vec![
            Proc {
                name: "main".into(),
                params: vec![],
                locals: vec!["m".into()],
                stmts: vec![IpaStmt::Call(Call {
                    callee: Callee::Direct(ProcId(1)),
                    args: vec![IpaExpr::Const(5)],
                    ret: None,
                })],
                ret: None,
            },
            Proc {
                name: "g".into(),
                params: vec!["x".into()],
                locals: vec!["t".into()],
                stmts: vec![IpaStmt::Assign {
                    target: Slot::Local(0),
                    value: IpaExpr::Read(Slot::Param(0)),
                }],
                ret: None,
            },
        ],
        ProcId(0),
    )
}

#[test]
fn a_purity_certificate_is_accepted() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = interprocedural_certificate(
        &l,
        &prog,
        IpaClaim::Purity { proc: ProcId(1), purity: Purity::Pure },
        "P8",
    )
    .expect("g only writes a local");
    let (ok, out) = run_checker("pur-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_purity_claim_the_program_contradicts_is_rejected() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = cert_of(&l, &prog, IpaClaim::Purity { proc: ProcId(1), purity: Purity::Impure });
    let (ok, out) = run_checker("pur-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_mod_set_certificate_is_accepted() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = interprocedural_certificate(
        &l,
        &prog,
        IpaClaim::ModSet { proc: ProcId(1), symbols: vec![Slot::Local(0)] },
        "P8",
    )
    .expect("g writes its local");
    let (ok, out) = run_checker("mod-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_mod_set_claim_with_a_missing_symbol_is_rejected() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = cert_of(&l, &prog, IpaClaim::ModSet { proc: ProcId(1), symbols: vec![] });
    let (ok, out) = run_checker("mod-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_formal_value_certificate_is_accepted() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = interprocedural_certificate(
        &l,
        &prog,
        IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(5) },
        "P8",
    )
    .expect("g is called with 5");
    let (ok, out) = run_checker("fv-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_formal_value_claim_the_analysis_contradicts_is_rejected() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = cert_of(
        &l,
        &prog,
        IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(6) },
    );
    let (ok, out) = run_checker("fv-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_summary_certificate_is_accepted() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = interprocedural_certificate(
        &l,
        &prog,
        IpaClaim::Summary {
            proc: ProcId(1),
            read: vec![Slot::Param(0)],
            write: vec![Slot::Local(0)],
        },
        "P8",
    )
    .expect("g reads x and writes its local");
    let (ok, out) = run_checker("sum-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_summary_claim_missing_a_symbol_is_rejected() {
    let l = carrier_loop();
    let prog = sample_program();
    let c = cert_of(
        &l,
        &prog,
        IpaClaim::Summary { proc: ProcId(1), read: vec![], write: vec![Slot::Local(0)] },
    );
    let (ok, out) = run_checker("sum-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}
