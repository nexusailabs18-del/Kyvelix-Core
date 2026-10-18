//! Phase 3 end-to-end: a `Witness::FudChainEvidence` certificate is verified
//! by the **real** `m0-check` binary, which rebuilds the FUD chain from the
//! carried CFG rather than trusting the analyzer.
//!
//! The accepted cases show a scalar use certified as reaching (or provably not
//! reaching) a definition. The rejected cases are the point: the checker
//! refuses a forged reaching link, a claim that contradicts the evidence, and
//! a malformed carried CFG — it re-derives everything, exactly as `WithLoop`
//! and `CrossLoopSolution` do.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::{Certificate, Claim, Witness};
use m0::ir::cfg::{Block, CFG};
use m0::ir::scalar::{DefId, ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar, UseId};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue, Stmt};

/// Minimal loop used as the "file on disk". Its hash binds the certificate;
/// the carried CFG is the ground truth the checker rebuilds from.
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

/// A block whose single statement is a pure use of `v`.
fn use_block(id: usize, preds: Vec<usize>, succs: Vec<usize>, v: usize) -> Block {
    Block {
        id,
        preds,
        succs,
        stmts: vec![Stmt { id: 0, uses: vec![ScalarUse { var: v, stmt: 0, order: 0 }], defs: vec![] }],
        phi: vec![],
        upsilon: vec![],
        guards: vec![],
    }
}

/// A block whose single statement is a pure def of `v`.
fn def_block(id: usize, preds: Vec<usize>, succs: Vec<usize>, v: usize) -> Block {
    Block {
        id,
        preds,
        succs,
        stmts: vec![Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var: v, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] }],
        phi: vec![],
        upsilon: vec![],
        guards: vec![],
    }
}

/// `0(def x) → 1(use x)`: the use reaches DefId(0).
fn reach_cfg() -> CFG {
    CFG {
        vars: vec![var("x")],
        blocks: vec![
            def_block(0, vec![], vec![1], 0),
            use_block(1, vec![0], vec![], 0),
        ],
        entry: 0,
        exit: 1,
    }
}

/// `0(use x)`: the use has no reaching definition (uninitialized).
fn uninit_cfg() -> CFG {
    CFG {
        vars: vec![var("x")],
        blocks: vec![use_block(0, vec![], vec![], 0)],
        entry: 0,
        exit: 0,
    }
}

fn evidence_cert(
    cfg: CFG,
    claim: Claim,
    asserted_use: UseId,
    asserted_def: Option<DefId>,
) -> Certificate {
    Certificate {
        level: "M3-fud".into(),
        loop_hash: disk_loop().hash(),
        pair: (0, 0),
        claim,
        witness: Witness::FudChainEvidence {
            cfg: Box::new(cfg),
            asserted_use,
            asserted_def,
        },
        checker: "m0-check".into(),
    }
}

/// Run the real checker binary on (loop, cert). Returns (exit_ok, last line).
fn run_checker(tag: &str, loop_json: &str, cert_json: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("m0-p3-{}-{}", tag, std::process::id()));
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

fn loop_json() -> String {
    disk_loop().canonical_json()
}

#[test]
fn scalar_reach_is_accepted() {
    let cert = evidence_cert(
        reach_cfg(),
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(0),
        Some(DefId(0)),
    );
    let (ok, out) = run_checker("reach", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn scalar_no_reach_is_accepted() {
    let cert = evidence_cert(
        uninit_cfg(),
        Claim::ScalarNoReach { use_id: UseId(0) },
        UseId(0),
        None,
    );
    let (ok, out) = run_checker("noreach", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn cross_variable_reach_is_rejected() {
    // use x in block 1; def y in block 0. Claim the use of x reaches the def
    // of y — the checker rejects on the variable-agreement guard.
    let cfg = CFG {
        vars: vec![var("x"), var("y")],
        blocks: vec![
            def_block(0, vec![], vec![1], 1),
            use_block(1, vec![0], vec![], 0),
        ],
        entry: 0,
        exit: 1,
    };
    let cert = evidence_cert(
        cfg,
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(0),
        Some(DefId(0)),
    );
    let (ok, out) = run_checker("xvar", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(!ok && out.contains("REJECT"), "checker said: {out}");
}

#[test]
fn scalar_no_reach_where_reachable_is_rejected() {
    let cert = evidence_cert(
        reach_cfg(),
        Claim::ScalarNoReach { use_id: UseId(0) },
        UseId(0),
        None,
    );
    let (ok, out) =
        run_checker("falsenoreach", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(!ok && out.contains("REJECT"), "checker said: {out}");
}

#[test]
fn scalar_reach_where_uninitialized_is_rejected() {
    let cert = evidence_cert(
        uninit_cfg(),
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(0),
        Some(DefId(0)),
    );
    let (ok, out) =
        run_checker("falsereach", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(!ok && out.contains("REJECT"), "checker said: {out}");
}

#[test]
fn mismatched_claim_and_evidence_are_rejected() {
    // Claim names use 0; the evidence asserts use 1.
    let mut cfg = reach_cfg();
    cfg.blocks[1].stmts.push(Stmt {
        id: 1,
        uses: vec![ScalarUse { var: 0, stmt: 1, order: 0 }],
        defs: vec![],
    });
    let cert = evidence_cert(
        cfg,
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(1),
        Some(DefId(0)),
    );
    let (ok, out) =
        run_checker("mismatch", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(!ok && out.contains("REJECT"), "checker said: {out}");
}

#[test]
fn malformed_carried_cfg_is_rejected() {
    let mut cfg = reach_cfg();
    cfg.blocks[1].preds = vec![]; // break succ/pred symmetry
    let cert = evidence_cert(
        cfg,
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(0),
        Some(DefId(0)),
    );
    let (ok, out) =
        run_checker("malformed", &loop_json(), &serde_json::to_string(&cert).unwrap());
    assert!(!ok && out.contains("REJECT"), "checker said: {out}");
}
