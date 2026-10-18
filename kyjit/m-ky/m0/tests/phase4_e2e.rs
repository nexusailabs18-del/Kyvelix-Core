//! Phase 4 end-to-end: the two §4.3 checker arms are exercised by the **real**
//! `m0-check` binary.
//!
//! `Witness::ScalarDependenceEvidence` and `Witness::TripCountEvidence` each
//! carry a CFG and make the checker rebuild everything from it — the chains,
//! `scalar_dd`, `induct`, `trip_count`. These tests are the proof that the
//! rebuild really happens: every accepted certificate is re-derived from the
//! carried CFG, and every forged one (a wrong distance, a wrong trip count, a
//! nominated variable that is not an IV, a claim that disagrees with its own
//! evidence) is rejected by the checker's own recomputation rather than by
//! anything the analyzer asserted.
//!
//! The CFG shapes and the facts the analyses produce on them were measured
//! against `src/fud` before these expectations were written, not guessed.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::{Certificate, Claim, ScalarDistance, Witness};
use m0::ir::cfg::{Block, CFG};
use m0::ir::scalar::{DefId, ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar, UseId};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue, Stmt};

/// Minimal loop used as the "file on disk". Its hash binds the certificate; the
/// carried CFG is the ground truth the checker rebuilds from. Same shape as the
/// Phase 3 e2e harness, and deliberately *not* the same as the carried CFG: the
/// checker verifies the hash of the loop it was handed, then reasons entirely
/// from the CFG inside the witness.
fn disk_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![Access {
            kind: AccessKind::Write,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }],
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,}
}

fn var(n: &str) -> ScalarVar {
    ScalarVar { name: n.into(), ty: ScalarTy::I64 }
}

/// One statement: the reads `rhs` exposes, then `extra`, then a single def of
/// `def` carrying `rhs`. Numbering matches `tests/phase4_fuzz.rs::one_stmt`.
fn st(id: usize, rhs: ScalarRhs, extra: Vec<usize>, def: usize) -> Stmt {
    let mut reads = Vec::new();
    rhs.collect_vars(&mut reads);
    reads.extend(extra);
    Stmt {
        id,
        uses: reads
            .into_iter()
            .enumerate()
            .map(|(order, var)| ScalarUse { var, stmt: id, order })
            .collect(),
        defs: vec![ScalarDef { var: def, stmt: id, order: 0, rhs }],
    }
}

