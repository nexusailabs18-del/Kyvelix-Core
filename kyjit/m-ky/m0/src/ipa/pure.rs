//! Phase 8 §8.4 — interprocedural purity (Wolfe §8.4).
//!
//! Phase 5's [`crate::ir::purity`] classifies one function's body given a *set
//! of names already known pure*. Phase 8 supplies that set properly: purity is
//! computed over the whole call graph, bottom-up, so a chain
//! `main → outer → inner` is only `Pure` when `inner` is, and a `PureSubroutine`
//! that modifies its third parameter makes every caller that passes a *global*
//! there `Impure`.
//!
//! The lattice is Phase 5's three-valued [`Purity`], and the direction is the
//! same: every ambiguity resolves downward (`Pure` → `PureSubroutine` →
//! `Impure`), because guessing `Pure` wrongly would let a caller delete a call
//! with side effects. Purity is computed to a fixpoint over the call graph,
//! since mutual recursion means a caller's purity can depend on its callee's.
//! Each step only ever degrades, so the iteration terminates.

use std::collections::BTreeSet;

use crate::ipa::callgraph::CallGraph;
use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, ProcId, Slot};
use crate::ir::purity::Purity;

/// True when the expression reads any global.
fn reads_global(e: &IpaExpr) -> bool {
    let mut slots = Vec::new();
    e.slots_read(&mut slots);
    slots.iter().any(|s| matches!(s, Slot::Global(_)))
}

/// One procedure's purity ignoring its callees: side effects in its own body.
struct Local {
    impure: bool,
    modified_params: BTreeSet<usize>,
}

fn local_purity(prog: &IpaProgram, p: ProcId) -> Local {
    let proc = match prog.proc(p) {
        Some(pr) => pr,
        None => return Local { impure: true, modified_params: BTreeSet::new() },
    };
    let mut impure = false;
    let mut modified_params = BTreeSet::new();

    for stmt in &proc.stmts {
        match stmt {
            IpaStmt::Io { .. } => impure = true,
            IpaStmt::Assign { target, value } => {
                if reads_global(value) {
                    impure = true;
                }
                match target {
                    Slot::Global(_) => impure = true,
                    Slot::Param(k) => {
                        modified_params.insert(*k);
                    }
                    Slot::Local(_) => {}
                }
            }
            IpaStmt::Call(c) => {
                for a in &c.args {
                    if reads_global(a) {
                        impure = true;
                    }
                }
                if let Some(r) = c.ret {
                    match r {
                        Slot::Global(_) => impure = true,
                        Slot::Param(k) => {
                            modified_params.insert(k);
                        }
                        Slot::Local(_) => {}
                    }
                }
            }
        }
    }
    if let Some(r) = &proc.ret {
        if reads_global(r) {
            impure = true;
        }
    }
    Local { impure, modified_params }
}

