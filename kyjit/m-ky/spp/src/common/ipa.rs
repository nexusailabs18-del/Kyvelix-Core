//! SPP Local copy of IPA types (from UDDP m0)
//! This allows SPP to be independent of UDDP internals

use serde::{Deserialize, Serialize};

/// A procedure, by index into [`IpaProgram::procs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProcId(pub usize);

impl ProcId {
    pub fn index(self) -> usize {
        self.0
    }
}

/// A storage location a statement reads or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    /// The `k`-th formal parameter of the enclosing procedure.
    Param(usize),
    /// The `k`-th local of the enclosing procedure.
    Local(usize),
    /// The `k`-th global of the whole program.
    Global(usize),
}

impl Slot {
    pub fn as_str(self) -> String {
        match self {
            Slot::Param(k) => format!("p#{}", k),
            Slot::Local(k) => format!("l#{}", k),
            Slot::Global(k) => format!("g#{}", k),
        }
    }
}

/// A scalar expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpaExpr {
    Const(i64),
    Read(Slot),
    Add(Box<IpaExpr>, Box<IpaExpr>),
    Sub(Box<IpaExpr>, Box<IpaExpr>),
    Mul(Box<IpaExpr>, Box<IpaExpr>),
    /// `&slot`.
    AddrOf(Slot),
    /// `&f` — the address of a procedure (a function value).
    FnAddr(ProcId),
    /// A null / undefined value.
    Null,
}

impl IpaExpr {
    pub fn constant(c: i64) -> IpaExpr {
        IpaExpr::Const(c)
    }

    pub fn read(s: Slot) -> IpaExpr {
        IpaExpr::Read(s)
    }

    /// Append every slot this expression reads.
    pub fn slots_read(&self, out: &mut Vec<Slot>) {
        match self {
            IpaExpr::Read(s) => out.push(*s),
            IpaExpr::Add(a, b) | IpaExpr::Sub(a, b) | IpaExpr::Mul(a, b) => {
                a.slots_read(out);
                b.slots_read(out);
            }
            IpaExpr::AddrOf(_) | IpaExpr::FnAddr(_) | IpaExpr::Const(_) | IpaExpr::Null => {}
        }
    }

    /// The constant this expression evaluates to, if it is one.
    pub fn as_const(&self) -> Option<i64> {
        match self {
            IpaExpr::Const(c) => Some(*c),
            IpaExpr::Add(a, b) => Some(a.as_const()?.checked_add(b.as_const()?)?),
            IpaExpr::Sub(a, b) => Some(a.as_const()?.checked_sub(b.as_const()?)?),
            IpaExpr::Mul(a, b) => Some(a.as_const()?.checked_mul(b.as_const()?)?),
            _ => None,
        }
    }
}

/// Which way an I/O statement moves data (Wolfe §8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IoKind {
    Read,
    Write,
}

/// The callee of a call: a known procedure, or a computed function value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Callee {
    Direct(ProcId),
    Indirect(IpaExpr),
}

/// A call statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub callee: Callee,
    pub args: Vec<IpaExpr>,
    /// The slot the result is written to, if the result is used.
    pub ret: Option<Slot>,
}

/// A statement in a procedure body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stmt", rename_all = "snake_case")]
pub enum IpaStmt {
    Assign { target: Slot, value: IpaExpr },
    Call(Call),
    Io { channel: String, kind: IoKind },
}

/// One procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proc {
    pub name: String,
    /// Formal parameter names, in order. `Slot::Param(k)` names `params[k]`.
    pub params: Vec<String>,
    /// Local variable names, in order. `Slot::Local(k)` names `locals[k]`.
    pub locals: Vec<String>,
    pub stmts: Vec<IpaStmt>,
    /// The return expression, if any.
    pub ret: Option<IpaExpr>,
}

impl Proc {
    pub fn new(name: impl Into<String>, params: Vec<String>, locals: Vec<String>) -> Proc {
        Proc { name: name.into(), params, locals, stmts: Vec::new(), ret: None }
    }

    /// Symbol name for a slot, for diagnostics.
    pub fn slot_name(&self, s: Slot, prog_globals: &[String]) -> String {
        match s {
            Slot::Param(k) => self.params.get(k).cloned().unwrap_or_else(|| format!("p#{}", k)),
            Slot::Local(k) => self.locals.get(k).cloned().unwrap_or_else(|| format!("l#{}", k)),
            Slot::Global(k) => prog_globals.get(k).cloned().unwrap_or_else(|| format!("g#{}", k)),
        }
    }
}

/// A whole program: its globals, its procedures, and an entry point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpaProgram {
    pub globals: Vec<String>,
    pub procs: Vec<Proc>,
    pub entry: ProcId,
}

impl IpaProgram {
    pub fn new(globals: Vec<String>, procs: Vec<Proc>, entry: ProcId) -> IpaProgram {
        IpaProgram { globals, procs, entry }
    }

