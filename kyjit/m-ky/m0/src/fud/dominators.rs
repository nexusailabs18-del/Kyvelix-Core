//! Dominators and dominance frontiers (Wolfe §6.1.3, Phase 3.2).
//!
//! Uses the Cooper-Harvey-Kennedy iterative algorithm for the dominator tree,
//! then the classic two-phase frontier computation:
//!
//! ```text
//! DF(X)  = { Z | ∃ Y₁, Y₂ ∈ PRED(Z), Y₁ ≠ Y₂, X DOM Y₁, ¬(X DOM Y₂) }
//! DF⁺(S) = fixpoint of DF(S ∪ DF⁺(S))      (iterated frontier)
//! ```
//!
//! The equivalence theorem `DF⁺(S) = J⁺(S)` when `Entry ∈ S` is what makes
//! `DF⁺` the correct (and minimal) basis for φ-placement; the fuzz tests in
//! this module verify both `DF` against its definition and `DF⁺` against an
//! explicit join-set oracle on small CFGs.

use std::collections::BTreeSet;

use crate::ir::CFG;

/// The dominator tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DominatorTree {
    /// `idom[b]` = immediate dominator of `b`. `None` for the entry block and
    /// for blocks unreachable from entry (the latter cannot occur on a
    /// validated CFG).
    pub idom: Vec<Option<usize>>,
    /// Reverse postorder position; `rpo_index[entry] == 0` and a dominator
    /// always has a strictly smaller index than what it dominates.
    pub rpo_index: Vec<usize>,
    /// Dominator-tree children, sorted by block id for determinism.
    pub children: Vec<Vec<usize>>,
    /// Blocks in reverse postorder (entry first).
    pub rpo: Vec<usize>,
}

impl DominatorTree {
    /// The dominator-tree parent of `b` (`None` for the root).
    pub fn parent(&self, b: usize) -> Option<usize> {
        self.idom[b]
    }

    /// All dominators of `y` (including `y`), via the idom chain.
    pub fn dom_set(&self, y: usize) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        let mut cur = Some(y);
        while let Some(b) = cur {
            if !out.insert(b) {
                break;
            }
            cur = self.idom[b];
        }
        out
    }

    /// `x` dominates `y`: every path entry → y passes through `x`.
    pub fn dominates(&self, x: usize, y: usize) -> bool {
        let mut cur = Some(y);
        while let Some(b) = cur {
            if b == x {
                return true;
            }
            cur = self.idom[b];
        }
        false
    }
}

/// Dominance frontiers. `df[x]` is `DF(x)`; `df_plus[x]` is the iterated
/// frontier `DF⁺({x})` (transitive closure of the DF relation from `x`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DominanceFrontiers {
    pub df: Vec<BTreeSet<usize>>,
    pub df_plus: Vec<BTreeSet<usize>>,
}

/// `DF⁺(S)` for a set of blocks: the union of `df_plus[x]` over `x ∈ S`.
pub fn df_plus_of_set(frontiers: &DominanceFrontiers, s: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for &x in s {
        for &z in &frontiers.df_plus[x] {
            out.insert(z);
        }
    }
    out
}

/// Compute the dominator tree with the Cooper-Harvey-Kennedy algorithm.
pub fn compute_dominators(cfg: &CFG) -> DominatorTree {
    let n = cfg.blocks.len();
    let mut rpo = reverse_postorder(cfg);
    // Defensive: if a caller hands us an unreachable block (validate() rejects
    // it), append it so indices cover every block.
    if rpo.len() < n {
        for b in 0..n {
            if !rpo.contains(&b) {
                rpo.push(b);
            }
        }
    }
    let mut rpo_index = vec![usize::MAX; n];
    for (i, &b) in rpo.iter().enumerate() {
        rpo_index[b] = i;
    }

    let mut idom: Vec<Option<usize>> = vec![None; n];
    idom[cfg.entry] = None; // root
    let mut changed = true;
    while changed {
        changed = false;
        // Skip entry (index 0); process in RPO.
        for &b in rpo.iter().skip(1) {
            let preds: Vec<usize> = cfg.blocks[b]
                .preds
                .iter()
                .copied()
                .filter(|&p| idom[p].is_some() || p == cfg.entry)
                .collect();
            let mut new_idom: Option<usize> = None;
            for p in preds {
                new_idom = match new_idom {
                    None => Some(p),
                    Some(cur) => Some(intersect(&idom, &rpo_index, p, cur)),
                };
            }
            if let Some(new_idom) = new_idom {
                if idom[b] != Some(new_idom) {
                    idom[b] = Some(new_idom);
                    changed = true;
                }
            }
        }
    }

    let mut children: Vec<Vec<usize>> = vec![vec![]; n];
    for b in 0..n {
        if let Some(p) = idom[b] {
            children[p].push(b);
        }
    }
    for c in children.iter_mut() {
        c.sort_unstable();
    }

    DominatorTree { idom, rpo_index, children, rpo }
}

