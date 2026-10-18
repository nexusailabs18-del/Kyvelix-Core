//! Phase 8 §8.7 — bottom-up procedure summaries (Wolfe §8.5).
//!
//! A **summary** is everything the rest of the compiler wants to know about a
//! procedure without looking inside it: its purity, the symbols it may read and
//! write (its own and, transitively, its callees'), its I/O channels, the set of
//! procedures it can reach, and a schedule depth.
//!
//! Computation is bottom-up — a callee before its callers — but because
//! recursion means a procedure can be its own (indirect) callee, the read/write
//! sets are solved as a fixpoint over the call graph rather than by a single
//! topological pass. Each propagation only ever grows a set, so the fixpoint
//! terminates.
//!
//! The purity and I/O components are Phase 8's own [`crate::ipa::pure`] and
//! [`crate::ipa::io_effects`] results, so there is exactly one definition of
//! each.

use std::collections::{BTreeMap, BTreeSet};

use crate::ipa::callgraph::CallGraph;
use crate::ipa::compose::compose_into;
use crate::ipa::constprop::AbstractValue;
use crate::ipa::io_effects::{self, IoSummary};
use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, ProcId, Slot};
use crate::ipa::pure;
use crate::ir::purity::Purity;
use crate::points::transfer::Effect as PointsToEffect;
use crate::region::dataflow::Summary as RegionSummary;

/// Everything known about a procedure after the bottom-up pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSummary {
    pub proc: ProcId,
    pub purity: Purity,
    /// Symbols this procedure (or anything it calls) may read.
    pub read_set: BTreeSet<Slot>,
    /// Symbols this procedure (or anything it calls) may write.
    pub write_set: BTreeSet<Slot>,
    /// Its transitive I/O channels.
    pub io: IoSummary,
    /// Procedures it can reach, transitively (a recursive procedure reaches itself).
    pub callees: BTreeSet<ProcId>,
    /// Longest call chain below it (0 for a leaf).
    pub depth: usize,
    /// Phase 6: array region summaries per array symbol (base address).
    /// Maps array base address slot to its MayUse/MayMod/Kill regions.
    pub regions: BTreeMap<Slot, RegionSummary>,
    /// Phase 7: points-to effects (transfers) for this procedure.
    /// Captures the pointer operations that affect aliasing.
    pub pts_effects: Vec<PointsToEffect>,
    /// Phase 8: constant propagation lattice per call site.
    /// Maps (callee ProcId, callsite index) to the abstract value at that site.
    pub const_lattice: BTreeMap<(ProcId, usize), AbstractValue>,
}

/// Every slot an expression uses, including `&x` (taking an address is a use).
fn expr_slots(e: &IpaExpr, out: &mut BTreeSet<Slot>) {
    match e {
        IpaExpr::Read(s) | IpaExpr::AddrOf(s) => {
            out.insert(*s);
        }
        IpaExpr::Add(a, b) | IpaExpr::Sub(a, b) | IpaExpr::Mul(a, b) => {
            expr_slots(a, out);
            expr_slots(b, out);
        }
        _ => {}
    }
}

/// The slots a procedure reads directly in its own body.
pub fn local_reads(prog: &IpaProgram, p: ProcId) -> BTreeSet<Slot> {
    let mut out = BTreeSet::new();
    let proc = match prog.proc(p) {
        Some(pr) => pr,
        None => return out,
    };
    for stmt in &proc.stmts {
        match stmt {
            IpaStmt::Assign { value, .. } => expr_slots(value, &mut out),
            IpaStmt::Call(c) => {
                for a in &c.args {
                    expr_slots(a, &mut out);
                }
            }
            IpaStmt::Io { .. } => {}
        }
    }
    if let Some(r) = &proc.ret {
        expr_slots(r, &mut out);
    }
    out
}

