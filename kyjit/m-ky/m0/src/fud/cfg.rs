//! CFG construction from the IR (Phase 3.1, extended by Phase 9a).
//!
//! Phase 3 builds a [`CFG`] out of a [`crate::ir::Loop`]. Before Phase 9a the
//! IR modelled array accesses only, so every loop body was a single basic block
//! and the CFG had the degenerate shape
//!
//! ```text
//! entry(0) → body(1) → exit(2)
//! ```
//!
//! That shape has **no back edge**, so the Phase 4 analyses had nothing to chew
//! on: no loop header means [`crate::fud::induct`] finds no IVs and
//! [`crate::fud::scalar_dd`] reports no loop-carried situations. Phase 9a fixes
//! that by lowering the loop's scalar statements — its induction variables, its
//! guard, any scalar assignments — into CFG statements with real `uses`/`defs`,
//! and by emitting the loop's back edge.
//!
//! [`build_cfg_from_loop`] now yields the standard five-block loop CFG
//!
//! ```text
//! entry(0):   i := lower                 — IV initialisation
//! header(1):  φ_i := φ(entry.i, latch.i) — IV φ-terms + the loop guard
//! body(2):    user statements            — the loop body
//! latch(3):   i := i + step              — IV increment
//! exit(4):    (nothing)                  — where the guard falls through
//! ```
//!
//! with edges `0→1`, `1→2` (guard true), `1→4` (guard false), `2→3`, and the
//! back edge `3→1`. The φ-terms are placed by
//! [`crate::fud::build_chain_with_phi`] when the analyses run — the builder
//! itself emits none, exactly as Phase 3's other constructors do.
//!
//! Callers that already hold a statement list (Phase 3's tests, Phase 4's
//! fixtures) still use [`build_cfg_from_stmts`], which keeps the old
//! single-block shape.

use crate::ir::scalar::ScalarVar;
use crate::ir::{Block, Cmp, Loop, LoopStmt, ScalarExpr, ScalarTy, Stmt, StmtKind, CFG};

/// Build the canonical single-block CFG from an explicit statement list.
/// Statement/use/def ids and positions are rewritten to canonical values
/// (see [`CFG::new_single_block`]).
pub fn build_cfg_from_stmts(vars: Vec<ScalarVar>, stmts: Vec<Stmt>) -> CFG {
    CFG::new_single_block(vars, stmts)
}

