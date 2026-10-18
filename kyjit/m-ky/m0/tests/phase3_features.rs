//! Custom Phase 3 verification: the scalar IR, CFG, dominators, φ-placement,
//! Algorithm Chain (with the reverse pass), FollowChain, and FRUD chains —
//! checked with hand-built CFGs and hand-computed expected links, plus the
//! `FudChainEvidence` certificate through the real checker binary.

use m0::fud::{
    build_cfg_from_loop, build_cfg_from_stmts, build_chain_with_phi, cfg_matches_loop,
    compute_dominance_frontiers, compute_dominators, construct_frud_chains, find_reaching_defs,
    phi_blocks_for, ChainLink,
};
use m0::ir::cfg::{Block, CFG};
use m0::ir::scalar::{DefId, ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar, UseId};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue, Stmt};

fn var(n: &str) -> ScalarVar {
    ScalarVar { name: n.into(), ty: ScalarTy::I64 }
}

/// Build a block from per-statement `(uses, defs)` variable lists. Statement
/// ids and use/def positions are set canonically.
fn blk(id: usize, preds: Vec<usize>, succs: Vec<usize>, spec: Vec<(Vec<usize>, Vec<usize>)>) -> Block {
    let stmts = spec
        .into_iter()
        .enumerate()
        .map(|(s, (uses, defs))| Stmt {
            id: s,
            uses: uses.into_iter().enumerate().map(|(o, v)| ScalarUse { var: v, stmt: s, order: o }).collect(),
            defs: defs.into_iter().enumerate().map(|(o, v)| ScalarDef { var: v, stmt: s, order: o, rhs: ScalarRhs::Unknown }).collect(),
        })
        .collect();
    Block { id, preds, succs, stmts, phi: vec![], upsilon: vec![], guards: vec![] }
}

/// A block whose only statement is a pure use of `v`.
fn use_blk(id: usize, preds: Vec<usize>, succs: Vec<usize>, v: usize) -> Block {
    blk(id, preds, succs, vec![(vec![v], vec![])])
}

/// A block whose only statement is a pure def of `v`.
fn def_blk(id: usize, preds: Vec<usize>, succs: Vec<usize>, v: usize) -> Block {
    blk(id, preds, succs, vec![(vec![], vec![v])])
}

fn empty_blk(id: usize, preds: Vec<usize>, succs: Vec<usize>) -> Block {
    Block { id, preds, succs, stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] }
}

/// A one-statement pure def of `v`, for `build_cfg_from_stmts`.
fn def_stmt(v: usize) -> Stmt {
    Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var: v, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] }
}

/// A one-statement pure use of `v`, for `build_cfg_from_stmts`.
fn use_stmt(v: usize) -> Stmt {
    Stmt { id: 0, uses: vec![ScalarUse { var: v, stmt: 0, order: 0 }], defs: vec![] }
}

// ---- IR + CFG -------------------------------------------------------------

#[test]
fn build_cfg_from_loop_is_the_five_block_loop_shape() {
    let l = Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![
            Access { kind: AccessKind::Write, base: "A".into(), index: vec![1], elem_size: 8, offset: OffsetValue::Const(0), expr: None, indirect: None, guard: None, range: None, index_kind: IndexKind::Affine },
            Access { kind: AccessKind::Read, base: "A".into(), index: vec![1], elem_size: 8, offset: OffsetValue::Const(2), expr: None, indirect: None, guard: None, range: None, index_kind: IndexKind::Affine },
        ],
        recursion: None, chase: None, assertions: vec![],
     scalars: vec![], stmts: None,};
    // Phase 9a: the CFG carries the loop's back edge (latch → header), so the
    // scalar analyses have a cycle to chew on.
    let cfg = build_cfg_from_loop(&l).expect("loop with an IV has a CFG");
    assert_eq!(cfg.blocks.len(), 5);
    assert_eq!((cfg.entry, cfg.exit), (0, 4));
    assert_eq!(cfg.blocks[2].stmts.len(), 2);
    assert!(cfg.blocks[3].succs.contains(&1), "latch must reach the header");
    cfg.validate().expect("canonical CFG is valid");
    cfg_matches_loop(&cfg, &l).expect("shape matches the loop");
}