/// Interprocedural purity for every procedure (Wolfe §8.4).
///
/// Returns one entry per procedure, in declaration order.
pub fn classify(prog: &IpaProgram, cg: &CallGraph) -> Vec<Purity> {
    let n = prog.procs.len();
    let locals: Vec<Local> = prog.ids().map(|p| local_purity(prog, p)).collect();
    // Start every procedure pure and let the fixpoint degrade it.
    let mut modified: Vec<BTreeSet<usize>> = (0..n).map(|_| BTreeSet::new()).collect();
    let mut impure: Vec<bool> = vec![false; n];

    let mut changed = true;
    let mut guard = 0;
    while changed && guard < n + 2 {
        guard += 1;
        changed = false;
        for (i, proc) in prog.procs.iter().enumerate() {
            let me = ProcId(i);
            if locals[i].impure && !impure[i] {
                impure[i] = true;
                changed = true;
            }
            for k in &locals[i].modified_params {
                if modified[i].insert(*k) {
                    changed = true;
                }
            }
            // Then the callees' facts, mapped through each call's arguments.
            for stmt in &proc.stmts {
                let call = match stmt {
                    IpaStmt::Call(c) => c,
                    _ => continue,
                };
                for callee in callees_of(&call.callee, me, cg) {
                    let j = callee.index();
                    if impure[j] && !impure[i] {
                        impure[i] = true;
                        changed = true;
                    }
                    for k in modified[j].clone() {
                        // Formal `k` of the callee maps to argument `k` here. A
                        // `&G` actual binds the same cell as a `G` actual: the
                        // callee writes through the pointer, so the caller's
                        // global is modified just the same.
                        match call.args.get(k) {
                            Some(IpaExpr::Read(Slot::Global(_)) | IpaExpr::AddrOf(Slot::Global(_))) => {
                                if !impure[i] {
                                    impure[i] = true;
                                    changed = true;
                                }
                            }
                            Some(IpaExpr::Read(Slot::Param(kp)) | IpaExpr::AddrOf(Slot::Param(kp))) => {
                                if modified[i].insert(*kp) {
                                    changed = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    (0..n)
        .map(|i| {
            if impure[i] {
                Purity::Impure
            } else if modified[i].is_empty() {
                Purity::Pure
            } else {
                Purity::PureSubroutine {
                    modified: modified[i].iter().copied().collect(),
                }
            }
        })
        .collect()
}

/// The callees a call may reach: a direct callee, or every procedure passed at
/// the called formal's position.
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
    use crate::ipa::program::{Call, IoKind, Proc};

    fn proc_with(name: &str, params: Vec<&str>, stmts: Vec<IpaStmt>) -> Proc {
        let mut p = Proc::new(name, params.into_iter().map(String::from).collect(), vec![]);
        p.stmts = stmts;
        p
    }

    fn call1(callee: ProcId, args: Vec<IpaExpr>) -> IpaStmt {
        IpaStmt::Call(Call { callee: Callee::Direct(callee), args, ret: None })
    }

    #[test]
    fn a_body_with_no_effects_is_pure() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with("add", vec!["a", "b"], vec![]), proc_with("main", vec![], vec![])],
            ProcId(1),
        );
        let cg = callgraph::build(&prog);
        assert_eq!(classify(&prog, &cg), vec![Purity::Pure, Purity::Pure]);
    }

    #[test]
    fn writing_a_global_is_impure() {
        let prog = IpaProgram::new(
            vec!["g".into()],
            vec![proc_with(
                "bump",
                vec![],
                vec![IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(1) }],
            )],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        assert_eq!(classify(&prog, &cg)[0], Purity::Impure);
    }

    #[test]
    fn io_is_impure() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "print",
                vec![],
                vec![IpaStmt::Io { channel: "stdout".into(), kind: IoKind::Write }],
            )],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        assert_eq!(classify(&prog, &cg)[0], Purity::Impure);
    }

    #[test]
    fn writing_a_parameter_is_a_pure_subroutine() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "incr",
                vec!["x"],
                vec![IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(1) }],
            )],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        assert_eq!(classify(&prog, &cg)[0], Purity::PureSubroutine { modified: vec![0] });
    }

    #[test]
    fn a_callee_writing_a_global_makes_its_caller_impure() {
        let prog = IpaProgram::new(
            vec!["g".into()],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![])]),
                proc_with(
                    "writer",
                    vec![],
                    vec![IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(0) }],
                ),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let p = classify(&prog, &cg);
        assert_eq!(p[1], Purity::Impure);
        assert_eq!(p[0], Purity::Impure);
    }

    #[test]
    fn passing_a_global_to_a_pure_subroutine_makes_the_caller_impure() {
        let prog = IpaProgram::new(
            vec!["g".into()],
            vec![
                proc_with(
                    "main",
                    vec![],
                    vec![call1(ProcId(1), vec![IpaExpr::Read(Slot::Global(0))])],
                ),
                proc_with(
                    "incr",
                    vec!["x"],
                    vec![IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(0) }],
                ),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let p = classify(&prog, &cg);
        assert_eq!(p[1], Purity::PureSubroutine { modified: vec![0] });
        assert_eq!(p[0], Purity::Impure);
    }

    #[test]
    fn a_pure_callee_does_not_tarnish_its_caller() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![])]),
                proc_with("add", vec!["a", "b"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        assert_eq!(classify(&prog, &cg)[0], Purity::Pure);
    }
}