/// `entry 0 → header 1 → latch 2 → header 1; header 1 → exit 3`.
fn shell(init: Vec<Stmt>, header: Vec<Stmt>, latch: Vec<Stmt>) -> CFG {
    let cfg = CFG {
        vars: vec![var("s")],
        blocks: vec![
            Block { id: 0, preds: vec![], succs: vec![1], stmts: init, phi: vec![], upsilon: vec![], guards: vec![] },
            Block { id: 1, preds: vec![0, 2], succs: vec![2, 3], stmts: header, phi: vec![], upsilon: vec![], guards: vec![] },
            Block { id: 2, preds: vec![1], succs: vec![1], stmts: latch, phi: vec![], upsilon: vec![], guards: vec![] },
            Block { id: 3, preds: vec![1], succs: vec![], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
        ],
        entry: 0,
        exit: 3,
    };
    cfg.validate().expect("loop CFG is valid");
    cfg
}

/// The loop-carried flow dependence: header statement `s = s + 1`. After
/// φ-placement the back edge is a φ merge, and `scalar_dd` reports the single
/// situation `(DefId(0), UseId(0)) → One`.
fn carried_flow_cfg() -> CFG {
    shell(
        vec![],
        vec![st(0, ScalarRhs::Add(vec![ScalarRhs::Var(0), ScalarRhs::Const(1)]), vec![], 0)],
        vec![],
    )
}

/// A countable linear IV: `s = 1` in the entry, `s = s + 1` in the latch.
/// `induct` recovers an IV with `a = 1, b = 1, var = 0, loop_ = 1`, so the
/// checker's own `trip_count(1, 1, 8, 0)` yields `count 7, exit_value 8`.
fn iv_cfg() -> CFG {
    shell(
        vec![st(0, ScalarRhs::Const(1), vec![], 0)],
        vec![],
        vec![st(0, ScalarRhs::Add(vec![ScalarRhs::Var(0), ScalarRhs::Const(1)]), vec![], 0)],
    )
}

fn dd_cert(cfg: CFG, def: DefId, use_: UseId, distance: ScalarDistance) -> Certificate {
    Certificate {
        level: "M4-scalardd".into(),
        loop_hash: disk_loop().hash(),
        pair: (0, 0),
        claim: Claim::ScalarDependence { def, use_, distance },
        witness: Witness::ScalarDependenceEvidence {
            cfg: Box::new(cfg),
            asserted_def: def,
            asserted_use: use_,
        },
        checker: "m0-check".into(),
    }
}

fn tc_cert(cfg: CFG, iv_var: usize, loop_: usize, bound: i64, count: i64, exit_value: i64) -> Certificate {
    Certificate {
        level: "M4-tripcount".into(),
        loop_hash: disk_loop().hash(),
        pair: (0, 0),
        claim: Claim::TripCount { count, exit_value },
        witness: Witness::TripCountEvidence { cfg: Box::new(cfg), iv_var, loop_, bound, bound_step: 0, companion_step: None },
        checker: "m0-check".into(),
    }
}

/// Run the real checker binary on (loop, cert). Returns (exit_ok, first line).
fn run_checker(tag: &str, loop_json: &str, cert_json: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("m0-p4-{}-{}", tag, std::process::id()));
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

fn check(tag: &str, cert: &Certificate) -> (bool, String) {
    run_checker(tag, &disk_loop().canonical_json(), &serde_json::to_string(cert).unwrap())
}

// ---------------------------------------------------------------------------
// Claim::ScalarDependence / Witness::ScalarDependenceEvidence
// ---------------------------------------------------------------------------

#[test]
fn scalar_dependence_distance_one_is_accepted() {
    let (ok, out) = check("dd-ok", &dd_cert(carried_flow_cfg(), DefId(0), UseId(0), ScalarDistance::One));
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn scalar_dependence_with_the_wrong_distance_is_rejected() {
    // The same link, asserted at distance (0). The checker rebuilds `scalar_dd`
    // from the carried CFG, finds `(0, 0) → One`, and rejects.
    let (ok, out) = check("dd-dist", &dd_cert(carried_flow_cfg(), DefId(0), UseId(0), ScalarDistance::Zero));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("does not report"), "unexpected message: {out}");
}

#[test]
fn scalar_dependence_that_the_cfg_does_not_support_is_rejected() {
    // A def and a use in one straight-line block: no carried dependence exists,
    // so asserting `One` is a fabrication the rebuilt table cannot confirm.
    let cfg = CFG::new_single_block(
        vec![var("x")],
        vec![st(0, ScalarRhs::Const(1), vec![], 0), st(1, ScalarRhs::Var(0), vec![], 0)],
    );
    let (ok, out) = check("dd-nocarry", &dd_cert(cfg, DefId(0), UseId(0), ScalarDistance::One));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn scalar_dependence_where_claim_and_evidence_disagree_is_rejected() {
    // `s = 1` in the entry, `s = s + 1` in the latch: `scalar_dd` reports the
    // latch def (DefId(1)) reaching the latch use (UseId(0)) at distance (1).
    // That agreeing form is accepted. Then the evidence's def is moved to the
    // entry def, which is *also* in range: neither field is trusted, so the two
    // must agree before the rebuilt table is consulted at all.
    let mut cert = dd_cert(iv_cfg(), DefId(1), UseId(0), ScalarDistance::One);
    let (ok, out) = check("dd-agree", &cert);
    assert!(ok, "the agreeing form must be accepted, checker said: {out}");

    if let Witness::ScalarDependenceEvidence { asserted_def, .. } = &mut cert.witness {
        *asserted_def = DefId(0);
    }
    let (ok, out) = check("dd-forged", &cert);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("disagree"), "unexpected message: {out}");
}

#[test]
fn scalar_dependence_with_an_out_of_range_use_is_rejected() {
    let (ok, out) = check("dd-range", &dd_cert(carried_flow_cfg(), DefId(0), UseId(9), ScalarDistance::One));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("out of range"), "unexpected message: {out}");
}

