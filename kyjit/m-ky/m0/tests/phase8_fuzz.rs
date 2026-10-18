//! Phase 8 differential fuzz — interprocedural analysis.
//!
//! Wolfe's §8.11 fuzz strategy, implemented as a comparison against an oracle
//! that shares no code with the analyses: a concrete interpreter that *runs* the
//! generated program, tracks storage cells by identity, and reports which
//! locations it actually wrote and read.
//!
//! | Check | Analysis | Oracle |
//! | --- | --- | --- |
//! | MOD (§8.5.2) | `mod_analysis::analyze` | locations written during a concrete run |
//! | summaries (§8.7) | `summaries::compute` | locations read/written during a run |
//! | purity (§8.4) | `pure::classify` | a run that writes a global, does I/O, or writes through an argument must not be called `Pure` |
//! | constprop (§8.5.3) | `constprop::analyze` | the concrete value each formal is entered with |
//!
//! Every relation is **soundness**, not equality: a static answer may be less
//! precise than the interpreter's observation, never more optimistic. That is the
//! only comparison a fixpoint and a single execution can support, and it is
//! asserted in both directions — a claim the run contradicts is a violation.
//!
//! Programs are generated as a DAG (a call may only target a higher-indexed
//! procedure), so the interpreter terminates without a depth cap. Storage
//! bindings follow the model's own rule: an actual that is a plain `Read`/
//! `AddrOf` of a slot aliases the caller's cell (call-by-reference, which is what
//! the binding graph encodes); anything else gets a private cell.
//!
//! Effects are attributed *by binding*, never by comparing a cell's value before
//! and after a frame runs. When a callee writes through a formal, the fact is
//! pushed up through the actual that bound it, in the caller's own naming: `&g`
//! makes it a global write, `&x` a write of the caller's formal, `&t` a write of
//! its local, and a literal binds nothing so the write stops there. A diff-based
//! oracle would over-attribute — two actuals can alias one cell (`&x, &x`), so a
//! callee that wrote only its second formal would be reported as having modified
//! the first too — and it would also disagree with every analysis it checks, which
//! all name slots, never cells.
//!
//! Each driver takes a `bug` flag. Under `bug` the expected answer is computed
//! without interprocedural propagation — Wolfe's "skip SCC propagation" seed —
//! and the fuzz must report violations. An oracle that cannot fail is not an
//! oracle, so that is asserted, not assumed.

use std::collections::{BTreeMap, BTreeSet};

use m0::ir::purity::Purity;
use m0::ipa::callgraph::{build, CallGraph};
use m0::ipa::certificate::{check_ipa_claim, IpaClaim};
use m0::ipa::constprop::{
    analyze as analyze_constprop, apply_jump, jump_of_expr, meet, AbstractValue,
};
use m0::ipa::compose::{compose_into, map_symbol};
use m0::ipa::context::{refine as refine_context, ArgShape, MAX_CONTEXTS};
use m0::ipa::io_effects::{io_ops, summaries as io_summaries, IoOp};
use m0::ipa::mod_analysis::{analyze as analyze_mod, ModAnalysis};
use m0::ipa::program::{Call, Callee, IpaExpr, IpaProgram, IpaStmt, IoKind, Proc, ProcId, Slot};
use m0::ipa::pure::classify as classify_purity;
use m0::ipa::summaries::compute as compute_summaries;

/// A deterministic linear-congruential generator, so a failing case is
/// reproducible from its seed alone.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// A value in `[0, n)`.
    fn below(&mut self, n: i64) -> i64 {
        (self.next() >> 33) as i64 % n
    }
    fn one_of(&mut self, n: usize) -> usize {
        self.below(n as i64) as usize
    }
}

/// `&slot` — the model's aliasing actual.
fn addr(s: Slot) -> IpaExpr {
    IpaExpr::AddrOf(s)
}

/// An expression over the slots of procedure `i`, sized so that every slot it
/// names exists in that procedure.
fn gen_expr(rng: &mut Lcg, nparams: usize, nlocals: usize, i: usize, n: usize) -> IpaExpr {
    match rng.one_of(10) {
        0 | 1 | 2 => IpaExpr::Const(rng.below(5)),
        3 if nparams > 0 => IpaExpr::Read(Slot::Param(rng.one_of(nparams))),
        4 if nlocals > 0 => IpaExpr::Read(Slot::Local(rng.one_of(nlocals))),
        5 => IpaExpr::Read(Slot::Global(rng.one_of(2))),
        6 => IpaExpr::Add(
            Box::new(IpaExpr::Const(rng.below(4))),
            Box::new(IpaExpr::Const(rng.below(4))),
        ),
        7 if i + 1 < n => IpaExpr::FnAddr(ProcId(i + 1 + rng.one_of(n - i - 1))),
        _ => IpaExpr::Const(0),
    }
}

/// A writable target for procedure `i`: a global or one of its own slots.
fn gen_target(rng: &mut Lcg, nparams: usize, nlocals: usize) -> Slot {
    match rng.one_of(4) {
        0 => Slot::Global(rng.one_of(2)),
        1 if nlocals > 0 => Slot::Local(rng.one_of(nlocals)),
        2 if nparams > 0 => Slot::Param(rng.one_of(nparams)),
        _ => Slot::Global(rng.one_of(2)),
    }
}

/// A slot usable as an actual's alias target in procedure `i`.
fn gen_cell(rng: &mut Lcg, nparams: usize, nlocals: usize) -> Slot {
    match rng.one_of(3) {
        0 if nparams > 0 => Slot::Param(rng.one_of(nparams)),
        1 if nlocals > 0 => Slot::Local(rng.one_of(nlocals)),
        _ => Slot::Global(rng.one_of(2)),
    }
}

/// A random program: three to five procedures, `main` (`ProcId(0)`) first and
/// argument free, calls only to higher-numbered procedures (so the call graph is
/// a DAG and the interpreter cannot recurse), bodies of one to three statements.
fn gen_program(rng: &mut Lcg) -> IpaProgram {
    let n = 3 + rng.one_of(3);
    let mut procs: Vec<Proc> = Vec::new();
    for i in 0..n {
        let nparams = if i == 0 { 0 } else { 1 + rng.one_of(2) };
        let nlocals = rng.one_of(3);
        procs.push(Proc::new(
            format!("p{}", i),
            (0..nparams).map(|k| format!("x{}", k)).collect(),
            (0..nlocals).map(|k| format!("v{}", k)).collect(),
        ));
    }
    for i in 0..n {
        let nparams = procs[i].params.len();
        let nlocals = procs[i].locals.len();
        let mut stmts = Vec::new();
        for _ in 0..(1 + rng.one_of(3)) {
            match rng.one_of(10) {
                0..=4 => stmts.push(IpaStmt::Assign {
                    target: gen_target(rng, nparams, nlocals),
                    value: gen_expr(rng, nparams, nlocals, i, n),
                }),
                5..=8 if i + 1 < n => {
                    let q = i + 1 + rng.one_of(n - i - 1);
                    let args: Vec<IpaExpr> = (0..procs[q].params.len())
                        .map(|_| match rng.one_of(6) {
                            0..=2 => addr(gen_cell(rng, nparams, nlocals)),
                            3..=4 => IpaExpr::Const(rng.below(5)),
                            _ => gen_expr(rng, nparams, nlocals, i, n),
                        })
                        .collect();
                    let callee = if rng.one_of(8) == 0 {
                        Callee::Indirect(IpaExpr::FnAddr(ProcId(q)))
                    } else {
                        Callee::Direct(ProcId(q))
                    };
                    stmts.push(IpaStmt::Call(Call { callee, args, ret: None }));
                }
                _ => stmts.push(IpaStmt::Io {
                    channel: ["w", "r", "e"][rng.one_of(3)].to_string(),
                    kind: if rng.one_of(2) == 0 { IoKind::Write } else { IoKind::Read },
                }),
            }
        }
        procs[i].stmts = stmts;
    }
    let prog = IpaProgram::new(vec!["g0".into(), "g1".into()], procs, ProcId(0));
    assert_eq!(prog.validate(), Ok(()), "the generator produced a malformed program");
    prog
}

