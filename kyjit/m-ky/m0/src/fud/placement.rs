//! φ-term placement (Wolfe §6.1.2 / §6.1.4, Phase 3.3).
//!
//! Wagner's join-set characterisation says a φ-term for variable `M` is
//! needed exactly at the iterated join points `J⁺(D(M))` of the blocks
//! defining `M`. The equivalence theorem `DF⁺(S) = J⁺(S)` (when
//! `Entry ∈ S`) lets us compute those points with the iterated dominance
//! frontier, which the previous module already provides:
//!
//! ```text
//! PlacementDirect(cfg, D, DFPlus):
//!     for each variable M:
//!         S = D[M]                      # blocks defining M
//!         for each W in DFPlus[S]:
//!             place φ-term for M at W
//! ```
//!
//! The brute-force fuzz test in this module computes `J⁺` directly from the
//! definition (disjoint simple paths, no other definition blocks on either
//! path) and compares it against `DF⁺` on small DAGs, which is the check
//! that the theorem is actually doing the work we claim.

use std::collections::{BTreeSet, HashMap};

use crate::ir::{PhiTerm, CFG};

use super::dominators::{df_plus_of_set, DominanceFrontiers};

/// Which (variable, block) pairs need a φ-term.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhiPlacement {
    /// Sorted (variable, block) pairs, so placement is deterministic.
    pub phi_at: BTreeSet<(usize, usize)>,
}

impl PhiPlacement {
    pub fn is_empty(&self) -> bool {
        self.phi_at.is_empty()
    }

    pub fn len(&self) -> usize {
        self.phi_at.len()
    }
}

/// The blocks that define each variable (from the CFG's def records).
pub fn definitions_by_var(cfg: &CFG) -> HashMap<usize, BTreeSet<usize>> {
    let mut out: HashMap<usize, BTreeSet<usize>> = HashMap::new();
    for (b, blk) in cfg.blocks.iter().enumerate() {
        for st in &blk.stmts {
            for d in &st.defs {
                out.entry(d.var).or_default().insert(b);
            }
        }
    }
    out
}

/// Place φ-terms at `DF⁺(D(M))` for every variable `M` (Wolfe §6.1.4,
/// "PlacementDirect"). Because `DF⁺(S) = J⁺(S)` under `Entry ∈ S`, this
/// direct computation is the iterated-join worklist's fixed point.
pub fn place_phi_terms(
    cfg: &CFG,
    definitions: &HashMap<usize, BTreeSet<usize>>,
    frontiers: &DominanceFrontiers,
) -> PhiPlacement {
    let mut phi_at = BTreeSet::new();
    for (&var, blocks) in definitions.iter() {
        // Note: the theorem's hypothesis is Entry ∈ S; the standard SSA
        // argument is that a definition in Entry behaves like the implicit
        // "undefined" value plus the definition, and the frontier treatment
        // is identical either way for placement. We additionally include the
        // entry block in the frontier computation when entry itself defines
        // the variable -- in that case entry ∈ S already.
        for w in df_plus_of_set(frontiers, blocks) {
            // A φ-term needs at least two predecessors to merge.
            if cfg.blocks[w].preds.len() >= 2 {
                phi_at.insert((var, w));
            }
        }
    }
    PhiPlacement { phi_at }
}

/// Insert the placed φ-terms into the CFG (a copy with `block.phi` filled).
pub fn apply_placement(cfg: &CFG, placement: &PhiPlacement) -> CFG {
    let mut out = cfg.clone();
    for &(var, block) in &placement.phi_at {
        out.blocks[block].phi.push(PhiTerm { var, block });
    }
    out
}

