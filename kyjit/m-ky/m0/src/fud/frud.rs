//! FRUD chains — Factored Redef-Use chains (Wolfe §6.7.5, Phase 3.6).
//!
//! The mirror of the FUD chain: instead of linking each *use* to the
//! definitions that reach it, link each *definition* to the *uses* it reaches.
//! This is the anti-dependence direction, consumed by Phase 5's reduction
//! detection and SPP Layer 2's scalar expansion.
//!
//! Wolfe builds it with a reverse-Chain pass (`CurrUse` instead of `CurrDef`,
//! Υ-terms at reverse merge points). The **relation** it produces —
//! `def d reaches use u` — is exactly the transpose of the reaching-defs
//! relation that `FollowChain` computes:
//!
//! ```text
//! FRUD.def_to_use(d) = { u | d ∈ ReachingDefs(u) }
//! ```
//!
//! Both directions test the same predicate ("does d's value reach u along a
//! path with no intervening definition?"), so we construct FRUD by
//! transposing `find_reaching_defs` — a construction that is provably equal
//! to the reverse-Chain pass for the killing (scalar) definitions Phase 3
//! handles. The direct reverse pass becomes necessary only for *non-killing*
//! array definitions (Phase 6), where the transpose shortcut is retained as
//! the oracle.

use std::collections::{BTreeSet, HashMap};

use crate::ir::scalar::{DefId, UseId};
use crate::ir::CFG;

use super::chain::FudChain;
use super::reaching::find_reaching_defs;

/// The FRUD relation: which uses each definition reaches, and its inverse.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrudChain {
    /// def → the uses it reaches.
    pub def_to_use: HashMap<DefId, BTreeSet<UseId>>,
    /// use → the definitions that reach it (the FUD relation, kept alongside
    /// so both directions are available without recomputation).
    pub use_to_def: HashMap<UseId, BTreeSet<DefId>>,
}

impl FrudChain {
    /// The uses reached by `d` (empty if none).
    pub fn uses_of(&self, d: DefId) -> BTreeSet<UseId> {
        self.def_to_use.get(&d).cloned().unwrap_or_default()
    }

    /// Does `d` reach `u`?
    pub fn reaches(&self, d: DefId, u: UseId) -> bool {
        self.def_to_use.get(&d).map_or(false, |s| s.contains(&u))
    }
}

