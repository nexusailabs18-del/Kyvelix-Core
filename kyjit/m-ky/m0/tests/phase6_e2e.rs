//! Phase 6 end-to-end: the §6.2 `ArrayRegion` and `FunctionSummary` claims are
//! exercised by the **real** `m0-check` binary.
//!
//! `Witness::ArrayRegionEvidence` carries the loop and makes the checker
//! re-derive the region / summary from scratch. The accepted certificates prove
//! the rebuild happens; the forged ones (a region that under-approximates the
//! access, a summary missing a region the loops really touches) are rejected by
//! the checker's own recomputation.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::emit;
use m0::certificate::{Certificate, Claim, Witness};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue};
use m0::region::Region;

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

/// `A[i] = A[i+1]` over `i = 0..8`.
fn copy_loop() -> Loop {
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
    let dir = std::env::temp_dir().join(format!("m0-p6-{}-{}", tag, std::process::id()));
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

fn cert_of(loop_: &Loop, claim: Claim) -> Certificate {
    Certificate {
        level: "P6".into(),
        loop_hash: loop_.hash(),
        pair: emit::evidence_pair(loop_),
        claim,
        witness: Witness::ArrayRegionEvidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".into(),
    }
}

// ---------------------------------------------------------------------------
// Claim::ArrayRegion
// ---------------------------------------------------------------------------

#[test]
fn an_array_region_certificate_is_accepted() {
    let l = copy_loop();
    let c = emit::array_region_certificate(&l, 0, emit::evidence_pair(&l), "REGION")
        .expect("A[i] has a derivable region");
    let (ok, out) = run_checker("reg-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_region_that_under_approximates_the_access_is_rejected() {
    // The access A[i] over [0,8) touches [0:7]; claiming only [0:3] is a
    // fabrication the rebuilt region does not fit inside.
    let l = copy_loop();
    let c = cert_of(&l, Claim::ArrayRegion { access: 0, region: Region::span(0, 3) });
    let (ok, out) = run_checker("reg-small", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn a_region_that_over_approximates_is_accepted() {
    // The checker requires the derived box to be *contained in* the claim: a
    // wider claim is sound (it says "no more than this"), so it is accepted.
    let l = copy_loop();
    let c = cert_of(&l, Claim::ArrayRegion { access: 0, region: Region::span(-100, 100) });
    let (ok, out) = run_checker("reg-big", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

// ---------------------------------------------------------------------------
// Claim::FunctionSummary
// ---------------------------------------------------------------------------

#[test]
fn a_function_summary_certificate_is_accepted() {
    let l = copy_loop();
    let c = emit::function_summary_certificate(&l, emit::evidence_pair(&l), "SUMMARY")
        .expect("a non-empty loop has a summary");
    let (ok, out) = run_checker("sum-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_summary_missing_a_region_is_rejected() {
    let l = copy_loop();
    let s = m0::region::loop_summary(&l);
    // Drop every region: the claim then says the loop touches nothing, which
    // the rebuild contradicts. Exact agreement is required, so this is a reject.
    let c = cert_of(
        &l,
        Claim::FunctionSummary {
            func: "loop:test".into(),
            may_use: vec![],
            may_mod: s.may_mod,
            kill: s.kill,
        },
    );
    let (ok, out) = run_checker("sum-empty", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}