/// All φ-terms placed for `var` in the CFG, as block indices.
pub fn phi_blocks_for(cfg: &CFG, var: usize) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for (b, blk) in cfg.blocks.iter().enumerate() {
        for p in &blk.phi {
            if p.var == var {
                out.insert(b);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::dominators::{compute_dominance_frontiers, compute_dominators};
    use crate::fud::testgen::{gen_cfg, gen_cfg_dag, Rng};
    use crate::ir::cfg::{Block, Stmt, CFG};
    use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarVar, ScalarUse};

    fn blk(id: usize, preds: Vec<usize>, succs: Vec<usize>) -> Block {
        Block { id, preds, succs, stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] }
    }

    /// A definition of `var` as statement 0 of the block.
    fn def_stmt(var: usize) -> Stmt {
        Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] }
    }

    /// Entry → B1(def x) → B2(def x) → exit (linear, single def chain).
    fn linear_cfg() -> CFG {
        let mut b1 = blk(1, vec![0], vec![2]);
        b1.stmts.push(def_stmt(0));
        let mut b2 = blk(2, vec![1], vec![3]);
        b2.stmts.push(Stmt {
            id: 0,
            uses: vec![ScalarUse { var: 0, stmt: 0, order: 0 }],
            defs: vec![],
        });
        CFG {
            vars: vec![ScalarVar { name: "x".into(), ty: ScalarTy::I64 }],
            blocks: vec![blk(0, vec![], vec![1]), b1, b2, blk(3, vec![2], vec![])],
            entry: 0,
            exit: 3,
        }
    }

    /// Diamond: 0 → 1 → {2, 3} → 4; defs of x in 2 and 3.
    fn diamond_two_defs() -> CFG {
        let mut b2 = blk(2, vec![1], vec![4]);
        b2.stmts.push(def_stmt(0));
        let mut b3 = blk(3, vec![1], vec![4]);
        b3.stmts.push(def_stmt(0));
        CFG {
            vars: vec![ScalarVar { name: "x".into(), ty: ScalarTy::I64 }],
            blocks: vec![
                blk(0, vec![], vec![1]),
                blk(1, vec![0], vec![2, 3]),
                b2,
                b3,
                blk(4, vec![2, 3], vec![]),
            ],
            entry: 0,
            exit: 4,
        }
    }

    /// Nested diamonds: 0→1; 1→{2,3}; 2→4; 3→4; 4→{5,6}; 5→7; 6→7.
    /// Defs of x in 2, 3 and 5, 6 → φ expected at 4 and 7.
    fn nested_diamonds() -> CFG {
        let mut b2 = blk(2, vec![1], vec![4]);
        b2.stmts.push(def_stmt(0));
        let mut b3 = blk(3, vec![1], vec![4]);
        b3.stmts.push(def_stmt(0));
        let mut b5 = blk(5, vec![4], vec![7]);
        b5.stmts.push(def_stmt(0));
        let mut b6 = blk(6, vec![4], vec![7]);
        b6.stmts.push(def_stmt(0));
        CFG {
            vars: vec![ScalarVar { name: "x".into(), ty: ScalarTy::I64 }],
            blocks: vec![
                blk(0, vec![], vec![1]),
                blk(1, vec![0], vec![2, 3]),
                b2,
                b3,
                blk(4, vec![2, 3], vec![5, 6]),
                b5,
                b6,
                blk(7, vec![5, 6], vec![]),
            ],
            entry: 0,
            exit: 7,
        }
    }

    fn place(cfg: &CFG) -> PhiPlacement {
        let defs = definitions_by_var(cfg);
        let dom = compute_dominators(cfg);
        let fr = compute_dominance_frontiers(cfg, &dom);
        place_phi_terms(cfg, &defs, &fr)
    }

    #[test]
    fn phi_at_single_def_is_none() {
        // Linear chain, one def (well, one def and one use): no join point.
        let cfg = linear_cfg();
        let p = place(&cfg);
        assert!(p.is_empty(), "unexpected φ terms: {:?}", p.phi_at);
    }

    #[test]
    fn phi_at_two_defs_in_branches() {
        let cfg = diamond_two_defs();
        let p = place(&cfg);
        assert_eq!(p.phi_at, [(0usize, 4usize)].into_iter().collect());
        let with = apply_placement(&cfg, &p);
        with.validate().expect("placement keeps the CFG valid");
        assert_eq!(phi_blocks_for(&with, 0), [4].into_iter().collect());
    }

    #[test]
    fn phi_at_nested_branches() {
        let cfg = nested_diamonds();
        let p = place(&cfg);
        assert_eq!(
            p.phi_at,
            [(0usize, 4usize), (0usize, 7usize)].into_iter().collect()
        );
        let with = apply_placement(&cfg, &p);
        with.validate().expect("placement keeps the CFG valid");
    }

    // ---- Brute-force oracles ----

    fn reachable_avoiding(cfg: &CFG, target: usize, avoid: usize) -> bool {
        if target == avoid || cfg.entry == avoid {
            return false;
        }
        let mut seen = vec![false; cfg.blocks.len()];
        let mut stack = vec![cfg.entry];
        seen[cfg.entry] = true;
        while let Some(x) = stack.pop() {
            if x == target {
                return true;
            }
            for &s in &cfg.blocks[x].succs {
                if s != avoid && !seen[s] {
                    seen[s] = true;
                    stack.push(s);
                }
            }
        }
        false
    }

    fn brute_doms(cfg: &CFG) -> Vec<BTreeSet<usize>> {
        let n = cfg.blocks.len();
        (0..n)
            .map(|y| (0..n).filter(|&x| x == y || !reachable_avoiding(cfg, y, x)).collect())
            .collect()
    }

    fn brute_df(cfg: &CFG, doms: &[BTreeSet<usize>]) -> Vec<BTreeSet<usize>> {
        let n = cfg.blocks.len();
        let mut df = vec![BTreeSet::new(); n];
        for z in 0..n {
            let preds = &cfg.blocks[z].preds;
            for &y1 in preds {
                for &y2 in preds {
                    if y1 == y2 {
                        continue;
                    }
                    for x in 0..n {
                        if doms[y1].contains(&x) && !doms[y2].contains(&x) {
                            df[x].insert(z);
                        }
                    }
                }
            }
        }
        df
    }

    /// `DF⁺(S)` from brute dominators (independent of the CHK path).
    /// Starts from `DF(S)` and takes the closure — `S` is *not* seeded.
    fn brute_df_plus_for(cfg: &CFG, s: &BTreeSet<usize>) -> BTreeSet<usize> {
        let doms = brute_doms(cfg);
        let df = brute_df(cfg, &doms);
        let mut out = BTreeSet::new();
        let mut stack: Vec<usize> = s
            .iter()
            .flat_map(|x| df[*x].iter().copied())
            .collect();
        while let Some(y) = stack.pop() {
            if out.insert(y) {
                for &z in &df[y] {
                    stack.push(z);
                }
            }
        }
        out
    }

    /// All simple paths `from → to` (inclusive), for the join-set oracle.
    fn all_paths(cfg: &CFG, from: usize, to: usize) -> Vec<Vec<usize>> {
        fn rec(cfg: &CFG, cur: usize, to: usize, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
            path.push(cur);
            if cur == to {
                out.push(path.clone());
            } else {
                for i in 0..cfg.blocks[cur].succs.len() {
                    let s = cfg.blocks[cur].succs[i];
                    if !path.contains(&s) {
                        rec(cfg, s, to, path, out);
                    }
                }
            }
            path.pop();
        }
        let mut out = Vec::new();
        rec(cfg, from, to, &mut Vec::new(), &mut out);
        out
    }

    /// `J(X1, X2)`: merge points shared by two defs via disjoint paths.
    fn brute_j_of_pair(cfg: &CFG, x1: usize, x2: usize) -> BTreeSet<usize> {
        let mut j = BTreeSet::new();
        for z in 0..cfg.blocks.len() {
            let preds = &cfg.blocks[z].preds;
            if preds.len() < 2 {
                continue;
            }
            'join: for &y1 in preds.iter() {
                for &y2 in preds.iter() {
                    if y1 == y2 {
                        continue;
                    }
                    for p1 in all_paths(cfg, x1, y1) {
                        let s1: BTreeSet<usize> = p1.iter().copied().collect();
                        for p2 in all_paths(cfg, x2, y2) {
                            let s2: BTreeSet<usize> = p2.iter().copied().collect();
                            if s1.is_disjoint(&s2) {
                                j.insert(z);
                                break 'join;
                            }
                        }
                    }
                }
            }
        }
        j
    }

    /// `J⁺(S)`: iterated join set (fixpoint of `S ↦ S ∪ J(S)`).
    fn brute_j_plus(cfg: &CFG, s: &BTreeSet<usize>) -> BTreeSet<usize> {
        let mut jp = BTreeSet::new();
        let mut cur = s.clone();
        loop {
            let mut j = BTreeSet::new();
            let items: Vec<usize> = cur.iter().copied().collect();
            for i in 0..items.len() {
                for k in i + 1..items.len() {
                    for z in brute_j_of_pair(cfg, items[i], items[k]) {
                        j.insert(z);
                    }
                }
            }
            let mut grew = false;
            for z in &j {
                if jp.insert(*z) {
                    grew = true;
                }
            }
            if !grew {
                break;
            }
            cur.extend(j);
        }
        jp
    }

    // ---- Fuzz tests ----

    #[test]
    fn fuzz_phi_placement_vs_bruteforce() {
        let mut rng = Rng::new(0xf0_4c_4e_5e_e001);
        let mut tested = 0u64;
        for _ in 0..800 {
            let cfg = gen_cfg_dag(&mut rng, 6, 1);
            cfg.validate().expect("generated DAG valid");
            let defs = definitions_by_var(&cfg);
            let dom = compute_dominators(&cfg);
            let fr = compute_dominance_frontiers(&cfg, &dom);
            let p = place_phi_terms(&cfg, &defs, &fr);

            let mut expected = BTreeSet::new();
            for (&var, blocks) in &defs {
                for w in brute_df_plus_for(&cfg, blocks) {
                    if cfg.blocks[w].preds.len() >= 2 {
                        expected.insert((var, w));
                    }
                }
            }
            assert_eq!(p.phi_at, expected, "placement mismatch\ncfg={:?}", cfg);
            tested += 1;
        }
        println!("fuzz_phi_placement_vs_bruteforce: tested={}", tested);
    }

    #[test]
    fn fuzz_dominant_frontier_equals_join_set() {
        // Wolfe §6.1.3: DF⁺(S) = J⁺(S) when Entry ∈ S. Both sides computed
        // from their own definitions on small DAGs.
        let mut rng = Rng::new(0x10_e0_a7_5e_7e_0001);
        let mut tested = 0u64;
        for _ in 0..400 {
            let cfg = gen_cfg_dag(&mut rng, 6, 1);
            let defs = definitions_by_var(&cfg);
            if defs.is_empty() {
                continue;
            }
            for blocks in defs.values() {
                let mut s = blocks.clone();
                s.insert(cfg.entry);
                let dfp = brute_df_plus_for(&cfg, &s);
                let jp = brute_j_plus(&cfg, &s);
                assert_eq!(dfp, jp, "DF+ != J+ for S={:?}\ncfg={:?}", s, cfg);
                tested += 1;
            }
        }
        println!("fuzz_dominant_frontier_equals_join_set: tested={}", tested);
    }

    #[test]
    fn fuzz_phi_no_duplicates() {
        let mut rng = Rng::new(0xd0_4e_e0_5eed_0001);
        let mut tested = 0u64;
        for _ in 0..800 {
            let cfg = gen_cfg(&mut rng, 7, 1);
            cfg.validate().expect("generated CFG valid");
            let defs = definitions_by_var(&cfg);
            let dom = compute_dominators(&cfg);
            let fr = compute_dominance_frontiers(&cfg, &dom);
            let p = place_phi_terms(&cfg, &defs, &fr);

            // One φ per (variable, block): the BTreeSet cannot hold duplicates,
            // and after applying, no block has two φ for the same variable.
            let with = apply_placement(&cfg, &p);
            with.validate().expect("applied placement stays valid");
            for &(var, block) in &p.phi_at {
                assert!(defs.contains_key(&var), "φ for a variable with no defs");
                assert!(
                    cfg.blocks[block].preds.len() >= 2,
                    "φ at a non-merge block {}",
                    block
                );
            }
            tested += 1;
        }
        println!("fuzz_phi_no_duplicates: tested={}", tested);
    }
}

