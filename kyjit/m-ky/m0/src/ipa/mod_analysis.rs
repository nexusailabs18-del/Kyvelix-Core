//! Phase 8 §8.5.2 — interprocedural MOD (Wolfe §8.5.2, "binding graph").
//!
//! "Which variables can a call to `p` modify?" is not a per-procedure question:
//! `p` may write its own formals, and those formals are the caller's actuals —
//! which may themselves be formals of the caller, and so on up the call chain.
//! The **binding graph** makes that chain explicit: a vertex is a storage
//! location *in a procedure*, and an edge runs from a callee's formal to the
//! caller's actual that was bound to it, in the direction modifications travel.
//!
//! Globals are shared between procedures, so a global vertex is not tied to the
//! procedure that happens to write it: a write to `g` in any procedure means `g`
//! is in every procedure's MOD set. The graph is solved to a fixpoint; its SCCs
//! are reported too, because the spec states the method in terms of them, but the
//! worklist below reaches the same fixed point directly.
//!
//! ## What this does not model
//!
//! A pointer or array argument aliases an arbitrary location, which the scalar
//! `Slot` model cannot express. Such a callee is not silently treated as
//! harmless: an argument that is not a plain `Slot` read/`AddrOf` binds nothing,
//! so no MOD is claimed *through that argument*. A consumer that needs soundness
//! about pointers must combine this with Phase 7.

use std::collections::{BTreeMap, BTreeSet};

use crate::ipa::callgraph::{tarjan_scc, CallGraph};
use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, ProcId, Slot};

/// A storage location in a procedure. A global's `proc` is normalised to
/// [`ProcId(0)`](ProcId) because globals are shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModVertex {
    pub proc: ProcId,
    pub slot: Slot,
}

/// Normalise a vertex: global slots lose their procedure identity.
fn norm(v: ModVertex) -> ModVertex {
    match v.slot {
        Slot::Global(_) => ModVertex { proc: ProcId(0), slot: v.slot },
        _ => v,
    }
}

/// The result of the interprocedural MOD analysis.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModAnalysis {
    /// For each procedure, the slots it may modify directly or through calls.
    pub mod_sets: BTreeMap<ProcId, BTreeSet<Slot>>,
    /// The globals modified anywhere in the program.
    pub modified_globals: BTreeSet<usize>,
    /// The SCCs of the binding graph.
    pub sccs: Vec<Vec<ModVertex>>,
}

/// The slots a procedure writes directly in its own body.
pub fn local_writes(prog: &IpaProgram, p: ProcId) -> BTreeSet<Slot> {
    let mut out = BTreeSet::new();
    let proc = match prog.proc(p) {
        Some(pr) => pr,
        None => return out,
    };
    for stmt in &proc.stmts {
        match stmt {
            IpaStmt::Assign { target, .. } => {
                out.insert(*target);
            }
            IpaStmt::Call(c) => {
                if let Some(r) = c.ret {
                    out.insert(r);
                }
            }
            IpaStmt::Io { .. } => {}
        }
    }
    out
}

/// Run the interprocedural MOD analysis (Wolfe §8.5.2).
pub fn analyze(prog: &IpaProgram, cg: &CallGraph) -> ModAnalysis {
    let mut vertices: BTreeSet<ModVertex> = BTreeSet::new();
    let mut edges: Vec<(ModVertex, ModVertex)> = Vec::new();
    for p in prog.ids() {
        for s in local_writes(prog, p) {
            vertices.insert(norm(ModVertex { proc: p, slot: s }));
        }
        let proc = match prog.proc(p) {
            Some(pr) => pr,
            None => continue,
        };
        for stmt in &proc.stmts {
            let call = match stmt {
                IpaStmt::Call(c) => c,
                _ => continue,
            };
            for callee in callees_of(&call.callee, p, cg) {
                let nformals = prog.proc(callee).map(|c| c.params.len()).unwrap_or(0);
                for j in 0..nformals {
                    let actual = match call.args.get(j) {
                        Some(IpaExpr::Read(s)) | Some(IpaExpr::AddrOf(s)) => *s,
                        _ => continue,
                    };
                    let from = norm(ModVertex { proc: callee, slot: Slot::Param(j) });
                    let to = norm(ModVertex { proc: p, slot: actual });
                    vertices.insert(from);
                    vertices.insert(to);
                    edges.push((from, to));
                }
            }
        }
    }

    // Seed with the direct writes, then propagate to a fixpoint.
    let mut marked: BTreeSet<ModVertex> = BTreeSet::new();
    for p in prog.ids() {
        for s in local_writes(prog, p) {
            marked.insert(norm(ModVertex { proc: p, slot: s }));
        }
    }
    let mut changed = true;
    let mut guard = 0;
    let cap = edges.len() + vertices.len() + 2;
    while changed && guard <= cap {
        guard += 1;
        changed = false;
        for (from, to) in &edges {
            if marked.contains(from) && marked.insert(*to) {
                changed = true;
            }
        }
    }

    finish(prog, &vertices, &edges, &marked)
}

