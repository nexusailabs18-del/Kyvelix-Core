//! Phase 8 §8.8 — top-down context refinement (Wolfe §8.5).
//!
//! The bottom-up summaries of [`crate::ipa::summaries`] are context-insensitive:
//! if `helper` is called from fifty sites, its summary is the union of all fifty.
//! Cloning a summary per distinct **context** recovers precision — `helper(0)`'s
//! formal is read-only, because a constant cannot be written.
//!
//! This module walks the call graph *downward* from the entry, deriving each
//! callee's context from the argument expressions at the call site evaluated
//! under the caller's context. The number of contexts per procedure is capped at
//! [`MAX_CONTEXTS`]; past the cap the bottom-up summary is used unchanged, so a
//! program with pathological call-site variety degrades in precision rather than
//! in termination.

use std::collections::{BTreeMap, BTreeSet};

use crate::ipa::callgraph::CallGraph;
use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, ProcId, Slot};
use crate::ipa::summaries::ProcSummary;

/// The cap on distinct contexts cloned for a single procedure.
pub const MAX_CONTEXTS: usize = 8;

/// What a context knows about one argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgShape {
    Const(i64),
    Unknown,
}

/// A procedure plus the shapes of the arguments it was entered with.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContextKey {
    pub proc: ProcId,
    pub args: Vec<ArgShape>,
}

impl ContextKey {
    /// A context for `proc` with every argument unknown (the root context).
    pub fn root(proc: ProcId, nformals: usize) -> ContextKey {
        ContextKey { proc, args: vec![ArgShape::Unknown; nformals] }
    }
}

/// Specialise a summary for a context: a constant argument can be neither read
/// nor written, so it leaves the read/write sets.
pub fn specialize(base: &ProcSummary, ctx: &ContextKey) -> ProcSummary {
    let mut s = base.clone();
    for (j, shape) in ctx.args.iter().enumerate() {
        if matches!(shape, ArgShape::Const(_)) {
            s.read_set.remove(&Slot::Param(j));
            s.write_set.remove(&Slot::Param(j));
        }
    }
    s
}

/// Derive the callee's context from a call site evaluated under the caller's
/// context.
pub fn derive_context(
    caller_ctx: &ContextKey,
    call: &crate::ipa::program::Call,
    callee: ProcId,
    nformals: usize,
) -> ContextKey {
    let mut args = Vec::with_capacity(nformals);
    for j in 0..nformals {
        let shape = match call.args.get(j) {
            Some(e) => shape_of(e, caller_ctx),
            None => ArgShape::Unknown,
        };
        args.push(shape);
    }
    ContextKey { proc: callee, args }
}

/// The shape of an argument expression evaluated under a context.
fn shape_of(e: &IpaExpr, ctx: &ContextKey) -> ArgShape {
    if let Some(c) = e.as_const() {
        return ArgShape::Const(c);
    }
    match e {
        IpaExpr::Read(Slot::Param(j)) => ctx.args.get(*j).copied().unwrap_or(ArgShape::Unknown),
        _ => ArgShape::Unknown,
    }
}

