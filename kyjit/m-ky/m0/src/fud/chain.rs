//! Algorithm Chain (Wolfe §6.1.5, Phase 3.4).
//!
//! Links every scalar use to the unique definition — or φ-term — whose value
//! it sees, and records the def-def link (Wolfe §6.2) that lets a walker
//! enumerate all reaching definitions.
//!
//! ```text
//! Search(X):
//!     for each use / def / φ-term R in X, in order:
//!         M = variable at R
//!         if R is a use:     Chain[R] = CurrDef[M]
//!         else:              SaveChain[R] = CurrDef[M]; CurrDef[M] = R
//!     for Y in SUCC(X):
//!         J = WhichPred(X → Y)
//!         for each φ-term R in Y: φ-Chain[R][J] = CurrDef[M]
//!     for Y in Child(X): Search(Y)
//!     for each use / def / φ-term R in X, in reverse order:
//!         if R is a def or φ-term: CurrDef[M] = SaveChain[R]
//! ```
//!
//! **The reverse pass is not optional.** `CurrDef` is a single map shared by
//! the whole traversal; descending into a dominator child may redefine `M`,
//! and unless the child restores `CurrDef[M]` on the way out, the *next*
//! sibling sees the wrong definition. `chain_reverse_pass_restores` and
//! `fuzz_chain_vs_bruteforce` exist to catch exactly that bug.

use std::collections::HashMap;

use crate::ir::scalar::{DefId, PhiId, UseId};
use crate::ir::CFG;

use super::dominators::{compute_dominance_frontiers, compute_dominators, DominatorTree};
use super::placement::{apply_placement, definitions_by_var, place_phi_terms};

/// What a use reaches: a concrete definition, or the φ-term that merges the
/// definitions arriving at a join point. (A φ must then be followed back
/// through its incoming links — see [`super::reaching`].)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainLink {
    Def(DefId),
    Phi(PhiId),
}

/// The forward factored use-def chain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FudChain {
    /// use → the definition (or φ-term) whose value it sees. A use with no
    /// reaching definition is **absent** (it is uninitialized).
    pub use_to_def: HashMap<UseId, ChainLink>,
    /// φ-term → one link per predecessor, in predecessor order.
    pub phi_incoming: HashMap<PhiId, Vec<Option<ChainLink>>>,
    /// def → the link it overwrote (reverse-pass restore state).
    pub def_to_save: HashMap<DefId, Option<ChainLink>>,
    /// φ-term → the link it overwrote (reverse-pass restore state).
    pub phi_to_save: HashMap<PhiId, Option<ChainLink>>,
    /// def → the previous definition of the same variable (Wolfe §6.2
    /// def-def link; used by `FollowChain`).
    pub def_to_prev: HashMap<DefId, Option<DefId>>,
}

impl FudChain {
    /// The direct reaching definition of a use, when the link is a plain def
    /// (the single-block case). `None` for an absent link or a φ link.
    pub fn direct_def(&self, u: UseId) -> Option<DefId> {
        match self.use_to_def.get(&u) {
            Some(ChainLink::Def(d)) => Some(*d),
            _ => None,
        }
    }

    /// Does the chain claim a reaching definition for `u`?
    pub fn has_link(&self, u: UseId) -> bool {
        self.use_to_def.contains_key(&u)
    }
}

#[derive(Clone, Copy)]
enum Item {
    Use(UseId),
    Def(DefId),
    Phi(PhiId),
}