// ---------------------------------------------------------------------------
// The oracle: a concrete interpreter
// ---------------------------------------------------------------------------

/// A storage location as the interpreter sees it. Globals are shared; every
/// local is a fresh cell, and a formal bound to a caller's slot *is* that
/// caller's cell — call-by-reference, the same aliasing the binding graph
/// encodes. An actual that is not a plain slot read/`AddrOf` gets a private
/// cell, because the analysis cannot name it either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Cell {
    Global(usize),
    Boxed(usize),
}

/// Everything one procedure's *dynamic extent* (its own body plus every callee
/// it ran) revealed about side effects.
///
/// Effects are recorded by *write-through-binding*: when a callee writes through
/// a formal, the actual that bound it says which of the caller's slots changed,
/// and the fact is attributed there and travels upward. A raw before/after diff
/// of the cells a frame names would over-attribute — two actuals can alias the
/// same cell (`&x, &x`), and a callee that wrote only its second formal would
/// then be reported as having modified the first one too.
#[derive(Debug, Clone, Default)]
struct Extent {
    /// Globals written anywhere below this frame.
    globals: BTreeSet<usize>,
    /// Some frame in this extent performed I/O.
    io: bool,
    /// Every I/O channel touched anywhere below this frame.
    channels: BTreeSet<String>,
    /// The formals of *this* frame whose cell was written — directly or through
    /// a callee's binding — i.e. this procedure modified its caller's binding.
    wrote_params: BTreeSet<usize>,
    /// This frame's locals written directly or through a callee's binding.
    wrote_locals: BTreeSet<usize>,
    /// A global was read anywhere below this frame.
    read_global: bool,
}

impl Extent {
    /// Merge a callee's extent into this frame's, attributing each of the
    /// callee's effects through the actuals that bound them. `writes` maps a
    /// callee formal to the caller slot that actual denoted. Globals, I/O and
    /// global reads travel as they are; a formal the callee *wrote* becomes a
    /// write of the caller's own slot — global, formal, or local — and a formal
    /// bound to a literal writes nothing of this frame's when its actual owns no
    /// slot at all.
    ///
    /// The gate on `other.wrote_params` is the whole point: binding alone is not
    /// an effect. A callee that received `&g` and never assigned its formal did
    /// not touch `g`, and reporting it as modified would send the MOD analysis
    /// chasing writes that never happen — the check would then be measuring the
    /// oracle's imagination rather than the program's behaviour.
    fn merge_callee_through(&mut self, other: &Extent, writes: &[(usize, Slot)]) {
        self.globals.extend(other.globals.iter().copied());
        self.io |= other.io;
        self.channels.extend(other.channels.iter().cloned());
        self.read_global |= other.read_global;
        for (k, actual) in writes {
            if !other.wrote_params.contains(k) {
                continue;
            }
            match actual {
                Slot::Global(g) => {
                    self.globals.insert(*g);
                }
                Slot::Param(j) => {
                    self.wrote_params.insert(*j);
                }
                Slot::Local(l) => {
                    self.wrote_locals.insert(*l);
                }
            }
        }
    }

    /// Merge one invocation's extent into the whole-run record for a procedure.
    /// Here every field accumulates: two visits, one of which modified argument
    /// 2, mean the procedure can modify argument 2.
    fn absorb(&mut self, other: &Extent) {
        self.globals.extend(other.globals.iter().copied());
        self.io |= other.io;
        self.channels.extend(other.channels.iter().cloned());
        self.read_global |= other.read_global;
        self.wrote_params.extend(other.wrote_params.iter().copied());
        self.wrote_locals.extend(other.wrote_locals.iter().copied());
    }
}

/// What one execution of the program reveals.
#[derive(Debug, Default)]
struct Trace {
    /// `(proc, slot)` written in that procedure's own naming, including writes
    /// a callee performed through a binding to one of its formals, attributed
    /// through write-through-binding up to this frame's own slots.
    wrote: BTreeSet<(ProcId, Slot)>,
    /// `(proc, slot)` read in that procedure's own naming.
    read: BTreeSet<(ProcId, Slot)>,
    /// The procedures whose bodies actually ran.
    executed: BTreeSet<ProcId>,
    /// `(caller, callee)` for every call the run actually made.
    calls: BTreeSet<(ProcId, ProcId)>,
    /// Globals written anywhere.
    globals_written: BTreeSet<usize>,
    /// Procedures that performed I/O.
    io: BTreeSet<ProcId>,
    /// Writes observed *through a formal* — the cases propagation matters for.
    through_formal: usize,
    /// The merged effect of each procedure's dynamic extent over the whole run.
    extents: BTreeMap<ProcId, Extent>,
    /// `(callee, formal) → the values the callee's cell actually held on entry`,
    /// over every call the run made. `Const` propagation claims a formal always
    /// holds one value; this is what a run can contradict.
    passed: BTreeMap<(ProcId, usize), BTreeSet<i64>>,
}

/// The interpreter: runs `prog` from its entry, recording what it touches.
struct Interp<'a> {
    prog: &'a IpaProgram,
    vals: BTreeMap<Cell, i64>,
    fresh: usize,
    trace: Trace,
}