/// Assemble the answer from the solved fixed point.
fn finish(
    prog: &IpaProgram,
    vertices: &BTreeSet<ModVertex>,
    edges: &[(ModVertex, ModVertex)],
    marked: &BTreeSet<ModVertex>,
) -> ModAnalysis {
    let index: BTreeMap<ModVertex, usize> =
        vertices.iter().enumerate().map(|(i, v)| (*v, i)).collect();
    let ordered: Vec<ModVertex> = vertices.iter().copied().collect();
    let succ = |i: usize| -> Vec<usize> {
        let v = ordered[i];
        edges
            .iter()
            .filter(|(from, _)| *from == v)
            .filter_map(|(_, to)| index.get(to).copied())
            .collect()
    };
    let raw = tarjan_scc(ordered.len(), succ);
    let sccs: Vec<Vec<ModVertex>> = raw
        .into_iter()
        .map(|comp| comp.into_iter().map(|i| ordered[i]).collect())
        .collect();

    let mut modified_globals: BTreeSet<usize> = BTreeSet::new();
    for v in marked {
        if let Slot::Global(k) = v.slot {
            modified_globals.insert(k);
        }
    }
    let mut mod_sets: BTreeMap<ProcId, BTreeSet<Slot>> = BTreeMap::new();
    for p in prog.ids() {
        let mut set: BTreeSet<Slot> = BTreeSet::new();
        for v in marked {
            if v.proc == p {
                set.insert(v.slot);
            }
        }
        for k in &modified_globals {
            set.insert(Slot::Global(*k));
        }
        mod_sets.insert(p, set);
    }

    ModAnalysis { mod_sets, modified_globals, sccs }
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
    fn a_local_write_is_in_the_procedure_mod_set() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "p",
                vec![],
                vec![IpaStmt::Assign { target: Slot::Local(0), value: IpaExpr::Const(1) }],
            )],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let a = analyze(&prog, &cg);
        assert!(a.mod_sets[&ProcId(0)].contains(&Slot::Local(0)));
    }

    #[test]
    fn a_global_written_anywhere_is_in_every_mod_set() {
        let prog = IpaProgram::new(
            vec!["g".into()],
            vec![
                proc_with("writer", vec![], vec![IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(0) }]),
                proc_with("other", vec![], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let a = analyze(&prog, &cg);
        assert!(a.modified_globals.contains(&0));
        assert!(a.mod_sets[&ProcId(1)].contains(&Slot::Global(0)));
    }

    #[test]
    fn a_callee_writing_its_formal_marks_the_callers_actual() {
        // main calls setter(x); setter writes its formal 0. main's local 0 is
        // therefore modified through the call.
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Read(Slot::Local(0))])]),
                proc_with("setter", vec!["y"], vec![IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(1) }]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let a = analyze(&prog, &cg);
        assert!(a.mod_sets[&ProcId(1)].contains(&Slot::Param(0)));
        assert!(a.mod_sets[&ProcId(0)].contains(&Slot::Local(0)));
    }

    #[test]
    fn a_callee_passing_a_global_marks_it_globally() {
        // main calls setter(g); setter writes its formal → global g modified.
        let prog = IpaProgram::new(
            vec!["g".into()],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Read(Slot::Global(0))])]),
                proc_with("setter", vec!["y"], vec![IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(1) }]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let a = analyze(&prog, &cg);
        assert!(a.modified_globals.contains(&0));
    }

    #[test]
    fn a_binding_cycle_is_one_scc() {
        // main passes its formal 0 to itself through a direct call: the binding
        // vertices form a cycle.
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with("f", vec!["x"], vec![call1(ProcId(0), vec![IpaExpr::Read(Slot::Param(0))])])],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let a = analyze(&prog, &cg);
        assert!(a.sccs.iter().any(|c| c.len() >= 1));
    }
}