#[test]
fn cfg_validate_rejects_broken_symmetry() {
    let mut cfg = build_cfg_from_stmts(vec![var("x")], vec![]);
    cfg.blocks[1].preds = vec![]; // break entry→body
    assert!(cfg.validate().is_err());
}

#[test]
fn cfg_reversed_swaps_entry_exit() {
    let cfg = build_cfg_from_stmts(vec![var("x")], vec![def_stmt(0)]);
    let rev = cfg.reversed();
    assert_eq!((rev.entry, rev.exit), (2, 0));
    rev.validate().expect("reversed CFG is valid");
}

// ---- Dominators + frontiers ----------------------------------------------

#[test]
fn dominators_on_linear_chain() {
    let cfg = CFG {
        vars: vec![var("x")],
        blocks: vec![
            empty_blk(0, vec![], vec![1]),
            empty_blk(1, vec![0], vec![2]),
            empty_blk(2, vec![1], vec![3]),
            empty_blk(3, vec![2], vec![]),
        ],
        entry: 0,
        exit: 3,
    };
    let dom = compute_dominators(&cfg);
    assert_eq!(dom.parent(0), None);
    assert_eq!(dom.parent(1), Some(0));
    assert_eq!(dom.parent(2), Some(1));
    assert_eq!(dom.parent(3), Some(2));
    assert!(dom.dominates(0, 3));
    assert!(!dom.dominates(3, 0));
}

#[test]
fn df_of_diamond_merge() {
    let cfg = CFG {
        vars: vec![],
        blocks: vec![
            empty_blk(0, vec![], vec![1]),
            empty_blk(1, vec![0], vec![2, 3]),
            empty_blk(2, vec![1], vec![4]),
            empty_blk(3, vec![1], vec![4]),
            empty_blk(4, vec![2, 3], vec![]),
        ],
        entry: 0,
        exit: 4,
    };
    let dom = compute_dominators(&cfg);
    let fr = compute_dominance_frontiers(&cfg, &dom);
    assert!(fr.df[2].contains(&4));
    assert!(fr.df[3].contains(&4));
}

// ---- φ-placement + Chain --------------------------------------------------

fn diamond_defs_both_and_use() -> CFG {
    CFG {
        vars: vec![var("x")],
        blocks: vec![
            empty_blk(0, vec![], vec![1]),
            empty_blk(1, vec![0], vec![2, 3]),
            def_blk(2, vec![1], vec![4], 0),
            def_blk(3, vec![1], vec![4], 0),
            use_blk(4, vec![2, 3], vec![], 0),
        ],
        entry: 0,
        exit: 4,
    }
}

#[test]
fn phi_placed_at_merge_and_reached_by_use() {
    let cfg = diamond_defs_both_and_use();
    let (annotated, chain) = build_chain_with_phi(&cfg);
    assert_eq!(phi_blocks_for(&annotated, 0), [4].into_iter().collect());
    match chain.use_to_def.get(&UseId(0)) {
        Some(ChainLink::Phi(_)) => {}
        other => panic!("expected a φ link, got {:?}", other),
    }
}

#[test]
fn reaching_set_after_merge_has_both_defs() {
    let cfg = diamond_defs_both_and_use();
    let (annotated, chain) = build_chain_with_phi(&cfg);
    let rd = find_reaching_defs(&annotated, &chain);
    assert_eq!(rd.of(UseId(0)), [DefId(0), DefId(1)].into_iter().collect());
}

#[test]
fn chain_reverse_pass_isolates_branches() {
    // def x in block 0; def x in block 2; use x in block 3.
    // Block 3's use MUST reach the block-0 def, not block 2's (reverse pass).
    let cfg = CFG {
        vars: vec![var("x")],
        blocks: vec![
            def_blk(0, vec![], vec![1], 0),
            empty_blk(1, vec![0], vec![2, 3]),
            def_blk(2, vec![1], vec![4], 0),
            use_blk(3, vec![1], vec![4], 0),
            empty_blk(4, vec![2, 3], vec![]),
        ],
        entry: 0,
        exit: 4,
    };
    let (annotated, chain) = build_chain_with_phi(&cfg);
    let rd = find_reaching_defs(&annotated, &chain);
    assert_eq!(rd.of(UseId(0)), [DefId(0)].into_iter().collect());
}