impl<'a> Interp<'a> {
    fn new(prog: &'a IpaProgram) -> Interp<'a> {
        let mut vals = BTreeMap::new();
        for k in 0..prog.globals.len() {
            vals.insert(Cell::Global(k), 0);
        }
        Interp { prog, vals, fresh: 0, trace: Trace::default() }
    }

    fn alloc(&mut self) -> Cell {
        self.alloc_with(0)
    }

    /// A fresh private cell, initialised to `v`.
    fn alloc_with(&mut self, v: i64) -> Cell {
        self.fresh += 1;
        let c = Cell::Boxed(self.fresh);
        self.vals.insert(c, v);
        c
    }

    fn run(&mut self) {
        self.exec(ProcId(0), &[]);
    }

    /// The cell a slot names, given this frame's formal and local cells.
    fn locate(&self, params: &[Cell], locals: &[Cell], s: Slot) -> Cell {
        match s {
            Slot::Global(k) => Cell::Global(k),
            Slot::Param(j) => params.get(j).copied().unwrap_or(Cell::Global(0)),
            Slot::Local(k) => locals.get(k).copied().unwrap_or(Cell::Global(0)),
        }
    }

    fn value_of(&mut self, e: &IpaExpr, params: &[Cell], locals: &[Cell], proc: ProcId) -> i64 {
        match e {
            IpaExpr::Const(c) => *c,
            // Addresses and function values are opaque integers to this model.
            IpaExpr::Null | IpaExpr::FnAddr(_) | IpaExpr::AddrOf(_) => 0,
            IpaExpr::Read(s) => {
                self.trace.read.insert((proc, *s));
                let c = self.locate(params, locals, *s);
                *self.vals.get(&c).unwrap_or(&0)
            }
            IpaExpr::Add(a, b) => {
                let x = self.value_of(a, params, locals, proc);
                let y = self.value_of(b, params, locals, proc);
                x.wrapping_add(y)
            }
            IpaExpr::Sub(a, b) => {
                let x = self.value_of(a, params, locals, proc);
                let y = self.value_of(b, params, locals, proc);
                x.wrapping_sub(y)
            }
            IpaExpr::Mul(a, b) => {
                let x = self.value_of(a, params, locals, proc);
                let y = self.value_of(b, params, locals, proc);
                x.wrapping_mul(y)
            }
        }
    }

    /// Execute `proc` with `args` bound to its formals, in caller cells, and
    /// report the effects of this frame's dynamic extent.
    ///
    /// Attribution is *by binding*, never by value diff: a write lands on the
    /// slot the writing frame names, and each callee effect is re-expressed in
    /// this frame's naming through the actual that produced it. Two frames can
    /// then never disagree about who was written, and aliasing between actuals
    /// (`&x, &x`) cannot make an untouched formal look modified.
    fn exec(&mut self, proc: ProcId, args: &[Cell]) -> Extent {
        let p = self.prog.proc(proc).expect("a generated procedure exists");
        let nparams = p.params.len();
        let locals: Vec<Cell> = (0..p.locals.len()).map(|_| self.alloc()).collect();
        // The cells this frame can name, in slot order: formals, then locals.
        let cells: Vec<Cell> = (0..nparams + locals.len())
            .map(|i| if i < nparams { args.get(i).copied().unwrap_or(Cell::Global(0)) } else { locals[i - nparams] })
            .collect();
        self.trace.executed.insert(proc);
        let mut ext = Extent::default();

        for stmt in &p.stmts {
            match stmt {
                IpaStmt::Assign { target, value } => {
                    let v = self.value_of(value, args, &locals, proc);
                    // A statement that reads a global makes it so whether or not
                    // the value it yields differs; `extent` must know.
                    if mentions_global_read(value) {
                        ext.read_global = true;
                    }
                    self.record_write(target, &cells, &mut ext);
                    let cell = self.locate(args, &locals, *target);
                    self.vals.insert(cell, v);
                }
                IpaStmt::Io { channel, .. } => {
                    self.trace.io.insert(proc);
                    ext.io = true;
                    ext.channels.insert(channel.clone());
                }
                IpaStmt::Call(c) => {
                    let q = match &c.callee {
                        Callee::Direct(q) => *q,
                        Callee::Indirect(IpaExpr::FnAddr(q)) => *q,
                        // A callee the model cannot resolve is not executed; the
                        // call graph marks it `unresolved` for the same reason,
                        // and no analysis may assume the edge set is exhaustive.
                        Callee::Indirect(_) => continue,
                    };
                    let nq = self.prog.proc(q).map(|x| x.params.len()).unwrap_or(0);
                    // The caller slot each actual denotes, per callee formal: the
                    // map the callee's effects get pushed through. A formal with
                    // no entry was bound to a literal or a computed value, so
                    // nothing of this frame's can be written through it.
                    let mut writes: Vec<(usize, Slot)> = Vec::new();
                    let mut bound: Vec<Cell> = Vec::with_capacity(nq);
                    for j in 0..nq {
                        let a = match c.args.get(j) {
                            Some(x) => x,
                            None => {
                                bound.push(self.alloc_with(0));
                                continue;
                            }
                        };
                        let cell = match a {
                            IpaExpr::Read(s) | IpaExpr::AddrOf(s) => {
                                // Call-by-reference: the callee's formal *is* this
                                // slot's cell, so a write through it lands on the
                                // slot the *actual names*. That is the caller's own
                                // naming of the effect, and it is exactly the edge
                                // the MOD binding graph builds — which is why the
                                // slot is taken from the actual rather than looked
                                // up from the cell: a formal bound to `&g` and a
                                // global are the same cell, but the analysis names
                                // it `g`, and a chain of two such bindings must not
                                // turn a global into somebody's parameter.
                                writes.push((j, *s));
                                let c0 = self.locate(args, &locals, *s);
                                let v = *self.vals.get(&c0).unwrap_or(&0);
                                self.trace.passed.entry((q, j)).or_default().insert(v);
                                if let IpaExpr::Read(_) = a {
                                    self.trace.read.insert((proc, *s));
                                    if matches!(s, Slot::Global(_)) {
                                        ext.read_global = true;
                                    }
                                }
                                c0
                            }
                            // Call-by-value: a private cell holding the actual's
                            // value. A literal is the case where the analysis can
                            // name the value too.
                            _ => {
                                let v = self.value_of(a, args, &locals, proc);
                                self.trace.passed.entry((q, j)).or_default().insert(v);
                                self.alloc_with(v)
                            }
                        };
                        bound.push(cell);
                    }
                    let sub = self.exec(q, &bound);
                    self.trace.calls.insert((proc, q));
                    ext.merge_callee_through(&sub, &writes);
                }
            }
        }

        // Everything this extent revealed, in this procedure's own naming. The
        // fields are already in that naming — callee effects arrived translated
        // through the actuals — so `wrote` is read straight off them, and the two
        // can never disagree.
        for k in &ext.globals {
            self.trace.globals_written.insert(*k);
            self.trace.wrote.insert((proc, Slot::Global(*k)));
        }
        for j in &ext.wrote_params {
            self.trace.wrote.insert((proc, Slot::Param(*j)));
        }
        for l in &ext.wrote_locals {
            self.trace.wrote.insert((proc, Slot::Local(*l)));
        }

        self.trace.extents.entry(proc).or_default().absorb(&ext);
        ext
    }

    /// Record a direct write of `target` by this frame.
    ///
    /// A write of a formal is a write of *this* frame's formal even when the
    /// caller bound it to a global: [`crate::ipa::pure`] judges that way — the
    /// callee stays `PureSubroutine` naming the formal, and the caller that handed
    /// over the global is the one that becomes `Impure`. The shared store did
    /// change, though, so `globals_written`, which is a whole-run fact about the
    /// store that MOD must cover for every procedure, notes it here too.
    fn record_write(&mut self, target: &Slot, cells: &[Cell], ext: &mut Extent) {
        match target {
            Slot::Global(k) => {
                self.trace.globals_written.insert(*k);
                ext.globals.insert(*k);
            }
            Slot::Param(j) => {
                self.trace.through_formal += 1;
                ext.wrote_params.insert(*j);
                if let Some(Cell::Global(k)) = cells.get(*j).copied() {
                    self.trace.globals_written.insert(k);
                }
            }
            Slot::Local(l) => {
                ext.wrote_locals.insert(*l);
            }
        }
    }
}

/// Run the oracle on `prog`.
fn interpret(prog: &IpaProgram) -> Trace {
    let mut it = Interp::new(prog);
    it.run();
    it.trace
}

/// Does this expression read a global? `slots_read` deliberately ignores
/// `AddrOf`, which is exactly what `pure::reads_global` uses, so the oracle and
/// the analysis see the same thing here — and a pointer actual is *not* a read.
fn mentions_global_read(e: &IpaExpr) -> bool {
    let mut slots = Vec::new();
    e.slots_read(&mut slots);
    slots.iter().any(|s| matches!(s, Slot::Global(_)))
}

// ---------------------------------------------------------------------------
// The comparisons
// ---------------------------------------------------------------------------

/// `modified_globals` must agree with the union of the per-procedure sets.
fn globals_of(a: &ModAnalysis) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for set in a.mod_sets.values() {
        for s in set {
            if let Slot::Global(k) = s {
                out.insert(*k);
            }
        }
    }
    out
}

/// The MOD analysis (§8.5.2) must cover every write the interpreter observed in
/// that procedure's own naming — including one a callee performed through a
/// binding to a formal. `bug` runs the analysis with the binding edges removed
/// (Wolfe's "skip the propagation" seed), which is what makes the check
/// non-vacuous rather than what makes it correct.
fn mod_fuzz(seed: u64, cases: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let cg = build(&prog);
        // The seeded bug: run the real analysis over a program whose calls bind
        // nothing, so a write a callee performs through a formal never reaches
        // the caller's MOD set.
        let (subject, graph) = if bug {
            (opaque_args(&prog), build(&opaque_args(&prog)))
        } else {
            (prog.clone(), cg.clone())
        };
        let a = analyze_mod(&subject, &graph);
        for p in prog.ids() {
            let expect: BTreeSet<Slot> =
                t.wrote.iter().filter(|(q, _)| *q == p).map(|(_, s)| *s).collect();
            let got = a.mod_sets.get(&p).cloned().unwrap_or_default();
            if !expect.is_subset(&got) {
                violations += 1;
            }
        }
        // Globals are shared: one written anywhere belongs to every procedure.
        for k in &t.globals_written {
            if prog.ids().any(|p| !a.mod_sets[&p].contains(&Slot::Global(*k))) {
                violations += 1;
            }
        }
        // The analysis's own two views must agree with each other.
        if a.modified_globals != globals_of(&a) {
            violations += 1;
        }
    }
    violations
}

