//! Shared random-CFG generator for Phase 3 fuzz tests (test-only).
//!
//! Every generated CFG satisfies `validate()` by construction: symmetric
//! edges, all blocks reachable from entry and co-reachable to exit,
//! canonical statement/use/def positions.

use crate::ir::cfg::{Block, Stmt, CFG};
use crate::ir::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar};

/// Small xorshift64 PRNG (deterministic, no dependencies).
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// True with probability `num/den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
}

/// Random statements with canonical positions. `nv` scalars, `ns` statements.
pub fn gen_statements(rng: &mut Rng, nv: usize, ns: usize) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(ns);
    for s in 0..ns {
        let nu = rng.below(3) as usize;
        let nd = rng.below(2) as usize;
        let uses = (0..nu)
            .map(|o| ScalarUse { var: rng.below(nv as u64) as usize, stmt: s, order: o })
            .collect();
        let defs = (0..nd)
            .map(|o| ScalarDef { var: rng.below(nv as u64) as usize, stmt: s, order: o, rhs: ScalarRhs::Unknown })
            .collect();
        out.push(Stmt { id: s, uses, defs });
    }
    out
}

fn gen_vars(rng: &mut Rng, max_vars: usize) -> Vec<ScalarVar> {
    let nv = 1 + rng.below(max_vars as u64) as usize;
    (0..nv).map(|i| ScalarVar { name: format!("v{}", i), ty: ScalarTy::I64 }).collect()
}

/// Random CFG with `n` blocks total (entry 0, exit n-1). `max_stmts` bounds
/// statements per block. `allow_back_edges` permits cycles.
pub fn gen_cfg_with(rng: &mut Rng, max_blocks: usize, max_stmts: usize, allow_back_edges: bool) -> CFG {
    let n = 3 + rng.below((max_blocks - 3 + 1) as u64) as usize; // 3..=max_blocks
    let mut succs: Vec<Vec<usize>> = vec![vec![]; n];
    let mut preds: Vec<Vec<usize>> = vec![vec![]; n];

    // Every non-entry block gets at least one predecessor from a lower-index
    // block (guarantees reachability from entry).
    for i in 1..n {
        let p = rng.below(i as u64) as usize;
        succs[p].push(i);
        preds[i].push(p);
    }
    // Every non-exit block gets at least one successor with a higher index
    // (guarantees co-reachability to exit).
    for i in 0..n - 1 {
        let s = i + 1 + rng.below((n - 1 - i) as u64) as usize; // (i, n-1]... in [i+1, n-1]
        if !succs[i].contains(&s) {
            succs[i].push(s);
            preds[s].push(i);
        }
    }
    // Extra forward edges.
    for i in 0..n {
        for j in i + 1..n {
            if rng.chance(1, 4) && !succs[i].contains(&j) {
                succs[i].push(j);
                preds[j].push(i);
            }
        }
    }
    // Optional back edges (loops). Sources are interior blocks only (entry
    // stays predecessor-free, exit stays successor-free); targets are never
    // the entry. A self-loop is a legitimate (if degenerate) loop shape.
    if allow_back_edges {
        for i in 1..n - 1 {
            if rng.chance(1, 6) {
                let j = 1 + rng.below(i as u64) as usize; // in [1, i]
                if !succs[i].contains(&j) {
                    succs[i].push(j);
                    preds[j].push(i);
                }
            }
        }
    }
    for v in succs.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    for v in preds.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }

    let vars = gen_vars(rng, 3);
    let nv = vars.len();
    let mut blocks = Vec::with_capacity(n);
    for b in 0..n {
        let ns = rng.below((max_stmts + 1) as u64) as usize;
        blocks.push(Block {
            id: b,
            preds: preds[b].clone(),
            succs: succs[b].clone(),
            stmts: gen_statements(rng, nv, ns),
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        });
    }
    CFG { vars, blocks, entry: 0, exit: n - 1 }
}

/// Random CFG that may contain loops.
pub fn gen_cfg(rng: &mut Rng, max_blocks: usize, max_stmts: usize) -> CFG {
    gen_cfg_with(rng, max_blocks, max_stmts, true)
}

/// Random acyclic CFG (for oracles that enumerate simple paths).
pub fn gen_cfg_dag(rng: &mut Rng, max_blocks: usize, max_stmts: usize) -> CFG {
    gen_cfg_with(rng, max_blocks, max_stmts, false)
}