    pub fn proc(&self, id: ProcId) -> Option<&Proc> {
        self.procs.get(id.index())
    }

    /// The `ProcId` of the procedure named `name`, if any.
    pub fn proc_id(&self, name: &str) -> Option<ProcId> {
        self.procs.iter().position(|p| p.name == name).map(ProcId)
    }

    /// Every proc id in declaration order.
    pub fn ids(&self) -> impl Iterator<Item = ProcId> + '_ {
        (0..self.procs.len()).map(ProcId)
    }

    /// Structural checks: a non-empty program with an in-range entry, and every
    /// `Slot::Param`/`Slot::Local` index in range for its procedure.
    pub fn validate(&self) -> Result<(), String> {
        if self.procs.is_empty() {
            return Err("program has no procedures".into());
        }
        if self.entry.index() >= self.procs.len() {
            return Err(format!("entry {} out of range", self.entry.index()));
        }
        for (i, p) in self.procs.iter().enumerate() {
            let check = |s: Slot| -> Result<(), String> {
                match s {
                    Slot::Param(k) if k >= p.params.len() => {
                        Err(format!("proc {} reads param {} out of range", i, k))
                    }
                    Slot::Local(k) if k >= p.locals.len() => {
                        Err(format!("proc {} reads local {} out of range", i, k))
                    }
                    Slot::Global(k) if k >= self.globals.len() => {
                        Err(format!("proc {} reads global {} out of range", i, k))
                    }
                    _ => Ok(()),
                }
            };
            for stmt in &p.stmts {
                match stmt {
                    IpaStmt::Assign { target, value } => {
                        check(*target)?;
                        let mut slots = Vec::new();
                        value.slots_read(&mut slots);
                        for s in slots {
                            check(s)?;
                        }
                    }
                    IpaStmt::Call(c) => {
                        if let Callee::Direct(callee) = c.callee {
                            if callee.index() >= self.procs.len() {
                                return Err(format!(
                                    "proc {} calls proc {} out of range",
                                    i,
                                    callee.index()
                                ));
                            }
                        }
                        if let Some(r) = c.ret {
                            check(r)?;
                        }
                        for a in &c.args {
                            let mut slots = Vec::new();
                            a.slots_read(&mut slots);
                            for s in slots {
                                check(s)?;
                            }
                        }
                    }
                    IpaStmt::Io { .. } => {}
                }
            }
        }
        Ok(())
    }
}

// Simplified call graph types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraph {
    pub edges: Vec<(usize, usize)>, // (caller, callee)
}

impl CallGraph {
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }
}

/// Build call graph from program
pub fn build_call_graph(program: &IpaProgram) -> CallGraph {
    let mut edges = Vec::new();
    for (caller_id, caller) in program.procs.iter().enumerate() {
        for stmt in &caller.stmts {
            if let IpaStmt::Call(call) = stmt {
                if let Callee::Direct(callee) = call.callee {
                    edges.push((caller_id, callee.index()));
                }
            }
        }
    }
    CallGraph { edges }
}

/// Tarjan's SCC algorithm
pub fn tarjan_scc(graph: &CallGraph) -> Vec<Vec<usize>> {
    let n = graph.edges.iter().map(|(a, b)| *a.max(b)).max().unwrap_or(0) + 1;
    let mut adj = vec![Vec::new(); n];
    for (u, v) in &graph.edges {
        adj[*u].push(*v);
    }

    let mut index = 0;
    let mut indices = vec![-1; n];
    let mut lowlink = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut sccs = Vec::new();

    fn strongconnect(
        v: usize,
        adj: &[Vec<usize>],
        index: &mut i32,
        indices: &mut [i32],
        lowlink: &mut [i32],
        on_stack: &mut [bool],
        stack: &mut Vec<usize>,
        sccs: &mut Vec<Vec<usize>>,
    ) {
        indices[v] = *index;
        lowlink[v] = *index;
        *index += 1;
        stack.push(v);
        on_stack[v] = true;

        for &w in &adj[v] {
            if indices[w] == -1 {
                strongconnect(w, adj, index, indices, lowlink, on_stack, stack, sccs);
                lowlink[v] = lowlink[v].min(lowlink[w]);
            } else if on_stack[w] {
                lowlink[v] = lowlink[v].min(indices[w]);
            }
        }

        if lowlink[v] == indices[v] {
            let mut scc = Vec::new();
            loop {
                let w = stack.pop().unwrap();
                on_stack[w] = false;
                scc.push(w);
                if w == v { break; }
            }
            sccs.push(scc);
        }
    }

    for v in 0..n {
        if indices[v] == -1 {
            strongconnect(v, &adj, &mut index, &mut indices, &mut lowlink, &mut on_stack, &mut stack, &mut sccs);
        }
    }

    sccs
}