/// Build the standard five-block loop CFG from a `Loop` (Phase 9a).
///
/// Block layout (see the module docs): `entry(0) → header(1) → {body(2) →
/// latch(3) → header(1)}`, `header(1) → exit(4)`. Returns `None` when the loop
/// has no induction variables — without an IV there is no guard and no latch,
/// so there is no loop at all.
///
/// The IVs are registered as scalars `0..depth`; any scalars the loop declares
/// in [`crate::ir::Loop::scalars`] are appended after them **unless** their name
/// is already an IV name (so a loop that declares its own IVs does not double
/// them).
///
/// When [`crate::ir::Loop::stmts`] is `Some`, those statements are the body
/// verbatim. When it is `None`, the body is synthesised: the existing
/// `body: Vec<Access>` become array statements (no scalar uses/defs — the
/// array-side dependence is M2B's path).
///
/// # Statement numbering
///
/// Each block's statements are numbered from `0` (canonical, as
/// [`CFG::validate`] requires). A frontend `Stmt::id` is therefore *not*
/// preserved across the lowering: it is re-derived from position, exactly as
/// [`CFG::new_single_block`] does for a flat statement list.
pub fn build_cfg_from_loop(loop_: &Loop) -> Option<CFG> {
    if loop_.ivs.is_empty() {
        return None;
    }

    // ---- Scalar table: IVs first, then declared scalars by name. ----
    // The IVs occupy `0..depth` (the convention `Loop::iv_scalar_index`
    // documents); the loop's own scalar state only adds names that are not
    // IVs, so a loop that lists its IVs in `scalars` (the natural frontend
    // output) and one that leaves `scalars` empty produce the same table.
    let mut vars: Vec<ScalarVar> = loop_
        .ivs
        .iter()
        .map(|iv| ScalarVar { name: iv.name.clone(), ty: ScalarTy::I64 })
        .collect();
    for s in &loop_.scalars {
        if !vars.iter().any(|v| v.name == s.name) {
            vars.push(s.clone());
        }
    }

    // ---- Block 0: entry — initialise the IVs. ----
    let entry_stmts: Vec<LoopStmt> = loop_
        .ivs
        .iter()
        .enumerate()
        .map(|(k, iv)| LoopStmt {
            id: k,
            kind: StmtKind::Assign { target: k, expr: ScalarExpr::Const(iv.lower) },
        })
        .collect();

    // ---- Block 1: header — one guard per IV with its bound constraint. ----
    let header_stmts: Vec<LoopStmt> = (0..loop_.ivs.len())
        .map(|k| {
            let iv = &loop_.ivs[k];
            // Use the IV's bound_op to create the correct guard constraint
            let constraint = Cmp {
                iv: k,
                op: iv.bound_op,
                bound: iv.upper,
            };
            LoopStmt { id: k, kind: StmtKind::Guard { constraints: vec![constraint] } }
        })
        .collect();

    // ---- Block 2: body — user statements, or accesses synthesised. ----
    let body_stmts: Vec<LoopStmt> = match &loop_.stmts {
        Some(s) => s.clone(),
        None => synth_stmts_from_body(loop_),
    };

    // ---- Block 3: latch — the IV increment. ----
    let latch_stmts: Vec<LoopStmt> = loop_
        .ivs
        .iter()
        .enumerate()
        .map(|(k, iv)| LoopStmt {
            id: k,
            kind: StmtKind::Assign {
                target: k,
                expr: ScalarExpr::Add(
                    Box::new(ScalarExpr::Iv(k)),
                    Box::new(ScalarExpr::Const(iv.step)),
                ),
            },
        })
        .collect();

    let mut builder = CFGBuilder::new(vars);
    let entry = builder.new_block();
    for s in &entry_stmts {
        builder.add_stmt(entry, s);
    }
    let header = builder.new_block();
    for s in &header_stmts {
        builder.add_stmt(header, s);
    }
    let body = builder.new_block();
    for s in &body_stmts {
        builder.add_stmt(body, s);
    }
    let latch = builder.new_block();
    for s in &latch_stmts {
        builder.add_stmt(latch, s);
    }
    let exit = builder.new_block();

    builder.add_edge(entry, header);
    builder.add_edge(header, body); // guard true
    builder.add_edge(header, exit); // guard false
    builder.add_edge(body, latch);
    builder.add_edge(latch, header); // BACK EDGE — the critical one

    let cfg = builder.finish(entry, exit);
    match cfg.validate() {
        Ok(()) => Some(cfg),
        Err(_) => None,
    }
}

/// When `loop_.stmts` is `None`, synthesise a body from the existing
/// `body: Vec<Access>`: each access becomes an array statement, in body order.
/// Those statements contribute no scalar uses or defs.
fn synth_stmts_from_body(loop_: &Loop) -> Vec<LoopStmt> {
    loop_
        .body
        .iter()
        .enumerate()
        .map(|(id, a)| LoopStmt {
            id,
            kind: StmtKind::Array { access: a.clone() },
        })
        .collect()
}

/// Builder for a multi-block CFG (Phase 9a).
///
/// The Phase 3 CFG could be assembled by hand because it was always exactly
/// three blocks with fixed edges. The Phase 9a shape has a back edge, so the
/// pred/succ symmetry has to be maintained in two places at once — easy to get
/// wrong by hand, and the mistake is exactly the one that silently destroys
/// every downstream analysis. This helper makes `add_edge` the only way to
/// introduce an edge, so symmetry is structural.
struct CFGBuilder {
    blocks: Vec<Block>,
    vars: Vec<ScalarVar>,
}