/// The CHK `Intersect` walk: walk both fingers up the idom chain, always
/// moving the one with the larger RPO index, until they meet.
fn intersect(idom: &[Option<usize>], rpo_index: &[usize], b1: usize, b2: usize) -> usize {
    let mut f1 = b1;
    let mut f2 = b2;
    while f1 != f2 {
        while rpo_index[f1] > rpo_index[f2] {
            match idom[f1] {
                Some(p) => f1 = p,
                None => return f2,
            }
        }
        while rpo_index[f2] > rpo_index[f1] {
            match idom[f2] {
                Some(p) => f2 = p,
                None => return f1,
            }
        }
    }
    f1
}

/// Reverse postorder from the entry (iterative DFS).
fn reverse_postorder(cfg: &CFG) -> Vec<usize> {
    let n = cfg.blocks.len();
    let mut visited = vec![false; n];
    let mut post = Vec::with_capacity(n);
    // (block, next successor index)
    let mut stack: Vec<(usize, usize)> = vec![(cfg.entry, 0)];
    visited[cfg.entry] = true;
    while let Some(&mut (b, ref mut si)) = stack.last_mut() {
        if *si < cfg.blocks[b].succs.len() {
            let s = cfg.blocks[b].succs[*si];
            *si += 1;
            if !visited[s] {
                visited[s] = true;
                stack.push((s, 0));
            }
        } else {
            post.push(b);
            stack.pop();
        }
    }
    post.reverse();
    post
}

