//! Phase 6 §6.1.1 — the `MayUse` / `MayMod` / `Kill` dataflow (Wolfe §8.1.1).
//!
//! The three equations are Wolfe's, verbatim:
//!
//! ```text
//! MayUse(x) = MayUse(x) U  (U_{p in PRED(x)} MayUse(p))
//! MayMod(x) = MayMod(x) U  (U_{p in PRED(x)} MayMod(p))
//! Kill(x)   = Kill(x)   U  (I_{p in PRED(x)} Kill(p))
//! ```
//!
//! with each `x` seeded by the block's own accesses. The semantics the spec
//! attaches to them are what the implementation honours:
//!
//! - `MayUse(x)` — regions **some** path to `x` reads;
//! - `MayMod(x)` — regions **some** path to `x` writes;
//! - `Kill(x)` — regions **every** path to `x` has overwritten, so any earlier
//!   write to them is dead.
//!
//! ## The intersection, and why the entry is not its own kill set
//!
//! `Kill`'s meet is an intersection, and `I {}` over the empty predecessor set
//! is the top element — "every path overwrites everything", which for an entry
//! block would be a lie that silently kills every prior write in the program.
//! That is precisely the kind of vacuous-true that makes a dataflow look
//! conclusive while proving nothing, so a block with no predecessors seeds `Kill`
//! from its own writes only. The [`seed_revert_kill_intersection`] test
//! demonstrates the difference.
//!
//! ## Termination
//!
//! The spec's argument is that the sets are finite, bounded by the number of
//! array accesses. That is right but not sufficient on its own: a meet that
//! *shrinks* a set can oscillate with a join that grows it. Here every transfer
//! function is monotone in the same increasing direction (`MayUse`/`MayMod` union
//! in, `Kill` only gains regions as its predecessors' `Kill` grow), and the
//! lattice has a finite height, so the worklist converges regardless of visit
//! order. The iteration limit in [`dataflow`] is a tripwire against a
//! non-monotone change, not the thing that makes the algorithm correct; it
//! panics loudly instead of returning a silently truncated fixed point.
//!
//! ## What a "region" is here, and the scalar-CFG limitation
//!
//! The [`RegionProgram`] is the flow graph *with* the array operations attached:
//! blocks, edges, and per-block `Access`es. [`RegionProgram::from_loop`] builds
//! it from a `Loop` in the Phase 9a five-block shape, so the array side of the
//! IR — which is where regions live — is what the analysis reads.
//!
//! This is a deliberate deviation from the spec's `dataflow(cfg: &CFG)`.
//! [`crate::ir::cfg::CFG`] models scalars: an array access lowers to a statement
//! with no uses and no defs ([`crate::ir::stmt::Stmt::lower`]), so a `CFG` alone
//! cannot say which array a block touches. Taking `&CFG` would have produced an
//! analysis that always returns empty sets and looks like a working kill
//! analysis. See the module docs of [`crate::region`] for the same limitation
//! stated for the scalar case.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::ir::{Access, AccessKind, Loop};

use super::bounds::{region_of_access, RegionSet};

/// A flow graph whose blocks carry array accesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionProgram {
    pub blocks: Vec<RegionBlock>,
    pub entry: usize,
    pub exit: usize,
}

/// One block of a [`RegionProgram`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionBlock {
    pub id: usize,
    pub preds: Vec<usize>,
    pub succs: Vec<usize>,
    /// The accesses this block performs, in program order.
    pub ops: Vec<Access>,
}

impl RegionBlock {
    /// The regions this block reads.
    pub fn gen_use(&self, loop_: &Loop) -> RegionSet {
        let mut s = RegionSet::new();
        for a in &self.ops {
            if a.kind == AccessKind::Read {
                if let Some(r) = region_of_access(a, loop_) {
                    s.insert(r);
                }
            }
        }
        s
    }

    /// The regions this block writes.
    pub fn gen_mod(&self, loop_: &Loop) -> RegionSet {
        let mut s = RegionSet::new();
        for a in &self.ops {
            if a.kind == AccessKind::Write {
                if let Some(r) = region_of_access(a, loop_) {
                    s.insert(r);
                }
            }
        }
        s
    }
}

