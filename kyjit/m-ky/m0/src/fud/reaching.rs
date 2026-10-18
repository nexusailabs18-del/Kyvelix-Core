//! Finding all reaching definitions (Wolfe §6.3, Algorithm FollowChain).
//!
//! The chain gives each use one link (a def, or a φ-term). FollowChain turns
//! that into the *full set* of definitions that can reach the use: it follows
//! a φ back through all φ-incoming links, and a (non-killing) def back through
//! the def-def link, guarding cycles with a per-use marked set.
//!
//! ```text
//! FollowChain(d, u):
//!     if Marked[d] = u: return
//!     Marked[d] = u
//!     if d defines u:                add d to Defs[u]
//!     if d is a φ-term:              for each incoming j: FollowChain(φ-Chain[d][j], u)
//!     else if d is killing:          stop
//!     else:                          FollowChain(Chain[d], u)
//! ```
//!
//! For scalars every definition is *killing* (a scalar assignment overwrites
//! the whole variable), so the `else` branch is unreachable today; the slot is
//! kept because Phase 6's array chains need it. The correctness property that
//! matters here is: `Defs[u]` is exactly the set of definitions that can reach
//! `u` along some path with no intervening killing def of the same variable.

use std::collections::{BTreeSet, HashMap};

use crate::ir::scalar::{DefId, PhiId, UseId};
use crate::ir::CFG;

use super::chain::{ChainLink, FudChain};

/// The full set of definitions reaching each use.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReachingDefs {
    /// use → the (sorted, deduplicated) set of reaching definitions.
    pub defs: HashMap<UseId, BTreeSet<DefId>>,
}

impl ReachingDefs {
    /// The reaching-definition set of a use (empty if none/uninitialized).
    pub fn of(&self, u: UseId) -> BTreeSet<DefId> {
        self.defs.get(&u).cloned().unwrap_or_default()
    }
}

/// FollowChain over every use in the CFG.
pub fn find_reaching_defs(cfg: &CFG, chain: &FudChain) -> ReachingDefs {
    let ix = cfg.index();
    let mut defs: HashMap<UseId, BTreeSet<DefId>> = HashMap::new();
    for idx in 0..ix.uses.len() {
        let u = UseId(idx);
        let mut out: BTreeSet<DefId> = BTreeSet::new();
        // Per-use marked set of visited φ-terms (scalars need no def marks: a
        // def is its own terminator). Guards φ cycles from loop back edges.
        let mut marked: BTreeSet<PhiId> = BTreeSet::new();
        match chain.use_to_def.get(&u) {
            Some(ChainLink::Def(d)) => {
                out.insert(*d);
            }
            Some(ChainLink::Phi(p)) => {
                // Collect the direct defs on every incoming path. A successor's
                // φ-incoming slot may itself be a Def or a Phi; a Def that came
                // through a merge is still a genuine reaching definition of u.
                follow_phi(cfg, chain, *p, &mut out, &mut marked);
            }
            None => {}
        }
        if !out.is_empty() {
            defs.insert(u, out);
        }
    }
    ReachingDefs { defs }
}