/// Purity (§8.4) must never call a procedure `Pure` when its executed extent
/// wrote or read a global, performed I/O, or changed one of its own arguments,
/// nor `PureSubroutine` while its extent wrote a global or did I/O — and a
/// `PureSubroutine` must name every formal the run actually changed.
///
/// `bug` substitutes the seeded variant, [`purity_local_only`]: each procedure
/// judged from its own body, callees ignored. Guessing `Pure` too generously is
/// the direction of error that matters, because it is what lets a caller delete a
/// call; the interpreter is what catches it.
fn purity_fuzz(seed: u64, cases: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed + 1);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let cg = build(&prog);
        let cls = if bug {
            purity_local_only(&prog)
        } else {
            classify_purity(&prog, &cg)
        };
        for p in prog.ids() {
            let purity = &cls[p.index()];
            let ext = t.extents.get(&p).cloned().unwrap_or_default();
            let must_be_impure = !ext.globals.is_empty() || ext.io || ext.read_global;
            if matches!(purity, Purity::Pure) && (must_be_impure || !ext.wrote_params.is_empty()) {
                violations += 1;
            }
            if matches!(purity, Purity::PureSubroutine { .. }) && must_be_impure {
                violations += 1;
            }
            if let Purity::PureSubroutine { modified } = purity {
                // A `PureSubroutine` may only name a formal this procedure has…
                let nparams = prog.proc(p).map(|x| x.params.len()).unwrap_or(0);
                if modified.iter().any(|k| *k >= nparams) {
                    violations += 1;
                }
                // …and must name every one the run actually changed.
                let claimed: BTreeSet<usize> = modified.iter().copied().collect();
                if !ext.wrote_params.is_subset(&claimed) {
                    violations += 1;
                }
            }
        }
    }
    violations
}

/// The seed: [`crate::ipa::pure::classify`] without the fixpoint. Same local
/// rules, no callee effects folded in — the "a callee's side effects do not make
/// the caller impure" mistake.
fn purity_local_only(prog: &IpaProgram) -> Vec<Purity> {
    prog.ids()
        .map(|p| {
            let proc = prog.proc(p).expect("a generated procedure exists");
            let mut impure = false;
            let mut modified: BTreeSet<usize> = BTreeSet::new();
            for stmt in &proc.stmts {
                match stmt {
                    IpaStmt::Io { .. } => impure = true,
                    IpaStmt::Assign { target, value } => {
                        if mentions_global_read(value) || matches!(target, Slot::Global(_)) {
                            impure = true;
                        }
                        if let Slot::Param(k) = target {
                            modified.insert(*k);
                        }
                    }
                    IpaStmt::Call(c) => {
                        // Passing a global *by read* is this procedure's own
                        // effect; what its callee then does is not — that is
                        // precisely the seeded omission.
                        for a in &c.args {
                            if mentions_global_read(a) {
                                impure = true;
                            }
                        }
                        if let Some(Slot::Global(_)) = c.ret {
                            impure = true;
                        }
                        if let Some(Slot::Param(k)) = c.ret {
                            modified.insert(k);
                        }
                    }
                }
            }
            if impure {
                Purity::Impure
            } else if modified.is_empty() {
                Purity::Pure
            } else {
                Purity::PureSubroutine { modified: modified.into_iter().collect() }
            }
        })
        .collect()
}

/// The same program with every slot-naming argument replaced by a literal, so no
/// call binds anything. This is the MOD seed: the analysis is still
/// [`crate::ipa::mod_analysis::analyze`], but the binding edges it would build
/// have nowhere to come from, and the writes that travel through them vanish.
fn opaque_args(prog: &IpaProgram) -> IpaProgram {
    let mut out = prog.clone();
    for p in &mut out.procs {
        for stmt in &mut p.stmts {
            if let IpaStmt::Call(c) = stmt {
                for a in &mut c.args {
                    if matches!(a, IpaExpr::Read(_) | IpaExpr::AddrOf(_)) {
                        *a = IpaExpr::Const(0);
                    }
                }
            }
        }
    }
    assert_eq!(out.validate(), Ok(()));
    out
}

/// The call graph's own seed: only the textual, direct edges, so every call the
/// run makes through a resolved `&f` value is missing.
fn textual_only(prog: &IpaProgram) -> CallGraph {
    let mut g = CallGraph::default();
    for p in prog.ids() {
        let proc = prog.proc(p).expect("a generated procedure exists");
        for (si, stmt) in proc.stmts.iter().enumerate() {
            if let IpaStmt::Call(c) = stmt {
                if let Callee::Direct(q) = &c.callee {
                    g.edges.entry(p).or_default().insert(*q);
                    g.call_sites.entry((p, *q)).or_default().push(si);
                }
            }
        }
    }
    g
}

/// Bottom-up summaries (§8.7): every write and read the run observed in a
/// procedure's own naming must be in that procedure's summary, every procedure it
/// actually called must be in its transitive closure, and every channel it
/// touched must appear in its I/O summary. A summary's purity must also agree
/// with §8.4's, because the summary is the presentation consumers read.
///
/// `bug` analyses a program whose call sites bind nothing (the same
/// [`opaque_args`] seed as [`mod_fuzz`]), so an effect reached through a call
/// site is missing and the coverage checks fire.
fn summary_fuzz(seed: u64, cases: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed + 2);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let subject = if bug { opaque_args(&prog) } else { prog.clone() };
        let cg = build(&subject);
        let s = compute_summaries(&subject, &cg);
        let cls = classify_purity(&subject, &cg);
        for p in prog.ids() {
            let sum = match s.get(&p) {
                Some(x) => x,
                None => {
                    violations += 1;
                    continue;
                }
            };
            // Internal consistency: one analysis, two presentations.
            if sum.purity != cls[p.index()] || sum.proc != p {
                violations += 1;
            }
            for (_, slot) in t.wrote.iter().filter(|(q, _)| *q == p) {
                if !sum.write_set.contains(slot) {
                    violations += 1;
                }
            }
            for (_, slot) in t.read.iter().filter(|(q, _)| *q == p) {
                if !sum.read_set.contains(slot) {
                    violations += 1;
                }
            }
            for (_, callee) in t.calls.iter().filter(|(c, _)| *c == p) {
                if !sum.callees.contains(callee) {
                    violations += 1;
                }
                // A global a callee may write is a global the caller may write:
                // globals are shared, and §8.9's composition is what folds it.
                for g in &s[callee].write_set {
                    if matches!(g, Slot::Global(_)) && !sum.write_set.contains(g) {
                        violations += 1;
                    }
                }
            }
            // Every channel the run touched below this procedure must be in its
            // transitive I/O summary.
            let ext = t.extents.get(&p).cloned().unwrap_or_default();
            for ch in &ext.channels {
                if !sum.io.touches(ch) {
                    violations += 1;
                }
            }
        }
    }
    violations
}