impl RegionProgram {
    /// The Phase 9a five-block loop shape, with the loop's `body` accesses in
    /// the body block (block 2). Entry, header, latch and exit perform no array
    /// operation — they are the scalar guard and IV statements.
    ///
    /// `None` when the loop has no IVs: without a guard and a latch there is no
    /// loop to build, and a single-block program would silently report `Kill` for
    /// a body the program never said was a loop.
    pub fn from_loop(loop_: &Loop) -> Option<RegionProgram> {
        if loop_.ivs.is_empty() {
            return None;
        }
        let mk = |id: usize, preds: Vec<usize>, succs: Vec<usize>, ops: Vec<Access>| RegionBlock {
            id,
            preds,
            succs,
            ops,
        };
        Some(RegionProgram {
            blocks: vec![
                mk(0, vec![], vec![1], vec![]),
                mk(1, vec![0, 3], vec![2, 4], vec![]),
                mk(2, vec![1], vec![3], loop_.body.clone()),
                mk(3, vec![2], vec![1], vec![]),
                mk(4, vec![1], vec![], vec![]),
            ],
            entry: 0,
            exit: 4,
        })
    }

    /// A straight-line program: one block per access group, chained.
    pub fn linear(blocks: Vec<Vec<Access>>) -> RegionProgram {
        let n = blocks.len();
        let out = RegionProgram {
            blocks: blocks
                .into_iter()
                .enumerate()
                .map(|(i, ops)| RegionBlock {
                    id: i,
                    preds: if i == 0 { vec![] } else { vec![i - 1] },
                    succs: if i + 1 < n { vec![i + 1] } else { vec![] },
                    ops,
                })
                .collect(),
            entry: 0,
            exit: n.saturating_sub(1),
        };
        out
    }

    /// Structural validation: ids canonical, edges symmetric, entry has no
    /// preds, exit has no succs, every block reachable and co-reachable.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.blocks.len();
        if n == 0 {
            return Err("program has no blocks".into());
        }
        for (i, b) in self.blocks.iter().enumerate() {
            if b.id != i {
                return Err(format!("block {} has id {}", i, b.id));
            }
            for &p in &b.preds {
                if p >= n {
                    return Err(format!("block {} pred {} out of range", i, p));
                }
                if !self.blocks[p].succs.contains(&i) {
                    return Err(format!("edge {} -> {} missing reverse", p, i));
                }
            }
            for &s in &b.succs {
                if s >= n {
                    return Err(format!("block {} succ {} out of range", i, s));
                }
                if !self.blocks[s].preds.contains(&i) {
                    return Err(format!("edge {} -> {} missing reverse", i, s));
                }
            }
        }
        if self.entry >= n || self.exit >= n {
            return Err("entry/exit out of range".into());
        }
        if !self.blocks[self.entry].preds.is_empty() {
            return Err("entry has predecessors".into());
        }
        if !self.blocks[self.exit].succs.is_empty() {
            return Err("exit has successors".into());
        }
        let reach = self.reachable_from(self.entry);
        for (i, _) in self.blocks.iter().enumerate() {
            if !reach.contains(&i) {
                return Err(format!("block {} unreachable from entry", i));
            }
        }
        Ok(())
    }

    fn reachable_from(&self, start: usize) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        while let Some(b) = stack.pop() {
            if seen.insert(b) {
                stack.extend(self.blocks[b].succs.iter().copied());
            }
        }
        seen
    }
}

/// The three per-block region sets. Field names and shapes are the spec's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Dataflow {
    pub may_use: HashMap<usize, RegionSet>,
    pub may_mod: HashMap<usize, RegionSet>,
    pub kill: HashMap<usize, RegionSet>,
}

/// The whole-program summary: the triple at the exit block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Summary {
    pub may_use: Vec<super::bounds::Region>,
    pub may_mod: Vec<super::bounds::Region>,
    pub kill: Vec<super::bounds::Region>,
}