/// Compute `DF` and `DF⁺` for every block.
pub fn compute_dominance_frontiers(cfg: &CFG, dom: &DominatorTree) -> DominanceFrontiers {
    let n = cfg.blocks.len();
    let mut df: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for b in 0..n {
        let preds = &cfg.blocks[b].preds;
        if preds.len() >= 2 {
            let idom_b = dom.idom[b];
            for &p in preds {
                let mut runner = p;
                loop {
                    if Some(runner) == idom_b {
                        break;
                    }
                    df[runner].insert(b);
                    match dom.idom[runner] {
                        Some(next) => runner = next,
                        None => break,
                    }
                }
            }
        }
    }

    // DF⁺: transitive closure of the DF relation from each block.
    let mut df_plus = df.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for b in 0..n {
            let current: Vec<usize> = df_plus[b].iter().copied().collect();
            for y in current {
                for &z in &df[y] {
                    if df_plus[b].insert(z) {
                        changed = true;
                    }
                }
            }
        }
    }

    DominanceFrontiers { df, df_plus }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::testgen::{gen_cfg, Rng};
    use crate::ir::cfg::{Block, CFG};

    /// A hand-built diamond: 0 → 1 → {2, 3} → 4.
    fn diamond() -> CFG {
        let b = |id: usize, preds: Vec<usize>, succs: Vec<usize>| Block {
            id,
            preds,
            succs,
            stmts: vec![],
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        };
        CFG {
            vars: vec![],
            blocks: vec![
                b(0, vec![], vec![1]),
                b(1, vec![0], vec![2, 3]),
                b(2, vec![1], vec![4]),
                b(3, vec![1], vec![4]),
                b(4, vec![2, 3], vec![]),
            ],
            entry: 0,
            exit: 4,
        }
    }

    /// A hand-built loop: 0 → 1 → 2 → 1, 1 → 3(exit).
    fn loop_cfg() -> CFG {
        let b = |id: usize, preds: Vec<usize>, succs: Vec<usize>| Block {
            id,
            preds,
            succs,
            stmts: vec![],
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        };
        CFG {
            vars: vec![],
            blocks: vec![
                b(0, vec![], vec![1]),
                b(1, vec![0, 2], vec![2, 3]),
                b(2, vec![1], vec![1]),
                b(3, vec![1], vec![]),
            ],
            entry: 0,
            exit: 3,
        }
    }

    // ---- Brute-force oracles -------------------------------------------------

    /// Is `target` reachable from entry while avoiding `avoid`?
    fn reachable_avoiding(cfg: &CFG, target: usize, avoid: usize) -> bool {
        if target == avoid {
            return false;
        }
        let n = cfg.blocks.len();
        let mut seen = vec![false; n];
        let mut stack = vec![cfg.entry];
        if cfg.entry == avoid {
            return false;
        }
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

    /// `x DOM y` by the definition: every path entry → y passes through x.
    fn brute_dominates(cfg: &CFG, x: usize, y: usize) -> bool {
        x == y || !reachable_avoiding(cfg, y, x)
    }

    /// Brute-force dominator sets for every block.
    fn brute_dom_sets(cfg: &CFG) -> Vec<BTreeSet<usize>> {
        let n = cfg.blocks.len();
        (0..n)
            .map(|y| (0..n).filter(|&x| brute_dominates(cfg, x, y)).collect())
            .collect()
    }

    /// Brute-force `DF` from its definition, using brute-force dominators.
    fn brute_df(cfg: &CFG, doms: &[BTreeSet<usize>]) -> Vec<BTreeSet<usize>> {
        let n = cfg.blocks.len();
        let mut df: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
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

    fn closure_of(df: &[BTreeSet<usize>], x: usize) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        let mut stack: Vec<usize> = df[x].iter().copied().collect();
        while let Some(y) = stack.pop() {
            if out.insert(y) {
                for &z in &df[y] {
                    stack.push(z);
                }
            }
        }
        out
    }

    // ---- Unit tests ----------------------------------------------------------

    #[test]
    fn dom_entry_dominates_everything() {
        for cfg in [diamond(), loop_cfg()] {
            let dom = compute_dominators(&cfg);
            for y in 0..cfg.blocks.len() {
                assert!(dom.dominates(0, y), "entry must dominate block {}", y);
            }
        }
    }

    #[test]
    fn dom_reflexive() {
        let cfg = diamond();
        let dom = compute_dominators(&cfg);
        for y in 0..cfg.blocks.len() {
            assert!(dom.dominates(y, y));
        }
    }

    #[test]
    fn dom_idom_is_immediate() {
        let cfg = diamond();
        let dom = compute_dominators(&cfg);
        assert_eq!(dom.parent(0), None);
        assert_eq!(dom.parent(1), Some(0));
        assert_eq!(dom.parent(2), Some(1));
        assert_eq!(dom.parent(3), Some(1));
        assert_eq!(dom.parent(4), Some(1));

        let cfg = loop_cfg();
        let dom = compute_dominators(&cfg);
        assert_eq!(dom.parent(1), Some(0));
        assert_eq!(dom.parent(2), Some(1));
        assert_eq!(dom.parent(3), Some(1));
    }

    #[test]
    fn df_diamond_and_loop() {
        let cfg = diamond();
        let dom = compute_dominators(&cfg);
        let fr = compute_dominance_frontiers(&cfg, &dom);
        // Block 4's preds are 2 and 3; 4 ∈ DF(2) and 4 ∈ DF(3).
        assert!(fr.df[2].contains(&4));
        assert!(fr.df[3].contains(&4));

        let cfg = loop_cfg();
        let dom = compute_dominators(&cfg);
        let fr = compute_dominance_frontiers(&cfg, &dom);
        // Block 1 is the loop header: preds 0 and 2; 1 dominates 2 but not 0,
        // so 1 ∈ DF(1); 2's frontier is {1}.
        assert!(fr.df[1].contains(&1));
        assert!(fr.df[2].contains(&1));
    }

    #[test]
    fn df_plus_is_transitive_closure_on_hand_cfg() {
        for cfg in [diamond(), loop_cfg()] {
            let dom = compute_dominators(&cfg);
            let fr = compute_dominance_frontiers(&cfg, &dom);
            for x in 0..cfg.blocks.len() {
                let closure = closure_of(&fr.df, x);
                assert_eq!(fr.df_plus[x], closure, "DF+ mismatch for block {}", x);
            }
        }
    }

    // ---- Fuzz tests ----------------------------------------------------------

    #[test]
    fn fuzz_dominators_brute_force() {
        let mut rng = Rng::new(0xd01_4a70_5eed);
        let mut tested = 0u64;
        for _ in 0..1500 {
            let cfg = gen_cfg(&mut rng, 7, 2);
            cfg.validate().expect("generated CFG valid");
            let dom = compute_dominators(&cfg);
            let want = brute_dom_sets(&cfg);
            for y in 0..cfg.blocks.len() {
                let got = dom.dom_set(y);
                assert_eq!(got, want[y], "dom set mismatch for block {}\ncfg={:?}", y, cfg);
                for x in 0..cfg.blocks.len() {
                    assert_eq!(
                        dom.dominates(x, y),
                        want[y].contains(&x),
                        "dominates({},{}) mismatch\ncfg={:?}",
                        x,
                        y,
                        cfg
                    );
                }
            }
            tested += 1;
        }
        println!("fuzz_dominators_brute_force: tested={}", tested);
    }

    #[test]
    fn fuzz_idom_vs_dom() {
        let mut rng = Rng::new(0x1d0a_5eed_c0de_0001);
        for _ in 0..1500 {
            let cfg = gen_cfg(&mut rng, 7, 2);
            let dom = compute_dominators(&cfg);
            let want = brute_dom_sets(&cfg);
            for b in 0..cfg.blocks.len() {
                let mut s = want[b].clone();
                s.remove(&b);
                match dom.parent(b) {
                    None => {
                        // Only the entry may have no idom.
                        assert_eq!(b, cfg.entry, "non-entry block {} has no idom", b);
                    }
                    Some(idom) => {
                        // The idom is the unique dominator, other than b,
                        // that is dominated by every other dominator of b.
                        assert!(s.contains(&idom));
                        for &y in &s {
                            assert!(
                                want[idom].contains(&y),
                                "idom({})={} must be dominated by {}",
                                b,
                                idom,
                                y
                            );
                        }
                        // Uniqueness: no other element of s is dominated by all.
                        for &x in &s {
                            if x == idom {
                                continue;
                            }
                            let all = s.iter().all(|&y| want[x].contains(&y));
                            assert!(!all, "two candidates for idom({}): {} and {}", b, idom, x);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fuzz_df_brute_force() {
        let mut rng = Rng::new(0xdf_4f_4f_4f_4f_4f);
        for _ in 0..1500 {
            let cfg = gen_cfg(&mut rng, 7, 2);
            let dom = compute_dominators(&cfg);
            let fr = compute_dominance_frontiers(&cfg, &dom);
            let doms = brute_dom_sets(&cfg);
            let want = brute_df(&cfg, &doms);
            for x in 0..cfg.blocks.len() {
                assert_eq!(fr.df[x], want[x], "DF({}) mismatch\ncfg={:?}", x, cfg);
            }
        }
    }

    #[test]
    fn fuzz_df_plus_is_transitive_closure() {
        let mut rng = Rng::new(0xdf71_05_c105_e001);
        for _ in 0..1500 {
            let cfg = gen_cfg(&mut rng, 7, 2);
            let dom = compute_dominators(&cfg);
            let fr = compute_dominance_frontiers(&cfg, &dom);
            for x in 0..cfg.blocks.len() {
                let closure = closure_of(&fr.df, x);
                assert_eq!(fr.df_plus[x], closure, "DF+({}) mismatch\ncfg={:?}", x, cfg);
            }
            // df_plus_of_set agrees with the per-block union.
            let s: BTreeSet<usize> = (0..cfg.blocks.len()).collect();
            let union = df_plus_of_set(&fr, &s);
            let direct: BTreeSet<usize> = fr.df_plus.iter().flatten().copied().collect();
            assert_eq!(union, direct);
        }
    }
}