/// Constant propagation (§8.6), checked in both directions.
///
/// *Soundness*: when the analysis says a formal is `Const(c)`, every value the
/// run actually handed that formal must be `c`. The interpreter records the value
/// in the callee's cell on entry, so a single differing execution refutes it.
///
/// *Precision*: when every call site in the program passes the same literal to a
/// formal, the fixpoint must know that literal. This is the half a wrongly-seeded
/// lattice escapes — a `Top` that never improves looks perfectly sound.
///
/// *Density*: the result map is total over `(proc, formal)`, so a missing key is
/// a violation, not an absence to be re-interpreted.
///
/// `bug` feeds the analysis a graph with only the textual edges, so a formal
/// reached only through a resolved `&f` call site is never seeded and stays `Top`.
fn constprop_fuzz(seed: u64, cases: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed + 3);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        // A call through a *formal* reaches a callee only the arg-flow triplets
        // can name, so "every call site passes literal c" is a claim this driver
        // cannot establish for that callee's formal. Generated programs use
        // `&f` callees, so the guard rarely trips; it is here so the precision
        // half can never be wrong if the generator grows.
        if any_dynamic_callee(&prog) {
            continue;
        }
        let t = interpret(&prog);
        let graph = build(&prog);
        let vals = if bug {
            constprop_nonconst_init(&prog, &graph)
        } else {
            analyze_constprop(&prog, &graph, &[])
        };
        for p in prog.ids() {
            let nparams = prog.proc(p).map(|x| x.params.len()).unwrap_or(0);
            for j in 0..nparams {
                let got = match vals.get(&(p, j)) {
                    None => {
                        violations += 1;
                        continue;
                    }
                    Some(v) => v.clone(),
                };
                if let AbstractValue::Const(c) = got {
                    if let Some(observed) = t.passed.get(&(p, j)) {
                        if observed.iter().any(|v| *v != c) {
                            violations += 1;
                        }
                    }
                }
                if let Some(c) = uniform_literal(&prog, p, j) {
                    if got != AbstractValue::Const(c) {
                        violations += 1;
                    }
                }
            }
        }
    }
    violations
}

/// The seed for [`constprop_fuzz`]: the library's own lattice algebra (`meet`,
/// `jump_of_expr`, `apply_jump`) driven from a map initialised to `NonConst`
/// instead of `Top`. Because `NonConst` absorbs in `meet`, the first visit
/// permanently poisons every formal, which is exactly the mistake the dense-map
/// fix removed — and the precision half of the fuzz is what catches it.
fn constprop_nonconst_init(
    prog: &IpaProgram,
    cg: &CallGraph,
) -> BTreeMap<(ProcId, usize), AbstractValue> {
    let mut values: BTreeMap<(ProcId, usize), AbstractValue> = BTreeMap::new();
    for p in prog.ids() {
        if let Some(proc) = prog.proc(p) {
            for j in 0..proc.params.len() {
                values.insert((p, j), AbstractValue::NonConst);
            }
        }
    }
    let mut work: Vec<ProcId> = prog.ids().collect();
    let cap = 10_000 + prog.procs.len() * 16;
    let mut guard = 0;
    while let Some(p) = work.pop() {
        guard += 1;
        if guard > cap {
            break;
        }
        let proc = match prog.proc(p) {
            Some(x) => x,
            None => continue,
        };
        for stmt in &proc.stmts {
            if let IpaStmt::Call(c) = stmt {
                for callee in static_callees(&c.callee, p, cg) {
                    for (j, arg) in c.args.iter().enumerate() {
                        let out = apply_jump(&jump_of_expr(arg), &values, p);
                        let old = values
                            .get(&(callee, j))
                            .cloned()
                            .unwrap_or(AbstractValue::NonConst);
                        let new = meet(old.clone(), out);
                        if new != old {
                            values.insert((callee, j), new);
                            work.push(callee);
                        }
                    }
                }
            }
        }
    }
    values
}

/// The callees a call may reach, by the rule the analyses use.
fn static_callees(callee: &Callee, caller: ProcId, cg: &CallGraph) -> Vec<ProcId> {
    match callee {
        Callee::Direct(c) => vec![*c],
        Callee::Indirect(IpaExpr::FnAddr(q)) => vec![*q],
        Callee::Indirect(IpaExpr::Read(Slot::Param(j))) => {
            cg.passed_at(caller, *j).into_iter().collect()
        }
        Callee::Indirect(_) => Vec::new(),
    }
}



/// Does any procedure in `prog` hold a call the static graph can only resolve
/// through arg flow? Such a callee's formal is not comparable against a uniform
/// literal, so [`constprop_fuzz`] declines the precision half for those programs.
fn any_dynamic_callee(prog: &IpaProgram) -> bool {
    prog.procs.iter().any(|p| has_dynamic_callee(p))
}

/// The single literal every call site to `(p, j)` passes, if there is at least
/// one such site and they all pass that literal. Purely syntactic — the oracle's
/// own exact opposite of the analysis it checks.
fn uniform_literal(prog: &IpaProgram, p: ProcId, j: usize) -> Option<i64> {
    let mut seen: Option<i64> = None;
    for q in prog.ids() {
        let proc = prog.proc(q).expect("a generated procedure exists");
        for stmt in &proc.stmts {
            let c = match stmt {
                IpaStmt::Call(c) => c,
                _ => continue,
            };
            // Every target this call can name, by the rule the call graph's first
            // pass uses: a direct callee, or an `&f` that denotes exactly `f`.
            let target = match &c.callee {
                Callee::Direct(t) => *t,
                Callee::Indirect(IpaExpr::FnAddr(t)) => *t,
                // A target arg flow alone can name: no claimable uniformity.
                Callee::Indirect(_) => return None,
            };
            if target != p {
                continue;
            }
            match c.args.get(j) {
                Some(IpaExpr::Const(v)) => {
                    if seen.is_some() && seen != Some(*v) {
                        return None;
                    }
                    seen = Some(*v);
                }
                // A non-literal actual: nothing uniform to claim.
                _ => return None,
            }
        }
    }
    seen
}

// ---------------------------------------------------------------------------
// Certificates (§8.10) and determinism (§8.11)
// ---------------------------------------------------------------------------

/// One claim per analysis result, replayed against the program that produced it.
///
/// A certificate is only worth having if an independent replay accepts it, so for
/// each claim the driver requires [`check_ipa_claim`] to accept the derived claim
/// and to *refuse* a perturbed one, and requires the claim to survive a JSON
/// round-trip and still replay. The refusal half is what stops the accept half
/// from passing because the checker ignores what it was handed.
fn certificate_fuzz(seed: u64, cases: usize) -> usize {
    let mut rng = Lcg::new(seed + 5);
    let mut violations = 0;
    let (mut accepted, mut refused) = (0usize, 0usize);
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let cg = build(&prog);
        let cls = classify_purity(&prog, &cg);
        let mod_a = analyze_mod(&prog, &cg);
        let sums = compute_summaries(&prog, &cg);
        let vals = analyze_constprop(&prog, &cg, &[]);

        for p in prog.ids() {
            // Purity (§8.4): the rebuilt value replays; another does not.
            let got = cls[p.index()].clone();
            violations += pair(
                &mut accepted,
                &mut refused,
                &prog,
                IpaClaim::Purity { proc: p, purity: got.clone() },
                IpaClaim::Purity { proc: p, purity: perturb_purity(&got) },
            );

            // MOD (§8.5.2): the exact set replays; the same set plus one slot the
            // analysis did not report does not.
            let set: Vec<Slot> = mod_a.mod_sets[&p].iter().copied().collect();
            if let Some(extra) = spare_slot(&prog, p, &mod_a.mod_sets[&p]) {
                let mut more = set.clone();
                more.push(extra);
                violations += pair(
                    &mut accepted,
                    &mut refused,
                    &prog,
                    IpaClaim::ModSet { proc: p, symbols: set },
                    IpaClaim::ModSet { proc: p, symbols: more },
                );
            }

            // Summary (§8.7): exact sets replay; one fewer write does not.
            if let Some(s) = sums.get(&p) {
                let read: Vec<Slot> = s.read_set.iter().copied().collect();
                let write: Vec<Slot> = s.write_set.iter().copied().collect();
                if let Some(first) = write.first().copied() {
                    let less: Vec<Slot> = write.iter().copied().filter(|x| *x != first).collect();
                    violations += pair(
                        &mut accepted,
                        &mut refused,
                        &prog,
                        IpaClaim::Summary { proc: p, read: read.clone(), write: write.clone() },
                        IpaClaim::Summary { proc: p, read, write: less },
                    );
                }
            }

            // Formal value (§8.5.3): the lattice's answer replays; a different
            // constant or a swapped lattice element does not.
            let nparams = prog.proc(p).map(|x| x.params.len()).unwrap_or(0);
            for j in 0..nparams {
                if let Some(v) = vals.get(&(p, j)) {
                    violations += pair(
                        &mut accepted,
                        &mut refused,
                        &prog,
                        IpaClaim::FormalValue { proc: p, formal: j, value: v.clone() },
                        IpaClaim::FormalValue { proc: p, formal: j, value: perturb_value(v) },
                    );
                }
            }
        }

        // I/O order (§8.3): a same-channel pair must be accepted, a
        // different-channel pair refused.
        for p in prog.ids() {
            let ops = io_ops(&prog, p);
            if let Some((a, b)) = find_pair(&ops, true) {
                if check_ipa_claim(&IpaClaim::IoOrder { proc: p, first: a, second: b }, &prog).is_err()
                {
                    violations += 1;
                } else {
                    accepted += 1;
                }
                if let Some((c, d)) = find_pair(&ops, false) {
                    let got = check_ipa_claim(&IpaClaim::IoOrder { proc: p, first: c, second: d }, &prog);
                    if got.is_ok() {
                        violations += 1;
                    } else {
                        refused += 1;
                    }
                }
            }
        }
    }
    // If either half never happened, the replay proved nothing.
    if accepted == 0 || refused == 0 {
        violations += 1;
    }

    violations
}