/// Run the §8.1.1 dataflow to a fixed point over `prog`.
///
/// Panics if the worklist fails to settle within `blocks * 64 + 64` rounds —
/// impossible for a monotone framework, so the panic is a loud failure mode for
/// a future non-monotone edit rather than a normal path.
pub fn dataflow(prog: &RegionProgram, loop_: &Loop) -> Dataflow {
    let n = prog.blocks.len();
    let mut df = Dataflow::default();
    for b in &prog.blocks {
        df.may_use.insert(b.id, b.gen_use(loop_));
        df.may_mod.insert(b.id, b.gen_mod(loop_));
        df.kill.insert(b.id, b.gen_mod(loop_));
    }

    let mut work: Vec<usize> = (0..n).collect();
    let mut in_queue: HashSet<usize> = work.iter().copied().collect();
    let mut rounds = 0usize;
    let budget = n.saturating_mul(64) + 64;

    while let Some(x) = work.pop() {
        in_queue.remove(&x);
        rounds += 1;
        if rounds > budget {
            panic!("region dataflow did not converge in {} rounds", budget);
        }
        let preds = prog.blocks[x].preds.clone();

        // MayUse / MayMod: union over predecessors.
        let mut mu = df.may_use[&x].clone();
        let mut mm = df.may_mod[&x].clone();
        for &p in &preds {
            mu = mu.join(&df.may_use[&p]);
            mm = mm.join(&df.may_mod[&p]);
        }

        // Kill: intersection over predecessors, then union with the block's own
        // writes. A block with no predecessors contributes the empty intersection
        // (see the module docs) — its own writes only.
        let mut k = df.kill[&x].clone();
        if !preds.is_empty() {
            let mut it = preds.iter().map(|&p| df.kill[&p].clone());
            let mut acc = it.next().unwrap_or_default();
            for other in it {
                acc = intersect_sets(&acc, &other);
            }
            k = k.join(&acc);
        }

        let changed = mu != df.may_use[&x] || mm != df.may_mod[&x] || k != df.kill[&x];
        df.may_use.insert(x, mu);
        df.may_mod.insert(x, mm);
        df.kill.insert(x, k);

        if changed {
            for &s in &prog.blocks[x].succs {
                if in_queue.insert(s) {
                    work.push(s);
                }
            }
        }
    }
    df
}

/// Region-wise intersection: two sets meet by intersecting same-arity region
/// pairs. Regions that cannot meet are dropped from the result, which makes the
/// intersection narrower than either operand — the direction `Kill` requires.
fn intersect_sets(a: &RegionSet, b: &RegionSet) -> RegionSet {
    let mut out = RegionSet::new();
    for x in &a.regions {
        for y in &b.regions {
            if let Some(i) = x.intersect(y) {
                out.insert(i);
            }
        }
    }
    out
}

impl Dataflow {
    /// The summary at the exit block: what the whole program may read, may
    /// overwrite, and certainly overwrites on every path.
    pub fn summary(&self, exit: usize) -> Summary {
        Summary {
            may_use: self.may_use.get(&exit).map(|s| s.regions.clone()).unwrap_or_default(),
            may_mod: self.may_mod.get(&exit).map(|s| s.regions.clone()).unwrap_or_default(),
            kill: self.kill.get(&exit).map(|s| s.regions.clone()).unwrap_or_default(),
        }
    }
}

/// The whole-array summary of a `Loop`: the regions its body reads and writes,
/// computed directly from the accesses.
///
/// This bypasses the CFG because, as [`crate::region`]'s module docs explain, the
/// CFG does not model array accesses. It is the array-side triple Phase 7 and
/// Phase 8 need, and it is honest about coming from the loop rather than from a
/// dataflow that never saw an array.
pub fn loop_summary(loop_: &Loop) -> Summary {
    let mut may_use = RegionSet::new();
    let mut may_mod = RegionSet::new();
    for a in &loop_.body {
        let Some(r) = region_of_access(a, loop_) else { continue };
        match a.kind {
            AccessKind::Read => may_use.insert(r),
            AccessKind::Write => {
                may_use.insert(r.clone());
                may_mod.insert(r);
            }
        }
    }
    // A write region is killed only if every path overwrites it. Inside one loop
    // body every statement executes on every trip the body runs, so the kill set
    // is the write set; a write the guard can skip contributes nothing (its
    // region is `None` from `region_of_access`, so it never gets here).
    Summary {
        may_use: may_use.regions,
        may_mod: may_mod.regions.clone(),
        kill: may_mod.regions,
    }
}
