//! Phase 7 §7.4 — the fixpoint over the control-flow graph (Wolfe §8.2.1).
//!
//! > "Merge at CFG merge points: change definite to possible on conflicts.
//! >  Loops: iterate until fixpoint."
//!
//! # The program model
//!
//! The array IR ([`crate::ir::Access`]) has no first-class pointer: an access is
//! a *base name plus an affine index*, and indirection is the `Indirect` field of
//! an access, not a variable with a value. So Phase 7 needs its own small
//! program model — [`PtrProgram`] — that a frontend lowers pointers into, and
//! that a certificate can carry as ground truth. This is the same deviation
//! Phase 6 documents: the analysis runs over the IR that actually models the
//! thing being analysed, not over a CFG that models something else.
//!
//! A [`PtrProgram`] is a set of declared variables ([`PtrDecl`]) and a set of
//! blocks ([`PtrBlock`]), each with predecessors and a list of
//! [`Effect`]s. It is deliberately tiny: everything Phase 7 reasons about is
//! expressible as those two things.
//!
//! # Termination
//!
//! Every transfer function in [`crate::points::transfer`] is monotone in one
//! direction — definiteness only ever falls, target sets only ever grow — and
//! both lattices are finite (bounded by `vars²` relations), so the iteration
//! reaches a fixpoint. [`MAX_ITERS`] is a guard so a malformed program is a
//! `None` rather than a hang, not part of the termination argument.

use serde::{Deserialize, Serialize};

use crate::points::relation::{Definiteness, PointsToGraph, VarId};
use crate::points::transfer::{apply_effect, Effect};

/// A guard on fixpoint iterations, so a malformed transfer function is an error
/// rather than a hang. The real termination argument is monotonicity over a
/// finite lattice; this bound is never reached by a well-formed program.
pub const MAX_ITERS: usize = 10_000;

/// One declared variable of a [`PtrProgram`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtrDecl {
    pub name: String,
    /// True when this variable holds a pointer and so starts at `unknown`.
    /// Targets (the objects pointers point at) are `false`.
    pub is_pointer: bool,
}

impl PtrDecl {
    pub fn pointer(name: impl Into<String>) -> PtrDecl {
        PtrDecl { name: name.into(), is_pointer: true }
    }

    pub fn target(name: impl Into<String>) -> PtrDecl {
        PtrDecl { name: name.into(), is_pointer: false }
    }
}

/// One basic block: predecessors and the effects that run in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtrBlock {
    pub id: usize,
    pub preds: Vec<usize>,
    pub stmts: Vec<Effect>,
}

impl PtrBlock {
    pub fn new(id: usize, preds: Vec<usize>, stmts: Vec<Effect>) -> PtrBlock {
        PtrBlock { id, preds, stmts }
    }
}

/// A tiny pointer program: declarations plus blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtrProgram {
    pub decls: Vec<PtrDecl>,
    /// Index (into `blocks`) of the entry block.
    pub entry: usize,
    pub blocks: Vec<PtrBlock>,
}

impl PtrProgram {
    pub fn new(decls: Vec<PtrDecl>, entry: usize, blocks: Vec<PtrBlock>) -> PtrProgram {
        PtrProgram { decls, entry, blocks }
    }

    /// Resolve a declared name to its [`VarId`].
    pub fn var(&self, name: &str) -> Option<VarId> {
        self.decls
            .iter()
            .position(|d| d.name == name)
            .map(|i| VarId(i as u32))
    }

    /// Resolve a name, panicking on an undeclared name. For tests and builders,
    /// where a typo should be loud.
    pub fn expect(&self, name: &str) -> VarId {
        self.var(name)
            .unwrap_or_else(|| panic!("undeclared pointer variable {:?}", name))
    }

    pub fn decl(&self, id: VarId) -> Option<&PtrDecl> {
        self.decls.get(id.index())
    }

    /// Structural checks: a non-empty entry in range, block ids matching their
    /// positions, predecessors in range.
    pub fn validate(&self) -> Result<(), String> {
        if self.blocks.is_empty() {
            return Err("pointer program has no blocks".into());
        }
        if self.entry >= self.blocks.len() {
            return Err(format!("entry {} out of range", self.entry));
        }
        for (i, b) in self.blocks.iter().enumerate() {
            if b.id != i {
                return Err(format!("block at {} has id {}", i, b.id));
            }
            for p in &b.preds {
                if *p >= self.blocks.len() {
                    return Err(format!("block {} has predecessor {} out of range", i, p));
                }
            }
        }
        Ok(())
    }
}

