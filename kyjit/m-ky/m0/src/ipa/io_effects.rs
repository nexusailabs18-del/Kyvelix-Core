//! Phase 8 §8.3 — I/O dependence summary (Wolfe §8.3).
//!
//! > "Two I/O statements ... if the order is important ... insert dependence
//! >  relations between them."
//!
//! The rule is the one [`crate::ir::effect`] already uses at the array level,
//! lifted to whole procedures: two I/O operations on the **same channel** must
//! keep their order; two on different channels commute. A procedure's I/O
//! summary is the set of `(channel, kind)` pairs it can perform, transitively
//! through its callees.
//!
//! Reading and writing the same channel both order against every other
//! operation on that channel — a read must see the writes before it and a write
//! must be seen by the reads after it — so [`must_serialize`] is simply "same
//! channel".

use std::collections::BTreeMap;

use crate::ipa::callgraph::CallGraph;
use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, IoKind, ProcId, Slot};

/// One I/O operation, located by the procedure and statement it occurs in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IoOp {
    pub proc: ProcId,
    pub channel: String,
    pub kind: IoKind,
    /// Statement index within `proc`.
    pub stmt: usize,
}

/// Must two I/O operations keep their order? Exactly when they share a channel.
pub fn must_serialize(a: &IoOp, b: &IoOp) -> bool {
    a.channel == b.channel
}

/// The I/O operations in one procedure's own body, in statement order.
pub fn io_ops(prog: &IpaProgram, p: ProcId) -> Vec<IoOp> {
    let mut out = Vec::new();
    let proc = match prog.proc(p) {
        Some(pr) => pr,
        None => return out,
    };
    for (si, stmt) in proc.stmts.iter().enumerate() {
        if let IpaStmt::Io { channel, kind } = stmt {
            out.push(IoOp { proc: p, channel: channel.clone(), kind: *kind, stmt: si });
        }
    }
    out
}

/// A procedure's I/O summary: `channel → (reads, writes)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IoSummary {
    pub channels: BTreeMap<String, (bool, bool)>,
}

impl IoSummary {
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// True when this summary performs I/O on `channel`.
    pub fn touches(&self, channel: &str) -> bool {
        self.channels.contains_key(channel)
    }

    fn record(&mut self, op: &IoOp) -> bool {
        let entry = self.channels.entry(op.channel.clone()).or_insert((false, false));
        let before = *entry;
        match op.kind {
            IoKind::Read => entry.0 = true,
            IoKind::Write => entry.1 = true,
        }
        *entry != before
    }

    fn merge(&mut self, other: &IoSummary) -> bool {
        let mut changed = false;
        for (c, (r, w)) in &other.channels {
            let entry = self.channels.entry(c.clone()).or_insert((false, false));
            let before = *entry;
            entry.0 |= *r;
            entry.1 |= *w;
            changed |= *entry != before;
        }
        changed
    }
}

/// The transitive I/O summary of every procedure: each procedure's own I/O plus
/// its callees', to a fixpoint (so mutual recursion is seen).
pub fn summaries(prog: &IpaProgram, cg: &CallGraph) -> BTreeMap<ProcId, IoSummary> {
    let n = prog.procs.len();
    let mut out: BTreeMap<ProcId, IoSummary> = BTreeMap::new();
    for p in prog.ids() {
        let mut s = IoSummary::default();
        for op in io_ops(prog, p) {
            s.record(&op);
        }
        out.insert(p, s);
    }

    let mut changed = true;
    let mut guard = 0;
    while changed && guard <= n + 1 {
        guard += 1;
        changed = false;
        for p in prog.ids() {
            let callees: Vec<ProcId> = cg.callees(p).into_iter().collect();
            let mut add = IoSummary::default();
            for c in callees {
                if let Some(s) = out.get(&c) {
                    add.merge(s);
                }
            }
            if let Some(mine) = out.get_mut(&p) {
                if mine.merge(&add) {
                    changed = true;
                }
            }
        }
    }
    out
}

/// True when `p` (transitively) performs any I/O.
pub fn has_io(prog: &IpaProgram, cg: &CallGraph) -> BTreeMap<ProcId, bool> {
    summaries(prog, cg)
        .into_iter()
        .map(|(p, s)| (p, !s.is_empty()))
        .collect()
}

/// The callees a call may reach, for completeness of the callee walk.
#[allow(dead_code)]
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

    fn proc_with(name: &str, stmts: Vec<IpaStmt>) -> Proc {
        let mut p = Proc::new(name, vec![], vec![]);
        p.stmts = stmts;
        p
    }

    #[test]
    fn same_channel_serializes_and_different_channels_do_not() {
        let a = IoOp { proc: ProcId(0), channel: "stdout".into(), kind: IoKind::Write, stmt: 0 };
        let b = IoOp { proc: ProcId(0), channel: "stdout".into(), kind: IoKind::Read, stmt: 1 };
        let c = IoOp { proc: ProcId(0), channel: "stdin".into(), kind: IoKind::Read, stmt: 2 };
        assert!(must_serialize(&a, &b));
        assert!(!must_serialize(&a, &c));
    }

    #[test]
    fn a_summary_records_reads_and_writes_per_channel() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "p",
                vec![
                    IpaStmt::Io { channel: "f".into(), kind: IoKind::Write },
                    IpaStmt::Io { channel: "f".into(), kind: IoKind::Read },
                    IpaStmt::Io { channel: "g".into(), kind: IoKind::Read },
                ],
            )],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let s = &summaries(&prog, &cg)[&ProcId(0)];
        assert_eq!(s.channels["f"], (true, true));
        assert_eq!(s.channels["g"], (true, false));
        assert!(s.touches("f"));
        assert!(!s.touches("h"));
    }

    #[test]
    fn a_callees_io_is_visible_through_the_summary() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with(
                    "main",
                    vec![IpaStmt::Call(Call {
                        callee: Callee::Direct(ProcId(1)),
                        args: vec![],
                        ret: None,
                    })],
                ),
                proc_with("writer", vec![IpaStmt::Io { channel: "log".into(), kind: IoKind::Write }]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let s = &summaries(&prog, &cg)[&ProcId(0)];
        assert!(s.touches("log"));
        assert!(has_io(&prog, &cg)[&ProcId(0)]);
    }
}
