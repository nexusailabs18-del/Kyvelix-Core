//! Phase 8 §8.2 — call-graph construction (Wolfe §8.5.1).
//!
//! Wolfe's call graph is two things at once:
//!
//! - **edges** `p → q` for each call from `p` to `q`, and
//! - **triplets** `⟨q, k, p⟩`: procedure `q` is passed at argument position `k`
//!   into procedure `p`.
//!
//! The triplets are what make the graph *interprocedural* rather than merely
//! *intraprocedural*: a procedure passed as an argument is called indirectly
//! from inside the callee, so `p → q` is not textually a call at all. §8.2's
//! three cases are exactly the bookkeeping for that:
//!
//! 1. a call site that does not invoke `f_k` where `f_k` is not an argument —
//!    nothing to do;
//! 2. a call site that does not invoke `f_k` but passes `f_k` as argument `m` —
//!    record `⟨f_k, m, callee⟩`;
//! 3. a call site that *does* invoke `f_k` — add the edge and record the
//!    triplets for the procedures it passes.
//!
//! An indirect call `g()` where `g` is a formal parameter resolves through the
//! triplets: every procedure passed at `g`'s position into `p` is a possible
//! callee.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, ProcId, Slot};

/// `⟨passed, position, receiver⟩`: `passed` is passed at argument `position`
/// into `receiver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArgFlow {
    pub passed: ProcId,
    pub position: usize,
    pub receiver: ProcId,
}

/// The call graph of a program.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CallGraph {
    /// `p → { q }` for every call from `p` to `q`, direct or resolved indirect.
    pub edges: BTreeMap<ProcId, BTreeSet<ProcId>>,
    /// The `⟨q, k, p⟩` triplets.
    pub arg_flow: BTreeSet<ArgFlow>,
    /// `(caller, callee) → statement indices` of the direct calls.
    pub call_sites: BTreeMap<(ProcId, ProcId), Vec<usize>>,
    /// Callers that contain a call through a value the analysis could not
    /// resolve; their callee set may be incomplete, so no analysis may assume
    /// their edge set is exhaustive.
    pub unresolved: BTreeSet<ProcId>,
}

impl CallGraph {
    /// The callees of `p`.
    pub fn callees(&self, p: ProcId) -> BTreeSet<ProcId> {
        self.edges.get(&p).cloned().unwrap_or_default()
    }

    /// The callers of `q`.
    pub fn callers(&self, q: ProcId) -> BTreeSet<ProcId> {
        self.edges
            .iter()
            .filter(|(_, s)| s.contains(&q))
            .map(|(p, _)| *p)
            .collect()
    }

    /// The argument positions into `receiver` at which procedures are passed.
    pub fn passed_at(&self, receiver: ProcId, position: usize) -> BTreeSet<ProcId> {
        self.arg_flow
            .iter()
            .filter(|t| t.receiver == receiver && t.position == position)
            .map(|t| t.passed)
            .collect()
    }