/// The fixpoint result: a points-to graph at each block's entry and exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    /// State on entry to each block.
    pub in_graphs: Vec<PointsToGraph>,
    /// State on exit from each block.
    pub out_graphs: Vec<PointsToGraph>,
    /// Iterations the fixpoint ran.
    pub iters: usize,
}

impl Analysis {
    /// The graph on entry to `block`.
    pub fn in_of(&self, block: usize) -> Option<&PointsToGraph> {
        self.in_graphs.get(block)
    }

    /// The graph on exit from `block` — the state a query "after this block"
    /// should read.
    pub fn out_of(&self, block: usize) -> Option<&PointsToGraph> {
        self.out_graphs.get(block)
    }

    /// The **union** of every block's exit graph: the may-safe
    /// over-approximation with no program point attached (`may_point_to` and
    /// `may_alias` stay sound here; `must_point_to` does not).
    pub fn union(&self) -> PointsToGraph {
        let mut g = PointsToGraph::empty();
        for og in &self.out_graphs {
            g.merge_from(og);
        }
        g
    }

    /// The **meet** of every block's exit graph: the must-safe approximation
    /// with no program point attached (`must_point_to` stays sound here;
    /// `may_point_to` does not). Prefer [`Analysis::out_of`] with a real block
    /// unless a program-point-free fact is genuinely what is wanted.
    pub fn common(&self) -> PointsToGraph {
        let mut it = self.out_graphs.iter();
        match it.next() {
            None => PointsToGraph::empty(),
            Some(first) => {
                let mut g = first.clone();
                for og in it {
                    g.meet_with(og);
                }
                g
            }
        }
    }
}

/// The state every pointer starts in: pointing at `unknown`, `Possible`.
///
/// Only declared pointers are seeded. A target (`int x`) is not a pointer, so
/// seeding it would invent a relation the program never had.
fn initial_graph(prog: &PtrProgram) -> PointsToGraph {
    let mut g = PointsToGraph::empty();
    for (i, d) in prog.decls.iter().enumerate() {
        if d.is_pointer {
            g.add_relation(VarId(i as u32), g.unknown, Definiteness::Possible);
        }
    }
    g
}