#[test]
fn scalar_dependence_with_a_mismatched_claim_is_rejected() {
    // Evidence for a dependence paired with a Phase 3 reach claim.
    let mut cert = dd_cert(carried_flow_cfg(), DefId(0), UseId(0), ScalarDistance::One);
    cert.claim = Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) };
    let (ok, out) = check("dd-claim", &cert);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn scalar_dependence_with_a_malformed_carried_cfg_is_rejected() {
    let mut cfg = carried_flow_cfg();
    cfg.blocks[1].succs.push(99);
    let (ok, out) = check("dd-badcfg", &dd_cert(cfg, DefId(0), UseId(0), ScalarDistance::One));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("malformed"), "unexpected message: {out}");
}

// ---------------------------------------------------------------------------
// Claim::TripCount / Witness::TripCountEvidence
// ---------------------------------------------------------------------------

#[test]
fn trip_count_is_recomputed_and_accepted() {
    // `s = 1` then `s += 1` while `s < 8`: the checker's own `induct` yields
    // a = 1, b = 1, and its own `trip_count(1, 1, 8, 0)` yields (7, 8).
    let (ok, out) = check("tc-ok", &tc_cert(iv_cfg(), 0, 1, 8, 7, 8));
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn trip_count_off_by_one_is_rejected() {
    // The §4.4 seed's exact error shape: claiming the loop runs one more time
    // than the recomputation supports.
    let (ok, out) = check("tc-ceil", &tc_cert(iv_cfg(), 0, 1, 8, 8, 9));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("disagrees"), "unexpected message: {out}");
}

#[test]
fn trip_count_with_a_forged_count_is_rejected() {
    // `bound` and `bound_step` are inputs to the recomputation, not trusted
    // facts. Carrying the real bound while claiming the trip count of a much
    // larger one cannot forge agreement, because a and b come from the
    // checker's own `induct` run.
    let (ok, out) = check("tc-forged", &tc_cert(iv_cfg(), 0, 1, 8, 1000, 1001));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn trip_count_nominating_a_non_iv_variable_is_rejected() {
    // A second scalar that `induct` does not classify as an IV of loop 1. The
    // certificate cannot nominate an arbitrary variable and borrow the real
    // IV's arithmetic.
    let mut cfg = iv_cfg();
    cfg.vars.push(var("j"));
    let (ok, out) = check("tc-noniv", &tc_cert(cfg, 1, 1, 8, 7, 8));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("as an IV"), "unexpected message: {out}");
}

#[test]
fn trip_count_nominating_the_wrong_loop_is_rejected() {
    // The IV is real, but the certificate points it at the latch rather than at
    // this loop's header.
    let (ok, out) = check("tc-loop", &tc_cert(iv_cfg(), 0, 2, 8, 7, 8));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn trip_count_of_an_uncountable_loop_is_rejected() {
    // `s = s + 0` never reaches the bound: `trip_count` reports `count: None`,
    // and the arm treats "cannot be counted" as a rejection, never as a silent
    // match against a claim of `0`.
    let cfg = shell(
        vec![st(0, ScalarRhs::Const(1), vec![], 0)],
        vec![],
        vec![st(0, ScalarRhs::Add(vec![ScalarRhs::Var(0), ScalarRhs::Const(0)]), vec![], 0)],
    );
    let (ok, out) = check("tc-none", &tc_cert(cfg, 0, 1, 8, 0, 1));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn trip_count_with_a_mismatched_claim_is_rejected() {
    let mut cert = tc_cert(iv_cfg(), 0, 1, 8, 7, 8);
    cert.claim = Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) };
    let (ok, out) = check("tc-claim", &cert);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

#[test]
fn trip_count_with_a_malformed_carried_cfg_is_rejected() {
    let mut cfg = iv_cfg();
    cfg.blocks[2].succs.push(99);
    let (ok, out) = check("tc-badcfg", &tc_cert(cfg, 0, 1, 8, 7, 8));
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
    assert!(out.contains("malformed"), "unexpected message: {out}");
}