    /// The statement indices of the calls from `caller` to `callee`.
    pub fn sites(&self, caller: ProcId, callee: ProcId) -> &[usize] {
        self.call_sites
            .get(&(caller, callee))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Tarjan's strongly-connected-components, iteratively.
///
/// `succ(v)` gives the successors of vertex `v` over `0..n`. Returns the SCCs
/// in reverse topological order (a component's successors come first), which is
/// the order bottom-up summary computation wants. Shared with
/// [`crate::ipa::mod_analysis`], whose binding graph is also an SCC problem.
pub fn tarjan_scc(n: usize, succ: impl Fn(usize) -> Vec<usize>) -> Vec<Vec<usize>> {
    #[derive(Clone, Copy)]
    struct Frame {
        v: usize,
        next: usize,
    }
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut sccs: Vec<Vec<usize>> = Vec::new();
    let mut counter = 0usize;

    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        let mut frames = vec![Frame { v: root, next: 0 }];
        index[root] = counter;
        low[root] = counter;
        counter += 1;
        stack.push(root);
        on_stack[root] = true;

        while let Some(frame) = frames.last_mut() {
            let s = succ(frame.v);
            if frame.next < s.len() {
                let w = s[frame.next];
                frame.next += 1;
                if index[w] == usize::MAX {
                    index[w] = counter;
                    low[w] = counter;
                    counter += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    frames.push(Frame { v: w, next: 0 });
                } else if on_stack[w] {
                    let lv = low[frame.v];
                    low[frame.v] = lv.min(index[w]);
                }
            } else {
                let v = frame.v;
                frames.pop();
                if let Some(parent) = frames.last() {
                    low[parent.v] = low[parent.v].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    comp.sort_unstable();
                    sccs.push(comp);
                }
            }
        }
    }
    sccs
}

/// Resolve an indirect callee expression to the procedures it can denote.
fn resolve_callee(expr: &IpaExpr, caller: ProcId, g: &CallGraph, out: &mut BTreeSet<ProcId>) {
    match expr {
        IpaExpr::FnAddr(q) => {
            out.insert(*q);
        }
        // A call through formal parameter `j` reaches whatever procedure was
        // passed at position `j` into `caller` — the `⟨q, j, caller⟩` triplets.
        IpaExpr::Read(Slot::Param(j)) => {
            for q in g.passed_at(caller, *j) {
                out.insert(q);
            }
        }
        _ => {}
    }
}

/// Build the call graph of a program (Wolfe §8.5.1).
///
/// Pass 1 records the direct edges, the call sites and the `⟨q, k, p⟩` triplets
/// of procedures passed as arguments. Pass 2 resolves indirect calls through
/// those triplets. A call the second pass cannot resolve threads through a value
/// the model does not track (a local holding a function value, say), so its
/// caller is marked [`CallGraph::unresolved`] — the analysis then declines to
/// claim anything that assumes the edge set is complete.
pub fn build(prog: &IpaProgram) -> CallGraph {
    let mut g = CallGraph::default();
    let n = prog.procs.len();

    // Pass 1: direct calls, call sites, and the triplets for passed functions.
    for (i, p) in prog.procs.iter().enumerate() {
        let caller = ProcId(i);
        for (si, stmt) in p.stmts.iter().enumerate() {
            if let IpaStmt::Call(c) = stmt {
                match &c.callee {
                    Callee::Direct(callee) => {
                        g.edges.entry(caller).or_default().insert(*callee);
                        g.call_sites.entry((caller, *callee)).or_default().push(si);
                        for (j, a) in c.args.iter().enumerate() {
                            if let IpaExpr::FnAddr(q) = a {
                                g.arg_flow.insert(ArgFlow {
                                    passed: *q,
                                    position: j,
                                    receiver: *callee,
                                });
                            }
                        }
                    }
                    Callee::Indirect(_) => {
                        // Resolved in pass 2; `unresolved` is set there only if
                        // resolution actually fails, so a caller with both a
                        // resolvable and an unresolvable indirect call keeps the
                        // mark.
                    }
                }
            }
        }
    }

    // Pass 2: resolve indirect calls through the triplets gathered above.
    for (i, p) in prog.procs.iter().enumerate() {
        let caller = ProcId(i);
        for (si, stmt) in p.stmts.iter().enumerate() {
            if let IpaStmt::Call(c) = stmt {
                if let Callee::Indirect(expr) = &c.callee {
                    let mut resolved: BTreeSet<ProcId> = BTreeSet::new();
                    resolve_callee(expr, caller, &g, &mut resolved);
                    if resolved.is_empty() {
                        g.unresolved.insert(caller);
                    } else {
                        for q in &resolved {
                            g.edges.entry(caller).or_default().insert(*q);
                            g.call_sites.entry((caller, *q)).or_default().push(si);
                            for (j, a) in c.args.iter().enumerate() {
                                if let IpaExpr::FnAddr(f) = a {
                                    g.arg_flow.insert(ArgFlow {
                                        passed: *f,
                                        position: j,
                                        receiver: *q,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Keep only real procedures.
    for set in g.edges.values_mut() {
        set.retain(|q| q.index() < n);
    }
    g.edges.retain(|p, s| p.index() < n && !s.is_empty());
    g
}

impl CallGraph {
    /// The SCCs of the call graph, successors first (a component's callees come
    /// before it) — the order bottom-up summary computation consumes.
    pub fn sccs(&self, n: usize) -> Vec<Vec<usize>> {
        tarjan_scc(n, |v| {
            self.callees(ProcId(v)).into_iter().map(|q| q.index()).collect()
        })
    }

    /// True when `p` is on a call cycle (its SCC has more than one member, or it
    /// calls itself).
    pub fn is_recursive(&self, p: ProcId, n: usize) -> bool {
        self.sccs(n).into_iter().any(|c| c.len() > 1 && c.contains(&p.index()))
            || self.callees(p).contains(&p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipa::program::{Call, IpaStmt, Proc};

    fn proc_with(name: &str, params: Vec<&str>, stmts: Vec<IpaStmt>) -> Proc {
        let mut p = Proc::new(name, params.into_iter().map(String::from).collect(), vec![]);
        p.stmts = stmts;
        p
    }

    fn direct(callee: ProcId, args: Vec<IpaExpr>) -> IpaStmt {
        IpaStmt::Call(Call { callee: Callee::Direct(callee), args, ret: None })
    }

    #[test]
    fn direct_calls_become_edges_and_sites() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![direct(ProcId(1), vec![])]),
                proc_with("helper", vec![], vec![]),
            ],
            ProcId(0),
        );
        let g = build(&prog);
        assert!(g.callees(ProcId(0)).contains(&ProcId(1)));
        assert_eq!(g.sites(ProcId(0), ProcId(1)), &[0]);
    }

    #[test]
    fn a_passed_function_records_a_triplet() {
        // main(f) calls apply(f, &helper): helper passed at position 1 to apply.
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with(
                    "main",
                    vec!["f"],
                    vec![IpaStmt::Call(Call {
                        callee: Callee::Direct(ProcId(1)),
                        args: vec![IpaExpr::Read(Slot::Param(0)), IpaExpr::FnAddr(ProcId(2))],
                        ret: None,
                    })],
                ),
                proc_with("apply", vec!["g"], vec![]),
                proc_with("helper", vec![], vec![]),
            ],
            ProcId(0),
        );
        let g = build(&prog);
        assert!(g.arg_flow.contains(&ArgFlow {
            passed: ProcId(2),
            position: 1,
            receiver: ProcId(1)
        }));
    }

    #[test]
    fn an_indirect_call_through_a_formal_resolves_through_triplets() {
        // main passes &helper to apply; apply calls its formal g().
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with(
                    "main",
                    vec![],
                    vec![IpaStmt::Call(Call {
                        callee: Callee::Direct(ProcId(1)),
                        args: vec![IpaExpr::FnAddr(ProcId(2))],
                        ret: None,
                    })],
                ),
                proc_with(
                    "apply",
                    vec!["g"],
                    vec![IpaStmt::Call(Call {
                        callee: Callee::Indirect(IpaExpr::Read(Slot::Param(0))),
                        args: vec![],
                        ret: None,
                    })],
                ),
                proc_with("helper", vec![], vec![]),
            ],
            ProcId(0),
        );
        let g = build(&prog);
        assert!(g.callees(ProcId(1)).contains(&ProcId(2)));
        assert!(!g.unresolved.contains(&ProcId(1)));
    }

    #[test]
    fn an_unresolvable_indirect_call_is_marked_unresolved() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "main",
                vec![],
                vec![IpaStmt::Call(Call {
                    callee: Callee::Indirect(IpaExpr::Read(Slot::Local(0))),
                    args: vec![],
                    ret: None,
                })],
            )],
            ProcId(0),
        );
        let g = build(&prog);
        assert!(g.unresolved.contains(&ProcId(0)));
    }

    #[test]
    fn tarjan_finds_a_cycle_and_orders_successors_first() {
        // 0 → 1 → 2 → 1 (cycle {1,2}), 0 → 2.
        let succ = |v: usize| match v {
            0 => vec![1, 2],
            1 => vec![2],
            2 => vec![1],
            _ => vec![],
        };
        let sccs = tarjan_scc(3, succ);
        assert!(sccs.iter().any(|c| c == &vec![1, 2]));
        assert!(sccs.iter().any(|c| c == &vec![0]));
        let pos = |t: &[usize]| sccs.iter().position(|c| c == t).unwrap();
        assert!(pos(&[1, 2]) < pos(&[0]));
    }

    #[test]
    fn recursion_is_detected() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with("f", vec![], vec![direct(ProcId(0), vec![])])],
            ProcId(0),
        );
        let g = build(&prog);
        assert!(g.is_recursive(ProcId(0), 1));
    }
}