/// `FollowChain` on a φ link: recurse over incoming links.
fn follow_phi(
    cfg: &CFG,
    chain: &FudChain,
    p: PhiId,
    out: &mut BTreeSet<DefId>,
    marked: &mut BTreeSet<PhiId>,
) {
    if !marked.insert(p) {
        return; // already visited for this use (cycle guard)
    }
    let _ = cfg;
    if let Some(slots) = chain.phi_incoming.get(&p) {
        for slot in slots {
            match slot {
                Some(ChainLink::Def(d)) => {
                    out.insert(*d);
                }
                Some(ChainLink::Phi(p2)) => {
                    follow_phi(cfg, chain, *p2, out, marked);
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::chain::build_chain_with_phi;
    use crate::fud::testgen::{gen_cfg_dag, Rng};
    use crate::ir::cfg::{Block, CFG};
    use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar};
    use crate::ir::Stmt;

    fn var(n: &str) -> ScalarVar {
        ScalarVar { name: n.into(), ty: ScalarTy::I64 }
    }

    /// Block with a single statement that is a pure use of `v`.
    fn use_block(id: usize, preds: Vec<usize>, succs: Vec<usize>, v: usize) -> Block {
        Block {
            id,
            preds,
            succs,
            stmts: vec![Stmt {
                id: 0,
                uses: vec![ScalarUse { var: v, stmt: 0, order: 0 }],
                defs: vec![],
            }],
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        }
    }

    /// Block with a single statement that is a pure def of `v`.
    fn def_block(id: usize, preds: Vec<usize>, succs: Vec<usize>, v: usize) -> Block {
        Block {
            id,
            preds,
            succs,
            stmts: vec![Stmt {
                id: 0,
                uses: vec![],
                defs: vec![ScalarDef { var: v, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }],
            }],
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        }
    }

    fn empty_block(id: usize, preds: Vec<usize>, succs: Vec<usize>) -> Block {
        Block { id, preds, succs, stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] }
    }

    #[test]
    fn reaching_single_def() {
        // 0(def x) → 1(use x): the use sees DefId0.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                def_block(0, vec![], vec![1], 0),
                use_block(1, vec![0], vec![], 0),
            ],
            entry: 0,
            exit: 1,
        };
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let rd = find_reaching_defs(&annotated, &chain);
        assert_eq!(rd.of(UseId(0)), [DefId(0)].into_iter().collect());
    }

    #[test]
    fn reaching_killed_by_overwrite() {
        // 0(def x) → 1(def x) → 2(use x): only the second def reaches.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                def_block(0, vec![], vec![1], 0),
                def_block(1, vec![0], vec![2], 0),
                use_block(2, vec![1], vec![], 0),
            ],
            entry: 0,
            exit: 2,
        };
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let rd = find_reaching_defs(&annotated, &chain);
        assert_eq!(rd.of(UseId(0)), [DefId(1)].into_iter().collect());
    }

    #[test]
    fn reaching_through_phi() {
        // def x in both branches; use after merge → both defs reach.
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
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let rd = find_reaching_defs(&annotated, &chain);
        assert_eq!(rd.of(UseId(0)), [DefId(0), DefId(1)].into_iter().collect());
    }

    #[test]
    fn reaching_branch_conditional() {
        // x defined in the left branch only; a use *inside* the left branch
        // after the def sees it; a use in the right branch is uninitialized.
        let cfg = CFG {
            vars: vec![var("x")],
            blocks: vec![
                empty_block(0, vec![], vec![1]),
                empty_block(1, vec![0], vec![2, 3]),
                // left branch: def then use
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
                use_block(3, vec![1], vec![4], 0), // right branch use
                empty_block(4, vec![2, 3], vec![]),
            ],
            entry: 0,
            exit: 4,
        };
        let (annotated, chain) = build_chain_with_phi(&cfg);
        let rd = find_reaching_defs(&annotated, &chain);
        // ix uses: block2 stmt1 -> UseId0, block3 stmt0 -> UseId1.
        assert_eq!(rd.of(UseId(0)), [DefId(0)].into_iter().collect());
        assert!(rd.of(UseId(1)).is_empty(), "right-branch use is uninitialized");
    }

    /// Rewrite a CFG so every block has at most one *pure* statement (a use of
    /// one variable or a def of one variable, never both, never two). This
    /// keeps intra-block ordering trivial so the path-based oracle is exact.
    fn pure_blocks(cfg: &mut CFG, nv: usize, rng: &mut Rng) {
        for b in &mut cfg.blocks {
            b.phi.clear();
            b.upsilon.clear();
            let roll = rng.below(3);
            b.stmts = match roll {
                0 => vec![],
                1 => {
                    let v = rng.below(nv as u64) as usize;
                    vec![Stmt {
                        id: 0,
                        uses: vec![ScalarUse { var: v, stmt: 0, order: 0 }],
                        defs: vec![],
                    }]
                }
                _ => {
                    let v = rng.below(nv as u64) as usize;
                    vec![Stmt {
                        id: 0,
                        uses: vec![],
                        defs: vec![ScalarDef { var: v, stmt: 0, order: 0, rhs: ScalarRhs::Unknown }],
                    }]
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
    fn fuzz_reaching_vs_bruteforce() {
        // Path oracle: def `d` (var m, block db) reaches use `u` (var m, block
        // ub) iff some simple path db → ub has no other def-of-m block strictly
        // after db. Compared against FollowChain's Defs[u].
        let mut rng = Rng::new(0x7ea_c0de_0001_0001);
        let mut tested = 0u64;
        let mut mismatches = 0u64;
        let mut examples: Vec<String> = Vec::new();
        for _ in 0..1500 {
            let mut cfg = gen_cfg_dag(&mut rng, 6, 1);
            let nv = cfg.vars.len();
            pure_blocks(&mut cfg, nv, &mut rng);
            cfg.validate().expect("pure-block DAG valid");
            let (annotated, chain) = build_chain_with_phi(&cfg);
            let rd = find_reaching_defs(&annotated, &chain);
            let ix = annotated.index();

            // def blocks of each variable.
            let mut defs_of: Vec<Vec<(usize, usize)>> = vec![vec![]; nv]; // (db, def_index)
            for (i, s) in ix.defs.iter().enumerate() {
                defs_of[s.var].push((s.block, i));
            }

            for (ui, usite) in ix.uses.iter().enumerate() {
                let m = usite.var;
                let ub = usite.block;
                let mut want: BTreeSet<DefId> = BTreeSet::new();
                for &(db, di) in &defs_of[m] {
                    if db == ub {
                        continue; // pure blocks: cannot def and use in one block
                    }
                    for path in all_simple_paths(&annotated, db, ub) {
                        // No other def-of-m block strictly after db.
                        let clean = path[1..].iter().all(|bb| {
                            !ix.defs.iter().any(|s| s.block == *bb && s.var == m)
                        });
                        if clean {
                            want.insert(DefId(di));
                            break;
                        }
                    }
                }
                let got = rd.of(UseId(ui));
                if got != want {
                    mismatches += 1;
                    if examples.len() < 5 {
                        examples.push(format!(
                            "use#{} var={} block={} got={:?} want={:?}",
                            ui, m, ub, got, want
                        ));
                    }
                }
            }
            tested += 1;
        }
        println!(
            "fuzz_reaching_vs_bruteforce: tested={} mismatches={}",
            tested, mismatches
        );
        for ex in &examples {
            println!("  MISMATCH {}", ex);
        }
        assert_eq!(mismatches, 0, "FollowChain disagrees with the path oracle");
    }
}