/// Refine the bottom-up summaries with top-down contexts (Wolfe §8.5).
///
/// Returns `(context, summary)` pairs. A procedure appears at most
/// [`MAX_CONTEXTS`] times; past the cap its remaining contexts fall back to the
/// bottom-up summary (not returned here, since the caller already has it).
pub fn refine(
    prog: &IpaProgram,
    cg: &CallGraph,
    summaries: &BTreeMap<ProcId, ProcSummary>,
    entry: ProcId,
) -> Vec<(ContextKey, ProcSummary)> {
    let nformals = |p: ProcId| prog.proc(p).map(|x| x.params.len()).unwrap_or(0);
    let mut seen: BTreeSet<ContextKey> = BTreeSet::new();
    let mut count: BTreeMap<ProcId, usize> = BTreeMap::new();
    let mut out: Vec<(ContextKey, ProcSummary)> = Vec::new();
    let mut work: Vec<ContextKey> = vec![ContextKey::root(entry, nformals(entry))];

    while let Some(ctx) = work.pop() {
        if seen.contains(&ctx) {
            continue;
        }
        let used = count.entry(ctx.proc).or_insert(0);
        if *used >= MAX_CONTEXTS {
            continue;
        }
        *used += 1;
        seen.insert(ctx.clone());

        let base = match summaries.get(&ctx.proc) {
            Some(s) => s,
            None => continue,
        };
        out.push((ctx.clone(), specialize(base, &ctx)));

        let proc = match prog.proc(ctx.proc) {
            Some(p) => p,
            None => continue,
        };
        for stmt in &proc.stmts {
            let call = match stmt {
                IpaStmt::Call(c) => c,
                _ => continue,
            };
            for callee in callees_of(&call.callee, ctx.proc, cg) {
                if callee.index() >= prog.procs.len() {
                    continue;
                }
                work.push(derive_context(&ctx, call, callee, nformals(callee)));
            }
        }
    }

    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
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
    use crate::ipa::summaries;

    fn proc_with(name: &str, params: Vec<&str>, stmts: Vec<IpaStmt>) -> Proc {
        let mut p = Proc::new(name, params.into_iter().map(String::from).collect(), vec![]);
        p.stmts = stmts;
        p
    }

    fn call1(callee: ProcId, args: Vec<IpaExpr>) -> IpaStmt {
        IpaStmt::Call(Call { callee: Callee::Direct(callee), args, ret: None })
    }

    #[test]
    fn a_constant_argument_is_removed_from_the_read_and_write_sets() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with("g", vec!["x"], vec![])],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let base = &summaries::compute(&prog, &cg)[&ProcId(0)];
        let mut base2 = base.clone();
        base2.read_set.insert(Slot::Param(0));
        base2.write_set.insert(Slot::Param(0));
        let ctx = ContextKey { proc: ProcId(0), args: vec![ArgShape::Const(3)] };
        let spec = specialize(&base2, &ctx);
        assert!(!spec.read_set.contains(&Slot::Param(0)));
        assert!(!spec.write_set.contains(&Slot::Param(0)));
    }

    #[test]
    fn derive_context_propagates_a_callers_constant_into_the_callee() {
        let caller_ctx = ContextKey { proc: ProcId(0), args: vec![ArgShape::Const(7)] };
        let call = Call {
            callee: Callee::Direct(ProcId(1)),
            args: vec![IpaExpr::Read(Slot::Param(0))],
            ret: None,
        };
        let ctx = derive_context(&caller_ctx, &call, ProcId(1), 1);
        assert_eq!(ctx.args, vec![ArgShape::Const(7)]);
    }

    #[test]
    fn a_literal_argument_is_a_constant_shape() {
        let caller_ctx = ContextKey::root(ProcId(0), 0);
        let call = Call { callee: Callee::Direct(ProcId(1)), args: vec![IpaExpr::Const(9)], ret: None };
        let ctx = derive_context(&caller_ctx, &call, ProcId(1), 1);
        assert_eq!(ctx.args, vec![ArgShape::Const(9)]);
    }

    #[test]
    fn refine_walks_down_from_the_entry() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Const(5)])]),
                proc_with("g", vec!["x"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let base = summaries::compute(&prog, &cg);
        let refined = refine(&prog, &cg, &base, ProcId(0));
        // Both main and g appear, and g's context knows its argument is 5.
        assert!(refined.iter().any(|(c, _)| c.proc == ProcId(1) && c.args == vec![ArgShape::Const(5)]));
    }

    #[test]
    fn the_context_cap_is_respected() {
        // A procedure called from many distinct constant sites still appears at
        // most MAX_CONTEXTS times.
        let mut stmts = Vec::new();
        let mut procs = vec![proc_with("main", vec![], vec![])];
        procs.push(proc_with("g", vec!["x"], vec![]));
        for k in 0..(MAX_CONTEXTS + 4) {
            stmts.push(call1(ProcId(1), vec![IpaExpr::Const(k as i64)]));
        }
        procs[0].stmts = stmts;
        let prog = IpaProgram::new(vec![], procs, ProcId(0));
        let cg = callgraph::build(&prog);
        let base = summaries::compute(&prog, &cg);
        let refined = refine(&prog, &cg, &base, ProcId(0));
        let g_contexts = refined.iter().filter(|(c, _)| c.proc == ProcId(1)).count();
        assert!(g_contexts <= MAX_CONTEXTS, "got {} contexts", g_contexts);
    }
}