impl CFGBuilder {
    fn new(vars: Vec<ScalarVar>) -> CFGBuilder {
        CFGBuilder { blocks: Vec::new(), vars }
    }

    /// Append a fresh, empty block and return its index.
    fn new_block(&mut self) -> usize {
        let id = self.blocks.len();
        self.blocks.push(Block {
            id,
            preds: Vec::new(),
            succs: Vec::new(),
            stmts: Vec::new(),
            phi: Vec::new(),
            upsilon: Vec::new(),
            guards: Vec::new(),
        });
        id
    }

    /// Lower `s` and append it to `block`. The statement's canonical `id`
    /// (and every use/def `stmt`/`order`) is assigned from its position in the
    /// block by [`LoopStmt::lower`].
    fn add_stmt(&mut self, block: usize, s: &LoopStmt) {
        let stmt_index = self.blocks[block].stmts.len();
        // Capture guard constraints before lowering loses them
        if let StmtKind::Guard { constraints } = &s.kind {
            self.blocks[block].guards = constraints.clone();
        }
        self.blocks[block].stmts.push(s.lower(stmt_index));
    }

    /// Add the edge `from → to`, maintaining pred/succ symmetry.
    fn add_edge(&mut self, from: usize, to: usize) {
        self.blocks[from].succs.push(to);
        self.blocks[to].preds.push(from);
    }

    fn finish(self, entry: usize, exit: usize) -> CFG {
        CFG {
            vars: self.vars,
            blocks: self.blocks,
            entry,
            exit,
        }
    }
}

