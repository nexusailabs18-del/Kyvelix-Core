//! Phase 7 end-to-end: the §7.8 `Alias` claim is exercised by the **real**
//! `m0-check` binary.
//!
//! `Witness::AliasEvidence` carries the pointer program and a basis; the checker
//! re-runs the points-to fixpoint and re-derives the alias relation. The accepted
//! certificates prove the rebuild happens; the forged ones (a `must` the graph
//! does not force, a `no-alias` between pointers that share a target, a
//! `disjoint-arrays` claim for overlapping walks) are rejected by the checker's
//! own recomputation.

use std::path::PathBuf;
use std::process::Command;

use m0::certificate::{Certificate, Claim, Witness};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue};
use m0::points::certificate::{
    alias_certificate, points_to, AliasBasis, AliasClaim,
};
use m0::points::fixpoint::{PtrBlock, PtrDecl, PtrProgram};
use m0::points::ptrarith::{ArrayTable, PtrStep};
use m0::points::relation::VarId;
use m0::points::transfer::Effect;

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

/// `A[i] = A[i+1]` over `i = 0..8` — any valid loop; Phase 7 facts ride on the
/// certificate's `loop_hash`, which must match this file.
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
    let dir = std::env::temp_dir().join(format!("m0-p7-{}-{}", tag, std::process::id()));
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

/// A certificate carrying `claim`/`basis` but a program whose facts may not
/// support it — used for the reject cases.
fn cert_of(loop_: &Loop, program: &PtrProgram, claim: AliasClaim, basis: AliasBasis) -> Certificate {
    Certificate {
        level: "P7".into(),
        loop_hash: loop_.hash(),
        pair: m0::certificate::emit::evidence_pair(loop_),
        claim: Claim::Alias(claim),
        witness: Witness::AliasEvidence {
            program: Box::new(program.clone()),
            basis,
        },
        checker: "m0-check".into(),
    }
}

/// `tx`/`ty` targets, `p`/`q` pointers; block 0 sets `p=&tx`, `q=&ty`.
fn distinct_program() -> PtrProgram {
    PtrProgram::new(
        vec![
            PtrDecl::target("tx"),
            PtrDecl::target("ty"),
            PtrDecl::pointer("p"),
            PtrDecl::pointer("q"),
        ],
        0,
        vec![PtrBlock::new(
            0,
            vec![],
            vec![
                Effect::AddressOf { lhs: VarId(2), target: VarId(0) },
                Effect::AddressOf { lhs: VarId(3), target: VarId(1) },
            ],
        )],
    )
}

/// As [`distinct_program`] but both pointers land on `tx`.
fn alias_program() -> PtrProgram {
    PtrProgram::new(
        vec![
            PtrDecl::target("tx"),
            PtrDecl::target("ty"),
            PtrDecl::pointer("p"),
            PtrDecl::pointer("q"),
        ],
        0,
        vec![PtrBlock::new(
            0,
            vec![],
            vec![
                Effect::AddressOf { lhs: VarId(2), target: VarId(0) },
                Effect::AddressOf { lhs: VarId(3), target: VarId(0) },
            ],
        )],
    )
}


// ---------------------------------------------------------------------------
// MustAlias
// ---------------------------------------------------------------------------

#[test]
fn a_must_alias_certificate_is_accepted() {
    let l = copy_loop();
    let prog = distinct_program();
    let c = alias_certificate(
        &l,
        &prog,
        AliasClaim::MustAlias { pointer: VarId(2), target: VarId(0), block: 0 },
        points_to(0),
        "P7",
    )
    .expect("p definitely points at tx");
    let (ok, out) = run_checker("must-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_must_alias_claim_the_graph_does_not_force_is_rejected() {
    let l = copy_loop();
    let prog = distinct_program();
    // p points at tx, never at ty.
    let c = cert_of(
        &l,
        &prog,
        AliasClaim::MustAlias { pointer: VarId(2), target: VarId(1), block: 0 },
        points_to(0),
    );
    let (ok, out) = run_checker("must-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

// ---------------------------------------------------------------------------
// NoAlias
// ---------------------------------------------------------------------------

#[test]
fn a_no_alias_certificate_is_accepted() {
    let l = copy_loop();
    let prog = distinct_program();
    let c = alias_certificate(
        &l,
        &prog,
        AliasClaim::NoAlias { p: VarId(2), q: VarId(3), block: 0 },
        points_to(0),
        "P7",
    )
    .expect("p and q land on different targets");
    let (ok, out) = run_checker("no-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_no_alias_claim_for_two_pointers_that_share_a_target_is_rejected() {
    let l = copy_loop();
    let prog = alias_program();
    let c = cert_of(
        &l,
        &prog,
        AliasClaim::NoAlias { p: VarId(2), q: VarId(3), block: 0 },
        points_to(0),
    );
    let (ok, out) = run_checker("no-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}

// ---------------------------------------------------------------------------
// DisjointArrays
// ---------------------------------------------------------------------------

fn disjoint_basis() -> (AliasBasis, PtrStep, PtrStep) {
    let arrays = ArrayTable::of(&[(VarId(10), 8), (VarId(11), 8)]);
    (
        AliasBasis::Reconstructed { arrays },
        PtrStep::new(VarId(2), VarId(10), 1, 0, 8, 0),
        PtrStep::new(VarId(3), VarId(11), 1, 0, 8, 0),
    )
}

#[test]
fn a_disjoint_arrays_certificate_is_accepted() {
    let l = copy_loop();
    let prog = distinct_program();
    let (basis, s1, s2) = disjoint_basis();
    let c = alias_certificate(
        &l,
        &prog,
        AliasClaim::DisjointArrays { first: s1, second: s2 },
        basis,
        "P7",
    )
    .expect("the two walks land in different arrays");
    let (ok, out) = run_checker("disj-ok", &l, &c);
    assert!(ok && out.starts_with("ACCEPT"), "checker said: {out}");
}

#[test]
fn a_disjoint_arrays_claim_for_overlapping_walks_is_rejected() {
    let l = copy_loop();
    let prog = distinct_program();
    let arrays = ArrayTable::of(&[(VarId(10), 8)]);
    let c = cert_of(
        &l,
        &prog,
        AliasClaim::DisjointArrays {
            first: PtrStep::new(VarId(2), VarId(10), 2, 0, 8, 0),
            second: PtrStep::new(VarId(3), VarId(10), 1, 0, 8, 0),
        },
        AliasBasis::Reconstructed { arrays },
    );
    let (ok, out) = run_checker("disj-bad", &l, &c);
    assert!(!ok && out.starts_with("REJECT"), "expected rejection, got: {out}");
}