#[test]
fn single_block_uses_reach_the_prior_def() {
    // def x; use x; use x  → both uses reach DefId(0).
    let cfg = build_cfg_from_stmts(
        vec![var("x")],
        vec![def_stmt(0), use_stmt(0), use_stmt(0)],
    );
    let (annotated, chain) = build_chain_with_phi(&cfg);
    let rd = find_reaching_defs(&annotated, &chain);
    assert_eq!(chain.direct_def(UseId(0)), Some(DefId(0)));
    assert_eq!(chain.direct_def(UseId(1)), Some(DefId(0)));
    assert_eq!(rd.of(UseId(0)), [DefId(0)].into_iter().collect());
    assert_eq!(rd.of(UseId(1)), [DefId(0)].into_iter().collect());
}

#[test]
fn use_before_def_is_uninitialized() {
    let cfg = build_cfg_from_stmts(vec![var("x")], vec![use_stmt(0), def_stmt(0)]);
    let (annotated, chain) = build_chain_with_phi(&cfg);
    let rd = find_reaching_defs(&annotated, &chain);
    assert!(!chain.has_link(UseId(0)));
    assert!(rd.of(UseId(0)).is_empty());
}

#[test]
fn second_def_shadows_first() {
    // def x; def x; use x → use reaches the second def.
    let cfg = build_cfg_from_stmts(
        vec![var("x")],
        vec![def_stmt(0), def_stmt(0), use_stmt(0)],
    );
    let (annotated, chain) = build_chain_with_phi(&cfg);
    let rd = find_reaching_defs(&annotated, &chain);
    assert_eq!(chain.direct_def(UseId(0)), Some(DefId(1)));
    assert_eq!(rd.of(UseId(0)), [DefId(1)].into_iter().collect());
}

// ---- FRUD -----------------------------------------------------------------

#[test]
fn frud_is_the_transpose_of_reaching_defs() {
    // Over several hand-built shapes, `FrudChain.reaches(d, u)` must equal
    // `d ∈ ReachingDefs.of(u)`.
    let shapes = vec![
        diamond_defs_both_and_use(),
        CFG {
            vars: vec![var("x")],
            blocks: vec![
                def_blk(0, vec![], vec![1], 0),
                use_blk(1, vec![0], vec![], 0),
            ],
            entry: 0,
            exit: 1,
        },
        build_cfg_from_stmts(
            vec![var("x")],
            vec![def_stmt(0), use_stmt(0), def_stmt(0), use_stmt(0)],
        ),
    ];
    for cfg in shapes {
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let rd = find_reaching_defs(&annotated, &chain);
        let frud = construct_frud_chains(&annotated, &chain);
        let ix = annotated.index();
        for di in 0..ix.defs.len() {
            for ui in 0..ix.uses.len() {
                let via_frud = frud.reaches(DefId(di), UseId(ui));
                let via_reach = rd.of(UseId(ui)).contains(&DefId(di));
                assert_eq!(via_frud, via_reach, "def#{} → use#{}", di, ui);
            }
        }
    }
}

#[test]
fn frud_def_reaches_only_later_uses() {
    // def x; use x; def x; use x
    let cfg = build_cfg_from_stmts(
        vec![var("x")],
        vec![def_stmt(0), use_stmt(0), def_stmt(0), use_stmt(0)],
    );
    let (annotated, chain) = build_chain_with_phi(&cfg);
    let frud = construct_frud_chains(&annotated, &chain);
    assert_eq!(frud.uses_of(DefId(0)), [UseId(0)].into_iter().collect());
    assert_eq!(frud.uses_of(DefId(1)), [UseId(1)].into_iter().collect());
}

// ---- End-to-end: FudChainEvidence through the real m0-check binary --------

use m0::certificate::{Certificate, Claim, Witness};
use std::path::PathBuf;
use std::process::Command;