/// Build the chain for a CFG whose φ-terms have already been placed.
pub fn construct_chains(cfg: &CFG, dom: &DominatorTree) -> FudChain {
    let ix = cfg.index();
    let use_var: Vec<usize> = ix.uses.iter().map(|s| s.var).collect();
    let def_var: Vec<usize> = ix.defs.iter().map(|s| s.var).collect();
    let phi_var: Vec<usize> = ix.phis.iter().map(|s| s.var).collect();

    let mut use_at: HashMap<(usize, usize, usize), usize> = HashMap::new();
    for (i, s) in ix.uses.iter().enumerate() {
        use_at.insert((s.block, s.stmt, s.order), i);
    }
    let mut def_at: HashMap<(usize, usize, usize), usize> = HashMap::new();
    for (i, s) in ix.defs.iter().enumerate() {
        def_at.insert((s.block, s.stmt, s.order), i);
    }
    let mut phi_by_block: HashMap<usize, Vec<PhiId>> = HashMap::new();
    for (i, s) in ix.phis.iter().enumerate() {
        phi_by_block.entry(s.block).or_default().push(PhiId(i));
    }

    // Items of a block: φ-terms first, then statements (uses then defs).
    let mut items: HashMap<usize, Vec<Item>> = HashMap::new();
    for (b, blk) in cfg.blocks.iter().enumerate() {
        let mut list = Vec::new();
        if let Some(ps) = phi_by_block.get(&b) {
            for &p in ps {
                list.push(Item::Phi(p));
            }
        }
        for st in &blk.stmts {
            for o in 0..st.uses.len() {
                if let Some(&idx) = use_at.get(&(b, st.id, o)) {
                    list.push(Item::Use(UseId(idx)));
                }
            }
            for o in 0..st.defs.len() {
                if let Some(&idx) = def_at.get(&(b, st.id, o)) {
                    list.push(Item::Def(DefId(idx)));
                }
            }
        }
        items.insert(b, list);
    }

    let mut phi_incoming: HashMap<PhiId, Vec<Option<ChainLink>>> = HashMap::new();
    for (i, site) in ix.phis.iter().enumerate() {
        phi_incoming.insert(PhiId(i), vec![None; cfg.blocks[site.block].preds.len()]);
    }

    let mut chain = FudChain {
        use_to_def: HashMap::new(),
        phi_incoming,
        def_to_save: HashMap::new(),
        phi_to_save: HashMap::new(),
        def_to_prev: HashMap::new(),
    };
    let mut curr: HashMap<usize, ChainLink> = HashMap::new();
    let ctx = SearchCtx {
        cfg,
        dom,
        items: &items,
        phi_by_block: &phi_by_block,
        use_var: &use_var,
        def_var: &def_var,
        phi_var: &phi_var,
    };
    search(cfg.entry, &ctx, &mut curr, &mut chain);
    chain
}

/// Everything `Search` reads (bundled to keep the recursion signature small).
struct SearchCtx<'a> {
    cfg: &'a CFG,
    dom: &'a DominatorTree,
    items: &'a HashMap<usize, Vec<Item>>,
    phi_by_block: &'a HashMap<usize, Vec<PhiId>>,
    use_var: &'a [usize],
    def_var: &'a [usize],
    phi_var: &'a [usize],
}

fn var_of(it: &Item, ctx: &SearchCtx) -> usize {
    match it {
        Item::Use(u) => ctx.use_var[u.0],
        Item::Def(d) => ctx.def_var[d.0],
        Item::Phi(p) => ctx.phi_var[p.0],
    }
}