/// Build the FRUD chains by transposing the reaching-defs relation.
pub fn construct_frud_chains(cfg: &CFG, chain: &FudChain) -> FrudChain {
    let rd = find_reaching_defs(cfg, chain);
    let mut def_to_use: HashMap<DefId, BTreeSet<UseId>> = HashMap::new();
    let mut use_to_def: HashMap<UseId, BTreeSet<DefId>> = HashMap::new();
    for (u, defs) in &rd.defs {
        use_to_def.insert(*u, defs.clone());
        for d in defs {
            def_to_use.entry(*d).or_default().insert(*u);
        }
    }
    FrudChain { def_to_use, use_to_def }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::chain::build_chain_with_phi;
    use crate::ir::cfg::{Block, CFG};
    use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar};
    use crate::ir::Stmt;

    fn var(n: &str) -> ScalarVar {
        ScalarVar { name: n.into(), ty: ScalarTy::I64 }
    }
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
    fn empty_block(id: usize, preds: Vec<usize>, succs: Vec<usize>) -> Block {
        Block { id, preds, succs, stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] }
    }

    fn build(cfg: &CFG) -> FrudChain {
        let (annotated, chain) = build_chain_with_phi(cfg);
        construct_frud_chains(&annotated, &chain)
    }

    #[test]
    fn frud_linear() {
        // def x; use x  → the def reaches exactly that use.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                def_block(0, vec![], vec![1], 0),
                use_block(1, vec![0], vec![], 0),
            ],
            entry: 0,
            exit: 1,
        };
        let f = build(&cfg);
        assert_eq!(f.uses_of(DefId(0)), [UseId(0)].into_iter().collect());
    }

    #[test]
    fn frud_def_reaches_all_later_uses_until_redefined() {
        // def x; use x; use x; def x; use x.
        let cfg = CFG::new_single_block(
            vec![var("x")],
            vec![
                Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var: 0, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] },
                Stmt { id: 1, uses: vec![ScalarUse { var: 0, stmt: 1, order: 0 }], defs: vec![] },
                Stmt { id: 2, uses: vec![ScalarUse { var: 0, stmt: 2, order: 0 }], defs: vec![] },
                Stmt { id: 3, uses: vec![], defs: vec![ScalarDef { var: 0, stmt: 3, order: 0, rhs: ScalarRhs::Unknown }] },
                Stmt { id: 4, uses: vec![ScalarUse { var: 0, stmt: 4, order: 0 }], defs: vec![] },
            ],
        );
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let f = construct_frud_chains(&annotated, &chain);
        // defs: stmt0->DefId0, stmt3->DefId1; uses: stmt1->UseId0, stmt2->UseId1, stmt4->UseId2.
        assert_eq!(f.uses_of(DefId(0)), [UseId(0), UseId(1)].into_iter().collect());
        assert_eq!(f.uses_of(DefId(1)), [UseId(2)].into_iter().collect());
        assert!(f.reaches(DefId(0), UseId(1)));
        assert!(!f.reaches(DefId(0), UseId(2)));
    }

    #[test]
    fn frud_merge_links_both_defs() {
        // def x in both branches; use x after merge → both defs reach it.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                empty_block(0, vec![], vec![1]),
                empty_block(1, vec![0], vec![2, 3]),
                def_block(2, vec![1], vec![4], 0),
                def_block(3, vec![1], vec![4], 0),
                use_block(4, vec![2, 3], vec![], 0),
            ],
            entry: 0,
            exit: 4,
        };
        let f = build(&cfg);
        assert!(f.reaches(DefId(0), UseId(0)));
        assert!(f.reaches(DefId(1), UseId(0)));
    }

    #[test]
    fn frud_branch_links_local_def() {
        // x defined in the left branch then used there.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                empty_block(0, vec![], vec![1]),
                empty_block(1, vec![0], vec![2, 3]),
                Block {
                    id: 2,
                    preds: vec![1],
                    succs: vec![4],
                    stmts: vec![
                        Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var: 0, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] },
                        Stmt { id: 1, uses: vec![ScalarUse { var: 0, stmt: 1, order: 0 }], defs: vec![] },
                    ],
                    phi: vec![],
                    upsilon: vec![],
                    guards: vec![],
                },
                empty_block(3, vec![1], vec![4]),
                empty_block(4, vec![2, 3], vec![]),
            ],
            entry: 0,
            exit: 4,
        };
        let f = build(&cfg);
        assert!(f.reaches(DefId(0), UseId(0)));
    }
}
#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::fud::chain::build_chain_with_phi;
    use crate::fud::testgen::{gen_cfg_dag, Rng};
    use crate::ir::cfg::CFG;
    use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarUse};
    use crate::ir::Stmt;

    fn pure_blocks(cfg: &mut CFG, nv: usize, rng: &mut Rng) {
        for b in &mut cfg.blocks {
            b.phi.clear();
            b.upsilon.clear();
            b.stmts = match rng.below(3) {
                0 => vec![],
                1 => {
                    let v = rng.below(nv as u64) as usize;
                    vec![Stmt { id: 0, uses: vec![ScalarUse { var: v, stmt: 0, order: 0 }], defs: vec![] }]
                }
                _ => {
                    let v = rng.below(nv as u64) as usize;
                    vec![Stmt { id: 0, uses: vec![], defs: vec![ScalarDef { var: v, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }] }]
                }
            };
        }
    }

    fn all_simple_paths(cfg: &CFG, from: usize, to: usize) -> Vec<Vec<usize>> {
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

    #[test]
    fn fuzz_frud_vs_bruteforce() {
        // Symmetric to the reaching fuzz: def d (var m, block db) reaches use u
        // (var m, block ub) iff some simple path db → ub has no other def-of-m
        // block strictly after db. Compare against FrudChain.
        let mut rng = Rng::new(0xf4ed_c0de_0001_0001);
        let mut tested = 0u64;
        let mut mismatches = 0u64;
        let mut examples: Vec<String> = Vec::new();
        for _ in 0..1500 {
            let mut cfg = gen_cfg_dag(&mut rng, 6, 1);
            let nv = cfg.vars.len();
            pure_blocks(&mut cfg, nv, &mut rng);
            cfg.validate().expect("pure DAG valid");
            let (annotated, chain) = build_chain_with_phi(&cfg);
            let f = construct_frud_chains(&annotated, &chain);
            let ix = annotated.index();

            for (di, dsite) in ix.defs.iter().enumerate() {
                let m = dsite.var;
                let db = dsite.block;
                let mut want: BTreeSet<UseId> = BTreeSet::new();
                for (ui, usite) in ix.uses.iter().enumerate() {
                    if usite.var != m || usite.block == db {
                        continue;
                    }
                    let ub = usite.block;
                    for path in all_simple_paths(&annotated, db, ub) {
                        let clean = path[1..]
                            .iter()
                            .all(|bb| !ix.defs.iter().any(|s| s.block == *bb && s.var == m));
                        if clean {
                            want.insert(UseId(ui));
                            break;
                        }
                    }
                }
                let got = f.uses_of(DefId(di));
                if got != want {
                    mismatches += 1;
                    if examples.len() < 5 {
                        examples.push(format!(
                            "def#{} var={} block={} got={:?} want={:?}",
                            di, m, db, got, want
                        ));
                    }
                }
            }
            tested += 1;
        }
        println!(
            "fuzz_frud_vs_bruteforce: tested={} mismatches={}",
            tested, mismatches
        );
        for ex in &examples {
            println!("  MISMATCH {}", ex);
        }
        assert_eq!(mismatches, 0, "FRUD disagrees with the path oracle");
    }
}