/// Replay `good`, refuse `bad`. `good` must also survive a JSON round-trip and
/// still replay, because a certificate that cannot be serialized is not a
/// certificate. Returns one violation per refusal to accept, per failure to
/// refuse, and per round-trip mismatch; counts each accepted/refused claim.
fn pair(
    accepted: &mut usize,
    refused: &mut usize,
    prog: &IpaProgram,
    good: IpaClaim,
    bad: IpaClaim,
) -> usize {
    let mut violations = 0;
    let json = serde_json::to_string(&good).expect("a derived claim is serializable");
    let back: IpaClaim = serde_json::from_str(&json).expect("and deserializable");
    if back != good {
        violations += 1;
    }
    if check_ipa_claim(&good, prog).is_err() {
        violations += 1;
    } else {
        *accepted += 1;
    }
    if check_ipa_claim(&back, prog).is_err() {
        violations += 1;
    }
    if check_ipa_claim(&bad, prog).is_ok() {
        violations += 1;
    } else {
        *refused += 1;
    }
    violations
}

/// A purity guaranteed to differ from `got`.
fn perturb_purity(got: &Purity) -> Purity {
    match got {
        Purity::Pure => Purity::Impure,
        Purity::Impure => Purity::Pure,
        Purity::PureSubroutine { modified } => {
            let mut m = modified.clone();
            m.push(m.len() + 1);
            Purity::PureSubroutine { modified: m }
        }
    }
}

/// An abstract value guaranteed to differ from `got`.
fn perturb_value(got: &AbstractValue) -> AbstractValue {
    match got {
        AbstractValue::Const(c) => AbstractValue::Const(c.wrapping_add(1)),
        AbstractValue::Top => AbstractValue::NonConst,
        AbstractValue::NonConst => AbstractValue::Const(0),
    }
}

/// A slot in `p`'s naming that `set` does not already contain, if one exists.
fn spare_slot(prog: &IpaProgram, p: ProcId, set: &BTreeSet<Slot>) -> Option<Slot> {
    let proc = prog.proc(p)?;
    (0..proc.params.len())
        .map(Slot::Param)
        .chain((0..proc.locals.len()).map(Slot::Local))
        .chain((0..prog.globals.len()).map(Slot::Global))
        .find(|s| !set.contains(s))
}

/// Two I/O operations that share (`same`) or do not share a channel, the first
/// such pair in statement order.
fn find_pair(ops: &[IoOp], same: bool) -> Option<(usize, usize)> {
    for (i, a) in ops.iter().enumerate() {
        for b in ops.iter().skip(i + 1) {
            if (a.channel == b.channel) == same {
                return Some((a.stmt, b.stmt));
            }
        }
    }
    None
}


/// The call graph (§8.2) must know an edge for every call the run actually made,
/// including one made through a resolved `&f`, and must record a call-site
/// statement index for each edge it claims.
///
/// `bug` uses [`textual_only`]'s graph, so the edges the resolving second pass
/// discovers are missing.
fn callgraph_fuzz(seed: u64, cases: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed + 4);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let real = build(&prog);
        let g = if bug { textual_only(&prog) } else { real.clone() };
        for (caller, callee) in &t.calls {
            if !g.callees(*caller).contains(callee) {
                violations += 1;
                continue;
            }
            if g.sites(*caller, *callee).is_empty() {
                violations += 1;
            }
        }
        // An edge the graph claims must come from a call statement in the caller
        // that can name that callee, or from arg flow the interpreter declined to
        // follow — never from nothing.
        for (caller, set) in &g.edges {
            for callee in set {
                if t.calls.contains(&(*caller, *callee)) {
                    continue;
                }
                if !names_callee(&prog, *caller, *callee) {
                    violations += 1;
                }
            }
        }
        // `unresolved` must be justified by a call through a formal.
        for caller in &real.unresolved {
            if !has_dynamic_callee(prog.proc(*caller).expect("a generated procedure exists")) {
                violations += 1;
            }
        }
    }
    violations
}

/// Does some statement of `caller` name `callee` as its target, directly or
/// through a resolved `&f`?
fn names_callee(prog: &IpaProgram, caller: ProcId, callee: ProcId) -> bool {
    let proc = match prog.proc(caller) {
        Some(p) => p,
        None => return false,
    };
    proc.stmts.iter().any(|s| match s {
        IpaStmt::Call(c) => match &c.callee {
            Callee::Direct(q) | Callee::Indirect(IpaExpr::FnAddr(q)) => *q == callee,
            Callee::Indirect(_) => false,
        },
        _ => false,
    })
}

/// Does this procedure contain a call the static graph resolves through arg flow?
fn has_dynamic_callee(proc: &Proc) -> bool {
    proc.stmts.iter().any(|s| match s {
        IpaStmt::Call(c) => matches!(&c.callee, Callee::Indirect(IpaExpr::Read(_))),
        _ => false,
    })
}


// ---------------------------------------------------------------------------
// Determinism (§8.11) and the remaining module-level checks
// ---------------------------------------------------------------------------

/// Every Phase 8 analysis is a worklist fixpoint, so its answer must not depend
/// on when a procedure happens to be revisited, nor on being computed twice. A
/// non-deterministic answer cannot be certified: the producer and `m0-check`
/// would rebuild different results from the same program.
fn determinism_fuzz(seed: u64, cases: usize) -> usize {
    let mut rng = Lcg::new(seed + 6);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        if snapshot(&prog) != snapshot(&prog) {
            violations += 1;
        }
    }
    violations
}

/// The pipeline's whole answer as text: the call graph, purity, MOD (sets and
/// binding-graph SCCs), summaries with their I/O part, constant propagation, and
/// one certificate claim per result. Text rather than a struct because the
/// comparison is exact and several parts have no `PartialEq` worth relying on.
fn snapshot(prog: &IpaProgram) -> String {
    let cg = build(prog);
    let cls = classify_purity(prog, &cg);
    let mod_a = analyze_mod(prog, &cg);
    let sums = compute_summaries(prog, &cg);
    let io = io_summaries(prog, &cg);
    let vals = analyze_constprop(prog, &cg, &[]);
    let mut claims = Vec::new();
    for p in prog.ids() {
        claims.push(IpaClaim::Purity { proc: p, purity: cls[p.index()].clone() });
        claims.push(IpaClaim::ModSet {
            proc: p,
            symbols: mod_a.mod_sets[&p].iter().copied().collect(),
        });
        claims.push(IpaClaim::Summary {
            proc: p,
            read: sums[&p].read_set.iter().copied().collect(),
            write: sums[&p].write_set.iter().copied().collect(),
        });
    }
    let mut out = String::new();
    out.push_str(&format!("{:?}", cg));
    out.push('\n');
    out.push_str(&format!("{:?}", cls));
    out.push('\n');
    out.push_str(&format!("{:?}", mod_a.mod_sets));
    out.push('\n');
    out.push_str(&format!("{:?}", mod_a.sccs));
    out.push('\n');
    out.push_str(&format!("{:?}", mod_a.modified_globals));
    out.push('\n');
    out.push_str(&format!("{:?}", sums));
    out.push('\n');
    out.push_str(&format!("{:?}", io));
    out.push('\n');
    out.push_str(&format!("{:?}", vals));
    out.push('\n');
    out.push_str(&format!("{:?}", claims));
    out.push('\n');
    out.push_str(&format!("{:?}", prog));
    out
}