/// Algorithm Chain, recursive over the dominator tree. The reverse pass at
/// the end of each block is what keeps `CurrDef` correct for later siblings.
fn search(
    b: usize,
    ctx: &SearchCtx,
    curr: &mut HashMap<usize, ChainLink>,
    chain: &mut FudChain,
) {
    let empty: Vec<Item> = Vec::new();
    let block_items = ctx.items.get(&b).unwrap_or(&empty);

    // Forward pass.
    for it in block_items {
        let m = var_of(it, ctx);
        match it {
            Item::Use(u) => {
                if let Some(link) = curr.get(&m) {
                    chain.use_to_def.insert(*u, *link);
                }
            }
            Item::Def(d) => {
                let prev = curr.get(&m).copied();
                chain.def_to_save.insert(*d, prev);
                chain.def_to_prev.insert(
                    *d,
                    match prev {
                        Some(ChainLink::Def(pd)) => Some(pd),
                        _ => None,
                    },
                );
                curr.insert(m, ChainLink::Def(*d));
            }
            Item::Phi(p) => {
                let prev = curr.get(&m).copied();
                chain.phi_to_save.insert(*p, prev);
                curr.insert(m, ChainLink::Phi(*p));
            }
        }
    }

    // Record CurrDef into each successor's φ-terms.
    for &y in &ctx.cfg.blocks[b].succs {
        let j = ctx.cfg.blocks[y]
            .preds
            .iter()
            .position(|&p| p == b)
            .expect("succ/pred symmetry");
        if let Some(ps) = ctx.phi_by_block.get(&y) {
            for &p in ps {
                let m = ctx.phi_var[p.0];
                let val = curr.get(&m).copied();
                if let Some(slots) = chain.phi_incoming.get_mut(&p) {
                    slots[j] = val;
                }
            }
        }
    }

    // Recurse over dominator children.
    for &child in &ctx.dom.children[b] {
        search(child, ctx, curr, chain);
    }

    // Reverse pass: restore CurrDef.
    for it in block_items.iter().rev() {
        let m = var_of(it, ctx);
        match it {
            Item::Def(d) => {
                match chain.def_to_save.get(d).copied().flatten() {
                    Some(link) => {
                        curr.insert(m, link);
                    }
                    None => {
                        curr.remove(&m);
                    }
                }
            }
            Item::Phi(p) => {
                match chain.phi_to_save.get(p).copied().flatten() {
                    Some(link) => {
                        curr.insert(m, link);
                    }
                    None => {
                        curr.remove(&m);
                    }
                }
            }
            Item::Use(_) => {}
        }
    }
}