/// Check that a carried CFG matches the loop on disk, for the shapes the IR
/// can express.
///
/// Checked: the CFG is structurally valid; it has the Phase 9a five-block loop
/// shape (`entry → header → {body → latch → header, exit}`, back edge
/// `latch → header`, header fed by both the entry and the latch); the loop's
/// IVs agree with the CFG's first variables; and the body block's statement
/// count equals the loop's statement count ([`crate::ir::Loop::stmts`] when
/// present, else its access count).
///
/// # Known limitation
///
/// A carried CFG's uses/defs are still not cross-checked against the loop file
/// statement by statement: `Loop::stmts` is the only representation of those
/// facts on disk, and a loop that omits it (the common case) has no scalar
/// statements to compare against. The checker still **rebuilds** the chains
/// from the carried CFG, so a forged chain is impossible.
pub fn cfg_matches_loop(cfg: &CFG, loop_: &Loop) -> Result<(), String> {
    cfg.validate()?;
    if loop_.ivs.is_empty() {
        return Err("loop has no IVs, so it has no loop CFG".into());
    }
    let n = cfg.blocks.len();
    if n != 5 || cfg.entry != 0 || cfg.exit != 4 {
        return Err(format!(
            "CFG is not the five-block loop shape ({} blocks, entry {}, exit {})",
            n, cfg.entry, cfg.exit
        ));
    }
    // The header (block 1) is fed by the entry and by the latch (the back edge).
    if !cfg.blocks[1].preds.contains(&0) || !cfg.blocks[1].preds.contains(&3) {
        return Err("CFG header (block 1) is not fed by the entry and the latch".into());
    }
    if !cfg.blocks[3].succs.contains(&1) {
        return Err("CFG has no back edge latch(3) → header(1)".into());
    }
    // The IVs come first in the variable table, in order.
    for (k, iv) in loop_.ivs.iter().enumerate() {
        match cfg.vars.get(k) {
            Some(v) if v.name == iv.name => {}
            _ => {
                return Err(format!("CFG scalar {} is not IV {} ({})", k, k, iv.name));
            }
        }
    }
    // The body block holds exactly the loop's statements.
    let want = match &loop_.stmts {
        Some(s) => s.len(),
        None => loop_.body.len(),
    };
    let got = cfg.blocks[2].stmts.len();
    if got != want {
        return Err(format!(
            "CFG body has {} statements but the loop has {}",
            got, want
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::cfg::CFG;
    use crate::ir::scalar::{DefId, ScalarDef, ScalarRhs, ScalarTy, ScalarUse, UseId};
    use crate::ir::{Access, AccessKind, IndexKind, OffsetValue, IV};

    fn loop_with_n_body(n: usize) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 8, 1)],
            body: (0..n)
                .map(|k| Access {
                    kind: if k % 2 == 0 { AccessKind::Write } else { AccessKind::Read },
                    base: "A".into(),
                    index: vec![1],
                    elem_size: 8,
                    offset: OffsetValue::Const(k as i64),
                    expr: None,
                    indirect: None,
                    guard: None,
                    range: None,
                    index_kind: IndexKind::Affine,
                })
                .collect(),
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    #[test]
    fn cfg_from_loop_is_the_five_block_loop_shape() {
        let l = loop_with_n_body(2);
        let cfg = build_cfg_from_loop(&l).expect("loop with an IV has a CFG");
        assert_eq!(cfg.blocks.len(), 5);
        assert_eq!(cfg.entry, 0);
        assert_eq!(cfg.exit, 4);
        assert_eq!(cfg.blocks[0].preds, Vec::<usize>::new());
        assert_eq!(cfg.blocks[0].succs, vec![1]);
        // The header is fed by the entry *and the latch* — the back edge.
        assert_eq!(cfg.blocks[1].preds, vec![0, 3]);
        assert_eq!(cfg.blocks[1].succs, vec![2, 4]);
        assert_eq!(cfg.blocks[2].preds, vec![1]);
        assert_eq!(cfg.blocks[2].succs, vec![3]);
        assert_eq!(cfg.blocks[3].preds, vec![2]);
        assert_eq!(cfg.blocks[3].succs, vec![1]);
        assert_eq!(cfg.blocks[4].preds, vec![1]);
        assert_eq!(cfg.blocks[4].succs, Vec::<usize>::new());
        cfg.validate().expect("loop CFG is well-formed");
        // The exit block holds nothing; the entry and latch hold the IV init
        // and increment.
        assert!(cfg.blocks[4].stmts.is_empty());
        assert_eq!(cfg.blocks[0].stmts.len(), 1);
        assert_eq!(cfg.blocks[3].stmts.len(), 1);
    }

    #[test]
    fn cfg_from_loop_without_ivs_is_none() {
        let mut l = loop_with_n_body(2);
        l.ivs.clear();
        assert!(build_cfg_from_loop(&l).is_none());
    }

    #[test]
    fn cfg_back_edge_makes_the_iv_loop_carried() {
        // The whole point of Phase 9a: `induct` must now find the IV, because
        // the latch's def reaches the header's use through the back edge.
        let l = loop_with_n_body(2);
        let cfg = build_cfg_from_loop(&l).expect("CFG");
        let (annot, chains) = crate::fud::build_fud_chains(&cfg);
        let sc = crate::fud::scalar_dd(&annot, &chains);
        assert!(!sc.headers.is_empty(), "scalar_dd found no loop headers");
        let ind = crate::fud::induct(&annot, &chains);
        assert_eq!(ind.ivs.len(), 1, "expected exactly one IV");
        assert_eq!(annot.vars[ind.ivs[0].var].name, "i");
        assert_eq!(ind.ivs[0].a, 0, "IV starts at the loop lower bound");
        assert_eq!(ind.ivs[0].b, 1, "IV steps by the loop step");
        assert!(
            sc.situations
                .values()
                .any(|s| *s != crate::fud::ScalarDistance::Zero),
            "the IV read in the header must be loop-carried, not (0)"
        );
    }

    #[test]
    fn cfg_from_ir_preserves_statement_order() {
        let l = loop_with_n_body(5);
        let cfg = build_cfg_from_loop(&l).expect("CFG");
        assert_eq!(cfg.blocks[2].stmts.len(), 5);
        for (i, st) in cfg.blocks[2].stmts.iter().enumerate() {
            assert_eq!(st.id, i, "statement ids follow body order");
        }
    }

    #[test]
    fn cfg_succ_pred_are_consistent() {
        let l = loop_with_n_body(3);
        let cfg = build_cfg_from_loop(&l).expect("CFG");
        for (b, blk) in cfg.blocks.iter().enumerate() {
            for &s in &blk.succs {
                assert!(cfg.blocks[s].preds.contains(&b));
            }
            for &p in &blk.preds {
                assert!(cfg.blocks[p].succs.contains(&b));
            }
        }
    }

    #[test]
    fn cfg_matches_loop_accepts_matching_shape() {
        let l = loop_with_n_body(4);
        let cfg = build_cfg_from_loop(&l).expect("CFG");
        cfg_matches_loop(&cfg, &l).expect("shape matches");
    }

    #[test]
    fn cfg_matches_loop_rejects_wrong_statement_count() {
        let l = loop_with_n_body(4);
        let cfg = build_cfg_from_loop(&loop_with_n_body(3)).expect("CFG");
        let err = cfg_matches_loop(&cfg, &l).unwrap_err();
        assert!(err.contains("3 statements"), "got: {}", err);
    }

    #[test]
    fn cfg_matches_loop_rejects_the_back_edge_being_dropped() {
        // A CFG that lost its back edge is exactly the Phase 3 shape; the gate
        // must refuse it, because every Phase 4 analysis depends on that edge.
        let l = loop_with_n_body(4);
        let mut cfg = build_cfg_from_loop(&l).expect("CFG");
        cfg.blocks[1].preds.retain(|&p| p != 3);
        cfg.blocks[3].succs.clear();
        let err = cfg_matches_loop(&cfg, &l).unwrap_err();
        // Dropping the back edge leaves the latch as a dead end, so
        // `validate()` rejects it structurally before the shape check sees it.
        // Either refusal is correct — what matters is that the gate does not
        // accept a CFG with no back edge.
        assert!(
            err.contains("back edge") || err.contains("header") || err.contains("reach exit"),
            "got: {}",
            err
        );
    }

    #[test]
    fn cfg_matches_loop_rejects_a_header_with_only_the_back_edge_pred() {
        // The complementary case: the CFG stays structurally valid but the
        // entry→header edge is gone, so the shape check must fire.
        let l = loop_with_n_body(4);
        let mut cfg = build_cfg_from_loop(&l).expect("CFG");
        // Rewire: the entry feeds the body directly instead of the header, so
        // the header keeps only the back edge as an incoming edge. The graph is
        // still structurally valid (entry has no preds, every block reaches
        // exit), so the shape check is what must fire.
        cfg.blocks[0].succs = vec![2];
        cfg.blocks[2].preds = vec![0, 1];
        cfg.blocks[1].preds = vec![3];
        cfg.validate().expect("still a valid CFG");
        let err = cfg_matches_loop(&cfg, &l).unwrap_err();
        assert!(err.contains("entry and the latch"), "got: {}", err);
    }

    #[test]
    fn cfg_from_loop_lowers_declared_scalars_into_the_body() {
        // A loop whose frontend statements assign a declared scalar from the IV
        // must show that def in the body block.
        let mut l = loop_with_n_body(1);
        l.scalars = vec![
            ScalarVar { name: "i".into(), ty: ScalarTy::I64 },
            ScalarVar { name: "s".into(), ty: ScalarTy::I64 },
        ];
        l.stmts = Some(vec![LoopStmt {
            id: 0,
            kind: StmtKind::Assign { target: 1, expr: ScalarExpr::Iv(0) },
        }]);
        let cfg = build_cfg_from_loop(&l).expect("CFG");
        // `i` is already the IV at scalar 0; only `s` is appended.
        assert_eq!(cfg.vars.len(), 2);
        assert_eq!(cfg.vars[1].name, "s");
        cfg_matches_loop(&cfg, &l).expect("shape matches");
        let body = &cfg.blocks[2];
        assert_eq!(body.stmts.len(), 1);
        assert_eq!(body.stmts[0].defs.len(), 1);
        assert_eq!(body.stmts[0].defs[0].var, 1);
        assert_eq!(body.stmts[0].uses.len(), 1);
        assert_eq!(body.stmts[0].uses[0].var, 0);
        cfg.validate().expect("valid");
    }

    #[test]
    fn build_cfg_from_stmts_carries_uses_and_defs() {
        let vars = vec![
            ScalarVar { name: "x".into(), ty: ScalarTy::I64 },
            ScalarVar { name: "y".into(), ty: ScalarTy::I64 },
        ];
        let stmts = vec![
            Stmt {
                id: 0,
                uses: vec![ScalarUse { var: 1, stmt: 0, order: 0 }],
                defs: vec![ScalarDef { var: 0, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }],
            },
            Stmt {
                id: 1,
                uses: vec![ScalarUse { var: 0, stmt: 1, order: 0 }],
                defs: vec![],
            },
        ];
        let cfg = build_cfg_from_stmts(vars, stmts);
        cfg.validate().expect("valid");
        let ix = cfg.index();
        assert_eq!(ix.uses.len(), 2);
        assert_eq!(ix.defs.len(), 1);
        assert_eq!(ix.uses[0].var, 1);
        assert_eq!(ix.defs[0].var, 0);
    }

    #[test]
    fn fuzz_cfg_well_formed() {
        // Random loop shapes must always produce a well-formed CFG whose
        // succ/pred edges agree and whose statement order is the body order.
        let mut state: u64 = 0x5eed_c0de_1234_5678;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut tested = 0u64;
        for _ in 0..2000 {
            let n = (next() % 9) as usize; // 0..=8 accesses
            let l = loop_with_n_body(n);
            let cfg = build_cfg_from_loop(&l).expect("fuzz loop must have a CFG");
            cfg.validate().expect("fuzz CFG must be valid");
            assert_eq!(cfg.blocks[2].stmts.len(), n);
            for (i, st) in cfg.blocks[2].stmts.iter().enumerate() {
                assert_eq!(st.id, i);
            }
            cfg_matches_loop(&cfg, &l).expect("fuzz shape must match");
            tested += 1;
        }
        assert_eq!(tested, 2000);
    }

    #[test]
    fn fuzz_shuffled_defs_and_uses_positions() {
        // Random multi-statement scalar programs: positions recorded by
        // construction must agree with the canonical index.
        let mut state: u64 = 0xdead_beef_0bad_f00d;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..1000 {
            let nv = 1 + (next() % 3) as usize;
            let ns = (next() % 6) as usize;
            let mut stmts = Vec::new();
            for s in 0..ns {
                let nu = (next() % 3) as usize;
                let nd = (next() % 2) as usize;
                stmts.push(Stmt {
                    id: s,
                    uses: (0..nu)
                        .map(|_| ScalarUse { var: (next() as usize) % nv, stmt: s, order: 0 })
                        .collect(),
                    defs: (0..nd)
                        .map(|_| ScalarDef { var: (next() as usize) % nv, stmt: s, order: 0, rhs: ScalarRhs::Unknown })
                        .collect(),
                });
            }
            let vars: Vec<ScalarVar> = (0..nv)
                .map(|i| ScalarVar { name: format!("v{}", i), ty: ScalarTy::I64 })
                .collect();
            let cfg: CFG = build_cfg_from_stmts(vars, stmts);
            cfg.validate().expect("constructed CFG valid");
            let ix = cfg.index();
            // Every site agrees with its recorded scalar list entry.
            for (i, site) in ix.uses.iter().enumerate() {
                let u = &cfg.blocks[site.block].stmts[site.stmt].uses[site.order];
                assert_eq!(u.var, site.var);
                assert_eq!(u.var, cfg.use_var(UseId(i)));
            }
            for (i, site) in ix.defs.iter().enumerate() {
                let d = &cfg.blocks[site.block].stmts[site.stmt].defs[site.order];
                assert_eq!(d.var, site.var);
                assert_eq!(d.var, cfg.def_var(DefId(i)));
            }
        }
    }
}