/// Summary composition (§8.9): translating a callee's read/write sets into the
/// caller's space at a call site must land inside what the bottom-up pass (§8.7)
/// already claims for the caller, because that pass folded this very site.
/// `map_symbol` is checked against the model's binding rule directly.
fn compose_fuzz(seed: u64, cases: usize) -> usize {
    let mut rng = Lcg::new(seed + 7);
    let mut violations = 0;
    let mut sites = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let cg = build(&prog);
        let sums = compute_summaries(&prog, &cg);
        for p in prog.ids() {
            let proc = prog.proc(p).expect("a generated procedure exists");
            let mine = &sums[&p];
            for stmt in &proc.stmts {
                let call = match stmt {
                    IpaStmt::Call(c) => c,
                    _ => continue,
                };
                let q = match &call.callee {
                    Callee::Direct(q) => *q,
                    Callee::Indirect(IpaExpr::FnAddr(q)) => *q,
                    // An arg-flow callee's site is §8.7's own tests' business;
                    // here the call site is what is under inspection.
                    Callee::Indirect(_) => continue,
                };
                sites += 1;
                let sub = &sums[&q];
                let mut reads = mine.read_set.clone();
                let mut writes = mine.write_set.clone();
                compose_into(&prog, call, &sub.read_set, &sub.write_set, &mut reads, &mut writes);
                // Folding the callee in must not grow the caller's answer: the
                // bottom-up pass already folded this exact site.
                if reads != mine.read_set || writes != mine.write_set {
                    violations += 1;
                }
                // A callee local cannot translate, and a formal whose actual the
                // model cannot name translates to nothing; a slot-naming actual
                // translates to exactly that slot.
                for k in 0..prog.proc(q).map(|x| x.locals.len()).unwrap_or(0) {
                    if map_symbol(&prog, call, Slot::Local(k)).is_some() {
                        violations += 1;
                    }
                }
                for j in 0..prog.proc(q).map(|x| x.params.len()).unwrap_or(0) {
                    let mapped = map_symbol(&prog, call, Slot::Param(j));
                    match call.args.get(j) {
                        Some(IpaExpr::Read(s)) | Some(IpaExpr::AddrOf(s)) => {
                            if mapped != Some(*s) {
                                violations += 1;
                            }
                        }
                        _ => {
                            if mapped.is_some() {
                                violations += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    if sites == 0 {
        violations += 1;
    }
    violations
}


/// Top-down context refinement (§8.8): a `Const` formal must leave the
/// specialised summary's read and write sets, no procedure may be cloned more
/// than [`MAX_CONTEXTS`] times, every returned context must name a real
/// procedure with the right arity, and specialisation may only ever remove — it
/// is a projection of the bottom-up summary, never an addition to it.
fn context_fuzz(seed: u64, cases: usize) -> usize {
    let mut rng = Lcg::new(seed + 8);
    let mut violations = 0;
    let mut refined = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let cg = build(&prog);
        let sums = compute_summaries(&prog, &cg);
        let pairs = refine_context(&prog, &cg, &sums, prog.entry);
        let mut per_proc: BTreeMap<ProcId, usize> = BTreeMap::new();
        for (ctx, s) in &pairs {
            refined += 1;
            let n = prog.proc(ctx.proc).map(|x| x.params.len());
            // A context names a real procedure and carries one shape per formal.
            match n {
                Some(n) if n == ctx.args.len() => {}
                _ => {
                    violations += 1;
                    continue;
                }
            }
            *per_proc.entry(ctx.proc).or_insert(0) += 1;
            // The root context — everything unknown — is the bottom-up summary.
            if ctx.args.iter().all(|a| matches!(a, ArgShape::Unknown)) {
                if s.read_set != sums[&ctx.proc].read_set
                    || s.write_set != sums[&ctx.proc].write_set
                {
                    violations += 1;
                }
            }
            // A constant formal is neither read nor written in the specialisation.
            for (j, shape) in ctx.args.iter().enumerate() {
                if matches!(shape, ArgShape::Const(_))
                    && (s.read_set.contains(&Slot::Param(j)) || s.write_set.contains(&Slot::Param(j)))
                {
                    violations += 1;
                }
            }
            // Specialisation only removes.
            if !s.read_set.is_subset(&sums[&ctx.proc].read_set)
                || !s.write_set.is_subset(&sums[&ctx.proc].write_set)
            {
                violations += 1;
            }
        }
        if per_proc.values().any(|c| *c > MAX_CONTEXTS) {
            violations += 1;
        }
    }
    if refined == 0 {
        violations += 1;
    }
    violations
}

/// The I/O summaries (§8.3) on their own, so a failure localizes: every channel
/// the run touched below a procedure must be in that procedure's transitive
/// summary. `bug` deletes every call statement, so a channel reachable only
/// through a callee disappears from its callers.
fn io_fuzz(seed: u64, cases: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed + 9);
    let mut violations = 0;
    for _ in 0..cases {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let subject = if bug { drop_calls(&prog) } else { prog.clone() };
        let cg = build(&subject);
        let io = io_summaries(&subject, &cg);
        for p in prog.ids() {
            let ext = t.extents.get(&p).cloned().unwrap_or_default();
            let sum = match io.get(&p) {
                Some(s) => s,
                None => {
                    violations += 1;
                    continue;
                }
            };
            for ch in &ext.channels {
                if !sum.touches(ch) {
                    violations += 1;
                }
            }
        }
    }
    violations
}

/// A program with every call statement removed.
fn drop_calls(prog: &IpaProgram) -> IpaProgram {
    let mut out = prog.clone();
    for p in &mut out.procs {
        p.stmts.retain(|s| !matches!(s, IpaStmt::Call(_)));
    }
    assert_eq!(out.validate(), Ok(()));
    out
}

// ---------------------------------------------------------------------------
// The drivers
// ---------------------------------------------------------------------------

const SEED: u64 = 0x5EED;
const CASES: usize = 2000;

#[test]
fn mod_analysis_covers_every_observed_write() {
    for seed in [1u64, 2, 3, 42, 1234, 999_999, SEED] {
        assert_eq!(mod_fuzz(seed, CASES, false), 0, "seed {seed} produced a MOD violation");
    }
}

#[test]
fn purity_is_never_over_optimistic() {
    for seed in [1u64, 5, 13, 4242, 55_555, SEED] {
        assert_eq!(purity_fuzz(seed, CASES, false), 0, "seed {seed} produced a purity violation");
    }
}

#[test]
fn summaries_cover_reads_writes_callees_and_channels() {
    for seed in [1u64, 7, 19, 303030, SEED] {
        assert_eq!(summary_fuzz(seed, CASES, false), 0, "seed {seed} produced a summary violation");
    }
}

#[test]
fn constprop_is_sound_and_precise_on_literals() {
    for seed in [1u64, 4, 16, 65536, 1_000_003, SEED] {
        assert_eq!(constprop_fuzz(seed, CASES, false), 0, "seed {seed} produced a constprop violation");
    }
}

#[test]
fn the_call_graph_sees_every_call_the_run_made() {
    for seed in [1u64, 8, 21, 1024, SEED] {
        assert_eq!(callgraph_fuzz(seed, CASES, false), 0, "seed {seed} produced a call graph violation");
    }
}

#[test]
fn every_certificate_replays_and_a_perturbed_one_does_not() {
    for seed in [1u64, 3, 11, 4096, SEED] {
        assert_eq!(certificate_fuzz(seed, CASES), 0, "seed {seed} produced a certificate violation");
    }
}

#[test]
fn the_pipeline_is_deterministic() {
    for seed in [1u64, 23, 999, SEED] {
        assert_eq!(determinism_fuzz(seed, CASES), 0, "seed {seed} produced a non-deterministic answer");
    }
}

#[test]
fn composition_at_a_call_site_matches_the_bottom_up_summary() {
    for seed in [1u64, 17, 2048, SEED] {
        assert_eq!(compose_fuzz(seed, CASES), 0, "seed {seed} produced a composition violation");
    }
}

#[test]
fn context_refinement_only_projects_and_stays_capped() {
    for seed in [1u64, 31, 8192, SEED] {
        assert_eq!(context_fuzz(seed, CASES), 0, "seed {seed} produced a context violation");
    }
}

#[test]
fn io_summaries_cover_every_channel_the_run_touched() {
    for seed in [1u64, 64, 121] {
        assert_eq!(io_fuzz(seed, CASES, false), 0, "seed {seed} produced an I/O violation");
    }
}

// ---------------------------------------------------------------------------
// The oracle is not vacuous: each seeded bug must be detected (§8.11)
// ---------------------------------------------------------------------------

#[test]
fn dropping_binding_propagation_breaks_mod() {
    let seeded = mod_fuzz(9, CASES, true);
    assert!(seeded > 0, "the seeded MOD bug was not detected");
    assert_eq!(mod_fuzz(9, CASES, false), 0, "the corrected analysis still reports violations");
}

#[test]
fn ignoring_callee_effects_breaks_purity() {
    let seeded = purity_fuzz(9, CASES, true);
    assert!(seeded > 0, "the seeded purity bug was not detected");
    assert_eq!(purity_fuzz(9, CASES, false), 0, "the corrected purity still reports violations");
}

#[test]
fn folding_nothing_at_call_sites_breaks_summaries() {
    let seeded = summary_fuzz(9, CASES, true);
    assert!(seeded > 0, "the seeded summary bug was not detected");
    assert_eq!(summary_fuzz(9, CASES, false), 0, "the corrected summaries still report violations");
}

#[test]
fn a_top_seeded_wrong_breaks_constprop() {
    let seeded = constprop_fuzz(9, CASES, true);
    assert!(seeded > 0, "the seeded constprop bug was not detected");
    assert_eq!(constprop_fuzz(9, CASES, false), 0, "the corrected lattice still reports violations");
}

#[test]
fn textual_edges_alone_break_the_call_graph() {
    let seeded = callgraph_fuzz(9, CASES, true);
    assert!(seeded > 0, "the seeded call graph bug was not detected");
    assert_eq!(callgraph_fuzz(9, CASES, false), 0, "the corrected graph still reports violations");
}

#[test]
fn deleting_calls_breaks_the_io_summaries() {
    let seeded = io_fuzz(9, CASES, true);
    assert!(seeded > 0, "the seeded I/O bug was not detected");
    assert_eq!(io_fuzz(9, CASES, false), 0, "the corrected I/O summaries still report violations");
}

#[test]
fn the_generator_reaches_the_interesting_cases() {
    // A "clean" fuzz run only means something if the programs exercise the
    // machinery: a write through a formal, an indirect call, a procedure the run
    // never entered, and at least one uniform literal at a call site.
    let mut rng = Lcg::new(31);
    let (mut formal, mut indirect, mut skipped, mut uniform) = (0, 0, 0, 0);
    for _ in 0..600 {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        if t.through_formal > 0 {
            formal += 1;
        }
        if any_indirect_call(&prog) {
            indirect += 1;
        }
        if prog.ids().any(|p| !t.executed.contains(&p)) {
            skipped += 1;
        }
        for p in prog.ids() {
            for j in 0..prog.proc(p).map(|x| x.params.len()).unwrap_or(0) {
                if uniform_literal(&prog, p, j).is_some() {
                    uniform += 1;
                }
            }
        }
    }
    assert!(formal > 0, "no generated program wrote through a formal");
    assert!(indirect > 0, "no generated program called through &f");
    assert!(skipped > 0, "every generated program executed every procedure");
    assert!(uniform > 0, "no generated program passed a uniform literal");
}

/// Does any statement call through a value rather than by name?
fn any_indirect_call(prog: &IpaProgram) -> bool {
    prog.procs.iter().any(|p| {
        p.stmts.iter().any(|s| match s {
            IpaStmt::Call(c) => matches!(c.callee, Callee::Indirect(_)),
            _ => false,
        })
    })
}


#[test]
fn dbg_first_mod_violation() {
    let mut rng = Lcg::new(1);
    for c in 0..400 {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let cg = build(&prog);
        let a = analyze_mod(&prog, &cg);
        for p in prog.ids() {
            let expect: BTreeSet<Slot> =
                t.wrote.iter().filter(|(q, _)| *q == p).map(|(_, s)| *s).collect();
            let got = a.mod_sets.get(&p).cloned().unwrap_or_default();
            if !expect.is_subset(&got) {
                println!("case {c} proc {:?}", p);
                println!("prog = {}", serde_json::to_string_pretty(&prog).unwrap());
                println!("trace.wrote = {:?}", t.wrote);
                println!("expect = {:?}", expect);
                println!("got = {:?}", got);
                return;
            }
        }
    }
    println!("no violation");
}

#[test]
fn dbg_first_purity_violation() {
    let mut rng = Lcg::new(2);
    for c in 0..400 {
        let prog = gen_program(&mut rng);
        let t = interpret(&prog);
        let cg = build(&prog);
        let cls = classify_purity(&prog, &cg);
        for p in prog.ids() {
            let purity = &cls[p.index()];
            let ext = t.extents.get(&p).cloned().unwrap_or_default();
            let must_be_impure = !ext.globals.is_empty() || ext.io || ext.read_global;
            let bad = (matches!(purity, Purity::Pure)
                && (must_be_impure || !ext.wrote_params.is_empty()))
                || (matches!(purity, Purity::PureSubroutine { .. }) && must_be_impure);
            if bad {
                println!("case {c} proc {:?} purity {:?} ext {:?}", p, purity, ext);
                println!("prog = {}", serde_json::to_string_pretty(&prog).unwrap());
                println!("all purity = {:?}", cls);
                println!("executed = {:?}", t.executed);
                return;
            }
        }
    }
    println!("no violation");
}

#[test]
fn dbg_first_constprop_violation() {
    let mut rng = Lcg::new(65539);
    for c in 0..400 {
        let prog = gen_program(&mut rng);
        if any_dynamic_callee(&prog) {
            continue;
        }
        let t = interpret(&prog);
        let cg = build(&prog);
        let vals = analyze_constprop(&prog, &cg, &[]);
        for p in prog.ids() {
            let nparams = prog.proc(p).map(|x| x.params.len()).unwrap_or(0);
            for j in 0..nparams {
                let got = match vals.get(&(p, j)) {
                    Some(v) => v.clone(),
                    None => continue,
                };
                if let AbstractValue::Const(c0) = got {
                    if let Some(obs) = t.passed.get(&(p, j)) {
                        if obs.iter().any(|v| *v != c0) {
                            println!("case {c} proc {:?} formal {j} got Const({c0}) observed {:?}", p, obs);
                            println!("prog = {}", serde_json::to_string_pretty(&prog).unwrap());
                            println!("passed = {:?}", t.passed);
                            return;
                        }
                    }
                }
                if let Some(c0) = uniform_literal(&prog, p, j) {
                    if got != AbstractValue::Const(c0) {
                        println!("case {c} proc {:?} formal {j} precision: got {:?} uniform {c0}", p, got);
                        println!("prog = {}", serde_json::to_string_pretty(&prog).unwrap());
                        return;
                    }
                }
            }
        }
    }
    println!("no violation");
}