/// Compute every procedure's summary, bottom-up with a fixpoint for recursion.
pub fn compute(prog: &IpaProgram, cg: &CallGraph) -> BTreeMap<ProcId, ProcSummary> {
    let ids: Vec<ProcId> = prog.ids().collect();
    let purity = pure::classify(prog, cg);
    let io = io_effects::summaries(prog, cg);

    let mut reads: Vec<BTreeSet<Slot>> = ids.iter().map(|p| local_reads(prog, *p)).collect();
    let mut writes: Vec<BTreeSet<Slot>> = ids
        .iter()
        .map(|p| crate::ipa::mod_analysis::local_writes(prog, *p))
        .collect();
    let mut callees: Vec<BTreeSet<ProcId>> = ids.iter().map(|p| cg.callees(*p)).collect();

    let n = ids.len();
    let pos = |p: ProcId| ids.iter().position(|x| *x == p);
    let mut changed = true;
    let mut guard = 0;
    while changed && guard <= n + 2 {
        guard += 1;
        changed = false;
        for (i, p) in ids.iter().enumerate() {
            let proc = match prog.proc(*p) {
                Some(x) => x,
                None => continue,
            };
            // Reachable callees, transitively.
            let mut reachable = BTreeSet::new();
            for c in &callees[i] {
                reachable.insert(*c);
                if let Some(j) = pos(*c) {
                    reachable.extend(callees[j].iter().copied());
                }
            }
            if !reachable.is_subset(&callees[i]) {
                callees[i].extend(reachable);
                changed = true;
            }
            // Read/write sets, folded through each call.
            for stmt in &proc.stmts {
                let call = match stmt {
                    IpaStmt::Call(c) => c,
                    _ => continue,
                };
                for callee in callees_of(&call.callee, *p, cg) {
                    let j = match pos(callee) {
                        Some(j) => j,
                        None => continue,
                    };
                    let cr = reads[j].clone();
                    let cw = writes[j].clone();
                    if compose_into(prog, call, &cr, &cw, &mut reads[i], &mut writes[i]) {
                        changed = true;
                    }
                }
            }
        }
    }

    // Compute constant propagation lattice (formal values per procedure).
    let const_lattice = crate::ipa::constprop::analyze(prog, cg, &[]);

    let depth = schedule_depths(&ids, &callees);
    ids.iter()
        .enumerate()
        .map(|(i, p)| {
            (
                *p,
                ProcSummary {
                    proc: *p,
                    purity: purity[i].clone(),
                    read_set: reads[i].clone(),
                    write_set: writes[i].clone(),
                    io: io.get(p).cloned().unwrap_or_default(),
                    callees: callees[i].clone(),
                    depth: depth[i],
                    regions: BTreeMap::new(),       // Phase 6: populated by frontend
                    pts_effects: Vec::new(),         // Phase 7: populated by frontend
                    const_lattice: const_lattice.clone(), // Phase 8: formal values from constprop
                },
            )
        })
        .collect()
}

/// A schedule depth per procedure: the longest call chain beneath it, with any
/// cycle collapsed to its component's depth.
fn schedule_depths(ids: &[ProcId], callees: &[BTreeSet<ProcId>]) -> Vec<usize> {
    let index = |p: ProcId| ids.iter().position(|x| *x == p);
    let n = ids.len();
    let mut depth = vec![0usize; n];
    let mut changed = true;
    let mut guard = 0;
    while changed && guard <= n + 1 {
        guard += 1;
        changed = false;
        for i in 0..n {
            let mut want = 0usize;
            for c in &callees[i] {
                if let Some(j) = index(*c) {
                    want = want.max(depth[j] + 1);
                }
            }
            want = want.min(n);
            if want > depth[i] {
                depth[i] = want;
                changed = true;
            }
        }
    }
    depth
}

/// The callees a call may reach.
fn callees_of(callee: &Callee, caller: ProcId, cg: &CallGraph) -> Vec<ProcId> {
    match callee {
        Callee::Direct(c) => vec![*c],
        Callee::Indirect(IpaExpr::FnAddr(q)) => vec![*q],
        Callee::Indirect(IpaExpr::Read(Slot::Param(j))) => {
            cg.passed_at(caller, *j).into_iter().collect()
        }
        Callee::Indirect(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipa::callgraph;
    use crate::ipa::program::{Call, Proc};

    fn proc_with(name: &str, params: Vec<&str>, stmts: Vec<IpaStmt>) -> Proc {
        let mut p = Proc::new(name, params.into_iter().map(String::from).collect(), vec![]);
        p.stmts = stmts;
        p
    }

    fn call1(callee: ProcId, args: Vec<IpaExpr>) -> IpaStmt {
        IpaStmt::Call(Call { callee: Callee::Direct(callee), args, ret: None })
    }

    #[test]
    fn a_leaf_has_depth_zero_and_local_sets() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "leaf",
                vec!["a"],
                vec![IpaStmt::Assign { target: Slot::Local(0), value: IpaExpr::Read(Slot::Param(0)) }],
            )],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let s = &compute(&prog, &cg)[&ProcId(0)];
        assert_eq!(s.depth, 0);
        assert!(s.read_set.contains(&Slot::Param(0)));
        assert!(s.write_set.contains(&Slot::Local(0)));
    }

    #[test]
    fn a_caller_summary_folds_in_the_callees_sets() {
        // main calls g(x) where x is main's local 0; g reads its param 0.
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Read(Slot::Local(0))])]),
                proc_with(
                    "g",
                    vec!["y"],
                    vec![IpaStmt::Assign { target: Slot::Local(0), value: IpaExpr::Read(Slot::Param(0)) }],
                ),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let s = &compute(&prog, &cg)[&ProcId(0)];
        // g's read of its formal maps to main's local 0.
        assert!(s.read_set.contains(&Slot::Local(0)));
        assert!(s.callees.contains(&ProcId(1)));
        assert_eq!(s.depth, 1);
    }

    #[test]
    fn reachability_is_transitive() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![])]),
                proc_with("mid", vec![], vec![call1(ProcId(2), vec![])]),
                proc_with("leaf", vec![], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let s = &compute(&prog, &cg)[&ProcId(0)];
        assert!(s.callees.contains(&ProcId(1)) && s.callees.contains(&ProcId(2)));
        assert_eq!(s.depth, 2);
    }

    #[test]
    fn a_written_global_reaches_every_procedure_that_can_call_the_writer() {
        let prog = IpaProgram::new(
            vec!["g".into()],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![])]),
                proc_with("writer", vec![], vec![IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(1) }]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let s = &compute(&prog, &cg)[&ProcId(0)];
        assert!(s.write_set.contains(&Slot::Global(0)));
    }
}