/// Run the points-to fixpoint over `prog`.
///
/// Returns `None` when the program is structurally invalid or fails to converge
/// within [`MAX_ITERS`] (which a monotone well-formed program cannot do — see
/// the module docs).
pub fn analyze(prog: &PtrProgram) -> Option<Analysis> {
    if prog.validate().is_err() {
        return None;
    }
    let initial = initial_graph(prog);
    let n = prog.blocks.len();
    let mut in_g: Vec<PointsToGraph> = vec![PointsToGraph::empty(); n];
    let mut out_g: Vec<PointsToGraph> = vec![PointsToGraph::empty(); n];
    let mut iters = 0usize;

    loop {
        iters += 1;
        if iters > MAX_ITERS {
            return None;
        }
        let mut changed = false;
        for (i, b) in prog.blocks.iter().enumerate() {
            // Meet predecessors' exit graphs; a block with no predecessors — or
            // the entry block, whose entry edge comes from *outside* the program
            // — is also seeded by the initial state. The meet shrinks: a target
            // only survives if every source had it.
            let mut sources: Vec<&PointsToGraph> = Vec::with_capacity(b.preds.len() + 1);
            if i == prog.entry || b.preds.is_empty() {
                sources.push(&initial);
            }
            for p in &b.preds {
                if let Some(pg) = out_g.get(*p) {
                    sources.push(pg);
                }
            }
            let mut new_in = match sources.first() {
                Some(g) => (*g).clone(),
                None => initial.clone(),
            };
            for g in &sources[1..] {
                new_in.meet_with(g);
            }
            if new_in != in_g[i] {
                in_g[i] = new_in;
                changed = true;
            }
            let mut g = in_g[i].clone();
            for stmt in &b.stmts {
                apply_effect(&mut g, stmt);
            }
            if g != out_g[i] {
                out_g[i] = g;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    Some(Analysis {
        in_graphs: in_g,
        out_graphs: out_g,
        iters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::points::transfer::Effect;

    /// A program whose declarations are `names`, with `is_pointer` all true
    /// except names starting with `t` (targets).
    fn prog(decls: &[(&str, bool)], blocks: Vec<PtrBlock>) -> PtrProgram {
        let d = decls
            .iter()
            .map(|(n, p)| {
                if *p {
                    PtrDecl::pointer(*n)
                } else {
                    PtrDecl::target(*n)
                }
            })
            .collect();
        PtrProgram::new(d, 0, blocks)
    }

    #[test]
    fn straight_line_address_of_is_must() {
        let p = prog(
            &[("tx", false), ("p", true)],
            vec![PtrBlock::new(
                0,
                vec![],
                vec![Effect::AddressOf {
                    lhs: VarId(1),
                    target: VarId(0),
                }],
            )],
        );
        let a = analyze(&p).unwrap();
        let g = a.out_of(0).unwrap();
        assert!(g.must_point_to(VarId(1), VarId(0)));
    }

    #[test]
    fn branch_join_weakens_must() {
        let p = prog(
            &[("tx", false), ("ty", false), ("p", true)],
            vec![
                PtrBlock::new(
                    0,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(2), target: VarId(0) }],
                ),
                PtrBlock::new(
                    1,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(2), target: VarId(1) }],
                ),
                PtrBlock::new(2, vec![0, 1], vec![]),
            ],
        );
        let a = analyze(&p).unwrap();
        let g = a.out_of(2).unwrap();
        assert!(g.may_point_to(VarId(2), VarId(0)));
        assert!(g.may_point_to(VarId(2), VarId(1)));
        assert!(!g.must_point_to(VarId(2), VarId(0)));
    }

    #[test]
    fn a_kill_path_prevents_a_forged_must() {
        let p = prog(
            &[("tx", false), ("p", true)],
            vec![
                PtrBlock::new(
                    0,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(1), target: VarId(0) }],
                ),
                PtrBlock::new(1, vec![], vec![Effect::Kill { lhs: VarId(1) }]),
                PtrBlock::new(2, vec![0, 1], vec![]),
            ],
        );
        let a = analyze(&p).unwrap();
        let g = a.out_of(2).unwrap();
        assert!(g.may_point_to(VarId(1), VarId(0)));
        assert!(!g.must_point_to(VarId(1), VarId(0)));
    }

    #[test]
    fn loop_head_reaches_a_fixpoint_and_stays_sound() {
        let p = prog(
            &[("tx", false), ("p", true)],
            vec![
                PtrBlock::new(
                    0,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(1), target: VarId(0) }],
                ),
                PtrBlock::new(1, vec![0, 1], vec![]),
            ],
        );
        let a = analyze(&p).unwrap();
        // The back edge only ever weakens, so `may` must hold and `must` may not.
        let g = a.out_of(1).unwrap();
        assert!(g.may_point_to(VarId(1), VarId(0)));
        assert!(a.iters >= 2, "a cyclic program iterates at least twice");
    }

    #[test]
    fn invalid_program_is_rejected() {
        assert!(analyze(&PtrProgram::new(vec![], 0, vec![])).is_none());
        let bad = PtrProgram::new(vec![PtrDecl::pointer("p")], 5, vec![PtrBlock::new(0, vec![], vec![])]);
        assert!(analyze(&bad).is_none());
    }

    #[test]
    fn analysis_is_deterministic() {
        let p = prog(
            &[("tx", false), ("ty", false), ("p", true)],
            vec![
                PtrBlock::new(
                    0,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(2), target: VarId(0) }],
                ),
                PtrBlock::new(
                    1,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(2), target: VarId(1) }],
                ),
                PtrBlock::new(2, vec![0, 1], vec![]),
            ],
        );
        let a = analyze(&p).unwrap();
        let b = analyze(&p).unwrap();
        assert_eq!(a.out_graphs, b.out_graphs);
    }

    #[test]
    fn union_is_may_safe_and_common_is_must_safe() {
        let p = prog(
            &[("tx", false), ("ty", false), ("p", true)],
            vec![
                PtrBlock::new(
                    0,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(2), target: VarId(0) }],
                ),
                PtrBlock::new(
                    1,
                    vec![],
                    vec![Effect::AddressOf { lhs: VarId(2), target: VarId(1) }],
                ),
                PtrBlock::new(2, vec![0, 1], vec![]),
            ],
        );
        let a = analyze(&p).unwrap();
        let u = a.union();
        assert!(u.may_point_to(VarId(2), VarId(0)) && u.may_point_to(VarId(2), VarId(1)));
        // No target is definite on every path, so `common` reports no `must`.
        let c = a.common();
        assert!(!c.must_point_to(VarId(2), VarId(0)));
        assert!(!c.must_point_to(VarId(2), VarId(1)));
    }
}