/// Convenience: place φ-terms, apply them, and build the chain, returning the
/// φ-annotated CFG together with its chain. This is the shape a Phase 3
/// certificate carries (the CFG) and the checker rebuilds (the chain).
pub fn build_chain_with_phi(cfg: &CFG) -> (CFG, FudChain) {
    let dom = compute_dominators(cfg);
    let frontiers = compute_dominance_frontiers(cfg, &dom);
    let defs = definitions_by_var(cfg);
    let placement = place_phi_terms(cfg, &defs, &frontiers);
    let annotated = apply_placement(cfg, &placement);
    // Placement may add φ-terms, but never edges, so the dominator tree is
    // unchanged; recompute anyway to keep the pair self-consistent.
    let dom2 = compute_dominators(&annotated);
    let chain = construct_chains(&annotated, &dom2);
    (annotated, chain)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::testgen::{gen_cfg_dag, Rng};
    use crate::fud::placement::phi_blocks_for;
    use crate::ir::cfg::{Block, CFG};
    use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar};
    use crate::ir::Stmt;

    fn var(n: &str) -> ScalarVar {
        ScalarVar { name: n.into(), ty: ScalarTy::I64 }
    }

    /// Build a block whose statements are `(uses, defs)` variable lists, with
    /// canonical statement ids and use/def positions.
    fn mk_block(
        id: usize,
        preds: Vec<usize>,
        succs: Vec<usize>,
        spec: Vec<(Vec<usize>, Vec<usize>)>,
    ) -> Block {
        let stmts = spec
            .into_iter()
            .enumerate()
            .map(|(s, (uses, defs))| Stmt {
                id: s,
                uses: uses
                    .into_iter()
                    .enumerate()
                    .map(|(o, v)| ScalarUse { var: v, stmt: s, order: o })
                    .collect(),
                defs: defs
                    .into_iter()
                    .enumerate()
                    .map(|(o, v)| ScalarDef { var: v, stmt: s, order: o, rhs: ScalarRhs::Unknown })
                    .collect(),
            })
            .collect();
        Block { id, preds, succs, stmts, phi: vec![], upsilon: vec![], guards: vec![] }
    }

    fn def_stmt(v: usize) -> Stmt {
        Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var: v, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] }
    }
    fn use_stmt(v: usize) -> Stmt {
        Stmt { id: 0, uses: vec![ScalarUse { var: v, stmt: 0, order: 0 }], defs: vec![] }
    }

    #[test]
    fn chain_linear_is_simple() {
        let cfg = CFG::new_single_block(
            vec![var("x")],
            vec![def_stmt(0), use_stmt(0), def_stmt(0), use_stmt(0)],
        );
        let (_, chain) = build_chain_with_phi(&cfg);
        // ix uses: stmt1 -> UseId0, stmt3 -> UseId1; defs: stmt0->DefId0, stmt2->DefId1.
        assert_eq!(chain.direct_def(UseId(0)), Some(DefId(0)));
        assert_eq!(chain.direct_def(UseId(1)), Some(DefId(1)));
        assert_eq!(chain.def_to_prev.get(&DefId(1)).copied(), Some(Some(DefId(0))));
        assert_eq!(chain.def_to_prev.get(&DefId(0)).copied(), Some(None));
    }

    #[test]
    fn chain_use_before_def_is_uninitialized() {
        let cfg = CFG::new_single_block(vec![var("x")], vec![use_stmt(0), def_stmt(0)]);
        let (_, chain) = build_chain_with_phi(&cfg);
        assert!(!chain.has_link(UseId(0)), "use before def must be uninitialized");
    }

    #[test]
    fn chain_reverse_pass_restores() {
        // 0→1; 1→{2,3}; 2→4; 3→4. def x in 0; def x in 2; use x in 3.
        // Block 3's use must see the entry def (block 0), not block 2's.
        // Without the reverse pass, Search(2) leaves CurrDef[x] = d2 and
        // block 3's use wrongly reaches d2.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                mk_block(0, vec![], vec![1], vec![(vec![], vec![0])]),
                mk_block(1, vec![0], vec![2, 3], vec![]),
                mk_block(2, vec![1], vec![4], vec![(vec![], vec![0])]),
                mk_block(3, vec![1], vec![4], vec![(vec![0], vec![])]),
                mk_block(4, vec![2, 3], vec![], vec![]),
            ],
            entry: 0,
            exit: 4,
        };
        let (_, chain) = build_chain_with_phi(&cfg);
        assert_eq!(chain.direct_def(UseId(0)), Some(DefId(0)));
    }

    #[test]
    fn chain_merge_goes_through_phi() {
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                mk_block(0, vec![], vec![1], vec![]),
                mk_block(1, vec![0], vec![2, 3], vec![]),
                mk_block(2, vec![1], vec![4], vec![(vec![], vec![0])]),
                mk_block(3, vec![1], vec![4], vec![(vec![], vec![0])]),
                mk_block(4, vec![2, 3], vec![], vec![(vec![0], vec![])]),
            ],
            entry: 0,
            exit: 4,
        };
        let (annotated, chain) = build_chain_with_phi(&cfg);
        assert_eq!(phi_blocks_for(&annotated, 0), [4].into_iter().collect());
        match chain.use_to_def.get(&UseId(0)) {
            Some(ChainLink::Phi(p)) => {
                assert_eq!(
                    chain.phi_incoming.get(p).cloned(),
                    Some(vec![Some(ChainLink::Def(DefId(0))), Some(ChainLink::Def(DefId(1)))])
                );
            }
            other => panic!("expected a φ link, got {:?}", other),
        }
    }

    #[test]
    fn chain_branch_takes_correct_def() {
        // Each branch defines then uses x (def stmt then use stmt).
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                mk_block(0, vec![], vec![1], vec![]),
                mk_block(1, vec![0], vec![2, 3], vec![]),
                mk_block(2, vec![1], vec![4], vec![(vec![], vec![0]), (vec![0], vec![])]),
                mk_block(3, vec![1], vec![4], vec![(vec![], vec![0]), (vec![0], vec![])]),
                mk_block(4, vec![2, 3], vec![], vec![]),
            ],
            entry: 0,
            exit: 4,
        };
        let (annotated, chain) = build_chain_with_phi(&cfg);
        annotated.validate().expect("valid CFG");
        // ix: block2 def->DefId0, block2 use->UseId0; block3 def->DefId1, block3 use->UseId1.
        assert_eq!(chain.direct_def(UseId(0)), Some(DefId(0)));
        assert_eq!(chain.direct_def(UseId(1)), Some(DefId(1)));
    }

    #[test]
    fn fuzz_chain_single_block_vs_bruteforce() {
        // Single-block loops (the shape the current IR produces): the chain
        // must be exactly "each use sees the immediately preceding def of the
        // same variable". The brute oracle scans statements in order.
        let mut rng = Rng::new(0xc4a1_5eed_c0de_0001);
        let mut tested = 0u64;
        for _ in 0..3000 {
            let nv = 1 + rng.below(3) as usize;
            let ns = rng.below(6) as usize;
            let mut stmts = Vec::new();
            for s in 0..ns {
                let nu = rng.below(3) as usize;
                let nd = rng.below(2) as usize;
                stmts.push(Stmt {
                    id: s,
                    uses: (0..nu)
                        .map(|o| ScalarUse { var: rng.below(nv as u64) as usize, stmt: s, order: o })
                        .collect(),
                    defs: (0..nd)
                        .map(|o| ScalarDef { var: rng.below(nv as u64) as usize, stmt: s, order: o, rhs: ScalarRhs::Unknown })
                        .collect(),
                });
            }
            let vars: Vec<ScalarVar> = (0..nv)
                .map(|i| ScalarVar { name: format!("v{}", i), ty: ScalarTy::I64 })
                .collect();
            let cfg = CFG::new_single_block(vars, stmts);
            let (annotated, chain) = build_chain_with_phi(&cfg);
            let ix = annotated.index();

            // Brute: walk statements in order, tracking the last def per var.
            let mut last: Vec<Option<usize>> = vec![None; nv];
            let mut def_counter = 0usize;
            let mut expected: Vec<Option<usize>> = Vec::new();
            for st in &annotated.blocks[1].stmts {
                for u in &st.uses {
                    expected.push(last[u.var]);
                }
                for d in &st.defs {
                    last[d.var] = Some(def_counter);
                    def_counter += 1;
                }
            }
            assert_eq!(ix.uses.len(), expected.len());
            for (idx, want) in expected.iter().enumerate() {
                match (want, chain.direct_def(UseId(idx))) {
                    (Some(d), got) => assert_eq!(got, Some(DefId(*d)), "use {} mismatch", idx),
                    (None, got) => assert_eq!(got, None, "use {} should be uninitialized", idx),
                }
            }
            tested += 1;
        }
        println!("fuzz_chain_single_block_vs_bruteforce: tested={}", tested);
    }

    #[test]
    fn fuzz_chain_dag_def_link_dominates_use() {
        // General-CFG invariant: when a use reaches a plain def, that def's
        // block must dominate the use's block (the chain only carries a
        // definition forward along the dominator tree). Catches wrong links
        // on branching CFGs.
        let mut rng = Rng::new(0xd06a_1e5e_ed00_0001);
        let mut tested = 0u64;
        for _ in 0..1500 {
            let cfg = gen_cfg_dag(&mut rng, 6, 1);
            cfg.validate().expect("generated DAG valid");
            let (annotated, chain) = build_chain_with_phi(&cfg);
            let dom = compute_dominators(&annotated);
            let ix = annotated.index();
            for (idx, _site) in ix.uses.iter().enumerate() {
                let u = UseId(idx);
                if let Some(ChainLink::Def(d)) = chain.use_to_def.get(&u) {
                    assert_eq!(ix.defs[d.0].var, ix.uses[idx].var, "var mismatch");
                    let db = ix.defs[d.0].block;
                    let ub = ix.uses[idx].block;
                    assert!(
                        dom.dominates(db, ub),
                        "reaching def block {} does not dominate use block {}",
                        db,
                        ub
                    );
                }
            }
            tested += 1;
        }
        println!("fuzz_chain_dag_def_link_dominates_use: tested={}", tested);
    }
}