fn run_checker(tag: &str, loop_json: &str, cert_json: &str) -> String {
    let dir = std::env::temp_dir().join(format!("m0-p3f-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lp = dir.join("loop.json");
    let cp = dir.join("cert.json");
    std::fs::write(&lp, loop_json).unwrap();
    std::fs::write(&cp, cert_json).unwrap();
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-check"));
    let out = Command::new(bin).arg(&lp).arg(&cp).output().expect("run m0-check");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    std::fs::remove_dir_all(&dir).ok();
    if text.is_empty() { err } else { text }
}

/// A disk loop to bind the certificate hash to (its CFG is carried separately).
fn disk_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![Access {
            kind: AccessKind::Write, base: "A".into(), index: vec![1], elem_size: 8,
            offset: OffsetValue::Const(0), expr: None, indirect: None, guard: None,
            range: None, index_kind: IndexKind::Affine,
        }],
        recursion: None, chase: None, assertions: vec![],
     scalars: vec![], stmts: None,}
}

fn evidence(cfg: CFG, claim: Claim, u: UseId, d: Option<DefId>) -> Certificate {
    Certificate {
        level: "M3-fud".into(),
        loop_hash: disk_loop().hash(),
        pair: (0, 0),
        claim,
        witness: Witness::FudChainEvidence { cfg: Box::new(cfg), asserted_use: u, asserted_def: d },
        checker: "m0-check".into(),
    }
}

#[test]
fn fud_reach_certificate_accepted() {
    let cfg = CFG {
        vars: vec![var("x")],
        blocks: vec![
            def_blk(0, vec![], vec![1], 0),
            use_blk(1, vec![0], vec![], 0),
        ],
        entry: 0,
        exit: 1,
    };
    let cert = evidence(
        cfg,
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(0),
        Some(DefId(0)),
    );
    let out = run_checker("reach", &disk_loop().canonical_json(), &serde_json::to_string(&cert).unwrap());
    assert!(out.starts_with("ACCEPT"), "checker said: {}", out);
}

#[test]
fn fud_no_reach_certificate_accepted() {
    let cfg = CFG {
        vars: vec![var("x")],
        blocks: vec![use_blk(0, vec![], vec![], 0)],
        entry: 0,
        exit: 0,
    };
    let cert = evidence(cfg, Claim::ScalarNoReach { use_id: UseId(0) }, UseId(0), None);
    let out = run_checker("noreach", &disk_loop().canonical_json(), &serde_json::to_string(&cert).unwrap());
    assert!(out.starts_with("ACCEPT"), "checker said: {}", out);
}

#[test]
fn fud_cross_variable_reach_rejected() {
    // use x in block 1; def y in block 0. Claiming the use of x reaches the
    // def of y must be rejected (variable mismatch).
    let cfg = CFG {
        vars: vec![var("x"), var("y")],
        blocks: vec![
            def_blk(0, vec![], vec![1], 1),
            use_blk(1, vec![0], vec![], 0),
        ],
        entry: 0,
        exit: 1,
    };
    let cert = evidence(
        cfg,
        Claim::ScalarReach { use_id: UseId(0), def_id: DefId(0) },
        UseId(0),
        Some(DefId(0)),
    );
    let out = run_checker("xvar", &disk_loop().canonical_json(), &serde_json::to_string(&cert).unwrap());
    assert!(out.contains("REJECT"), "checker said: {}", out);
}

#[test]
fn fud_false_no_reach_rejected() {
    // The use IS reachable, so claiming no-reach must be rejected.
    let cfg = CFG {
        vars: vec![var("x")],
        blocks: vec![
            def_blk(0, vec![], vec![1], 0),
            use_blk(1, vec![0], vec![], 0),
        ],
        entry: 0,
        exit: 1,
    };
    let cert = evidence(cfg, Claim::ScalarNoReach { use_id: UseId(0) }, UseId(0), None);
    let out = run_checker("falsenoreach", &disk_loop().canonical_json(), &serde_json::to_string(&cert).unwrap());
    assert!(out.contains("REJECT"), "checker said: {}", out);
}
