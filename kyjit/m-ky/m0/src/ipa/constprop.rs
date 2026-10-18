//! Phase 8 §8.6 — interprocedural constant propagation (Wolfe §8.5.3).
//!
//! > "Jump functions: `J(q, j) = c` (Const), `J(q, j) = p.i + c` (Function),
//! >  `J(q, j) = ⊥` (NonConst)."
//!
//! A **jump function** describes how the value at a call site becomes the value
//! of the callee's formal. `f(3)` passes the constant 3; `f(x + 1)` passes the
//! caller's formal `x` plus one; `f(compute())` passes something the analysis
//! cannot name, which is `NonConst` — Wolfe's bottom.
//!
//! The analysis is a worklist over the call graph: seed the entry procedure's
//! arguments, meet each value into the callee's formal lattice, and re-propagate
//! whenever a formal's value improves. `Top` is "no information yet", so the
//! meet of `Top` with anything is that anything; a formal that genuinely
//! receives two different constants becomes `NonConst`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ipa::callgraph::CallGraph;
use crate::ipa::program::{Callee, IpaExpr, IpaProgram, IpaStmt, ProcId, Slot};

/// Wolfe's abstract value lattice (his `⊤` is our `Top`).
///
/// Adjacently tagged (`kind` + `val`) rather than internally tagged: serde
/// cannot serialize an internally tagged *newtype* variant (`Const(i64)`)
/// because there is no field to put the tag in. `IpaClaim::FormalValue` carries
/// one of these, so a certificate containing it must round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "val", rename_all = "snake_case")]
pub enum AbstractValue {
    /// No information yet — the identity of `meet`.
    Top,
    /// A known constant.
    Const(i64),
    /// Known not to be a single constant (Wolfe's `⊥`).
    NonConst,
}

/// The meet of two abstract values: two different constants are `NonConst`, and
/// `Top` is the identity.
pub fn meet(a: AbstractValue, b: AbstractValue) -> AbstractValue {
    match (a, b) {
        (AbstractValue::Top, x) => x,
        (x, AbstractValue::Top) => x,
        (AbstractValue::Const(c1), AbstractValue::Const(c2)) if c1 == c2 => AbstractValue::Const(c1),
        (AbstractValue::NonConst, _) | (_, AbstractValue::NonConst) => AbstractValue::NonConst,
        _ => AbstractValue::NonConst,
    }
}

/// How a call-site value becomes the callee's formal (Wolfe's `J`).
///
/// Adjacently tagged for the same reason as [`AbstractValue`]: `Const(i64)` is a
/// newtype variant and cannot carry an internal tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "val", rename_all = "snake_case")]
pub enum JumpFn {
    /// A constant.
    Const(i64),
    /// The caller's formal `formal`, scaled and shifted: `coeff·formal + offset`.
    Function { formal: usize, coeff: i64, offset: i64 },
    /// Anything else.
    NonConst,
}

/// The jump function for an argument expression, relative to the *caller's*
/// formals (`Slot::Param`).
pub fn jump_of_expr(expr: &IpaExpr) -> JumpFn {
    if let Some(c) = expr.as_const() {
        return JumpFn::Const(c);
    }
    match expr {
        IpaExpr::Read(Slot::Param(j)) => JumpFn::Function { formal: *j, coeff: 1, offset: 0 },
        IpaExpr::Add(a, b) => match (jump_of_expr(a), jump_of_expr(b)) {
            (JumpFn::Function { formal, coeff, offset }, JumpFn::Const(c))
            | (JumpFn::Const(c), JumpFn::Function { formal, coeff, offset }) => JumpFn::Function {
                formal,
                coeff,
                offset: offset + c,
            },
            _ => JumpFn::NonConst,
        },
        IpaExpr::Sub(a, b) => match (jump_of_expr(a), jump_of_expr(b)) {
            (JumpFn::Function { formal, coeff, offset }, JumpFn::Const(c)) => JumpFn::Function {
                formal,
                coeff,
                offset: offset - c,
            },
            _ => JumpFn::NonConst,
        },
        IpaExpr::Mul(a, b) => match (jump_of_expr(a), jump_of_expr(b)) {
            (JumpFn::Function { formal, coeff, offset }, JumpFn::Const(c))
            | (JumpFn::Const(c), JumpFn::Function { formal, coeff, offset }) => JumpFn::Function {
                formal,
                coeff: coeff * c,
                offset: offset * c,
            },
            _ => JumpFn::NonConst,
        },
        _ => JumpFn::NonConst,
    }
}

/// Apply a jump function to the caller's formal values.
///
/// A caller formal that is absent (or still `Top`) means the *call site* has not
/// been reached with any information yet, so nothing can be said of the callee's
/// side and the answer is `Top` — the identity of `meet`, which leaves the
/// callee's lattice alone. Returning `NonConst` here would be a precision bug:
/// it would let a procedure visited before its caller permanently poison its
/// callees, making the fixed point depend on worklist order.
pub fn apply_jump(jf: &JumpFn, values: &BTreeMap<(ProcId, usize), AbstractValue>, caller: ProcId) -> AbstractValue {
    match jf {
        JumpFn::Const(c) => AbstractValue::Const(*c),
        JumpFn::Function { formal, coeff, offset } => {
            match values.get(&(caller, *formal)) {
                Some(AbstractValue::Const(c)) => {
                    match coeff.checked_mul(*c).and_then(|p| p.checked_add(*offset)) {
                        Some(v) => AbstractValue::Const(v),
                        None => AbstractValue::NonConst,
                    }
                }
                Some(AbstractValue::NonConst) => AbstractValue::NonConst,
                // Unreached caller side: no information, not a contradiction.
                Some(AbstractValue::Top) | None => AbstractValue::Top,
            }
        }
        JumpFn::NonConst => AbstractValue::NonConst,
    }
}

/// The abstract value of an expression given a procedure's formal lattice.
pub fn value_of_expr(
    expr: &IpaExpr,
    values: &BTreeMap<(ProcId, usize), AbstractValue>,
    proc: ProcId,
) -> AbstractValue {
    if let Some(c) = expr.as_const() {
        return AbstractValue::Const(c);
    }
    match expr {
        IpaExpr::Read(Slot::Param(j)) => {
            values.get(&(proc, *j)).cloned().unwrap_or(AbstractValue::Top)
        }
        IpaExpr::Add(a, b) => binop(a, b, values, proc, |x, y| x.checked_add(y)),
        IpaExpr::Sub(a, b) => binop(a, b, values, proc, |x, y| x.checked_sub(y)),
        IpaExpr::Mul(a, b) => binop(a, b, values, proc, |x, y| x.checked_mul(y)),
        _ => AbstractValue::NonConst,
    }
}

fn binop(
    a: &IpaExpr,
    b: &IpaExpr,
    values: &BTreeMap<(ProcId, usize), AbstractValue>,
    proc: ProcId,
    f: impl Fn(i64, i64) -> Option<i64>,
) -> AbstractValue {
    match (value_of_expr(a, values, proc), value_of_expr(b, values, proc)) {
        (AbstractValue::Const(x), AbstractValue::Const(y)) => match f(x, y) {
            Some(v) => AbstractValue::Const(v),
            None => AbstractValue::NonConst,
        },
        _ => AbstractValue::NonConst,
    }
}

/// Interprocedural constant propagation (Wolfe §8.5.3).
///
/// `entry_args` seeds the lattice for named procedures (typically just the
/// program entry, with its command-line arguments as `NonConst`). Returns the
/// value each procedure's formal holds at the fixed point.
///
/// The map is **dense**: every `(proc, formal)` pair in the program has an entry,
/// defaulting to `Top` for a formal no call site ever reaches. That makes the
/// result a total function the certificate checker can compare against directly,
/// instead of a partial map whose absence has to be re-interpreted.
pub fn analyze(
    prog: &IpaProgram,
    cg: &CallGraph,
    entry_args: &[(ProcId, Vec<AbstractValue>)],
) -> BTreeMap<(ProcId, usize), AbstractValue> {
    let mut values: BTreeMap<(ProcId, usize), AbstractValue> = BTreeMap::new();
    for p in prog.ids() {
        if let Some(proc) = prog.proc(p) {
            for j in 0..proc.params.len() {
                values.insert((p, j), AbstractValue::Top);
            }
        }
    }
    // Every procedure is visited once; a procedure is re-visited whenever one of
    // its formals' values improves, because that can change what it passes on.
    let mut work: Vec<ProcId> = prog.ids().collect();
    for (p, args) in entry_args {
        for (j, v) in args.iter().enumerate() {
            let new = meet(values.get(&(*p, j)).cloned().unwrap_or(AbstractValue::Top), v.clone());
            values.insert((*p, j), new);
        }
    }
    // What each procedure may modify, formals included: a call that writes
    // through one of *my* formals destroys its value for the rest of my body.
    let modres = crate::ipa::mod_analysis::analyze(prog, cg);
    // Which formals may denote shared storage: only those can be invalidated by a
    // store to a global, so a by-value formal keeps what it holds.
    let aliased = may_alias_global(prog, cg);

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
        // `values` records what a formal holds *on entry*. A body statement can
        // overwrite it — `x = 0; g(x)` does not pass in `g` what the caller passed
        // in `x` — so the call sites are read against `cur`: this procedure's own
        // formals, seeded from the entry lattice and weakened at each write. Only
        // `cur` is weakened; the entry value, which is what the answer reports,
        // belongs to the caller.
        let mut cur: BTreeMap<(ProcId, usize), AbstractValue> = (0..proc.params.len())
            .map(|j| ((p, j), values.get(&(p, j)).cloned().unwrap_or(AbstractValue::Top)))
            .collect();
        let shared = aliased.get(&p).cloned().unwrap_or_default();
        for stmt in &proc.stmts {
            match stmt {
                IpaStmt::Assign { target, value } => {
                    track_write(&mut cur, p, target, Some(value), &shared);
                }
                IpaStmt::Io { .. } => {}
                IpaStmt::Call(c) => {
                    for callee in callees_of(&c.callee, p, cg) {
                        for (j, arg) in c.args.iter().enumerate() {
                            let jf = jump_of_expr(arg);
                            let out = apply_jump(&jf, &cur, p);
                            let old = values.get(&(callee, j)).cloned().unwrap_or(AbstractValue::Top);
                            let new = meet(old.clone(), out);
                            if new != old {
                                values.insert((callee, j), new);
                                work.push(callee);
                            }
                        }
                        kill_through_callee(&mut cur, p, c, callee, &modres, &shared);
                    }
                    // A call that returns into one of my formals writes it, and the
                    // value is whatever the callee computed.
                    if let Some(r) = c.ret {
                        track_write(&mut cur, p, &r, None, &shared);
                    }
                }
            }
        }
    }
    values
}

/// The formals that may denote shared storage. A caller that passed `&g` — or a
/// formal of its own that may itself be `&g` — made the callee's formal another
/// name for a global, so a write to that global writes the formal too, and the
/// body never had to name it as a target. Follows the same binding relation as
/// §8.5.2's binding graph but in the *caller-to-callee* direction, and grows to a
/// fixpoint so a chain of `&x` passing procedures is caught at every depth.
fn may_alias_global(prog: &IpaProgram, cg: &CallGraph) -> BTreeMap<ProcId, BTreeSet<usize>> {
    let mut out: BTreeMap<ProcId, BTreeSet<usize>> =
        prog.ids().map(|p| (p, BTreeSet::new())).collect();
    let mut changed = true;
    let cap = prog.procs.len() * prog.procs.len() + prog.procs.len() + 2;
    let mut guard = 0;
    while changed && guard <= cap {
        guard += 1;
        changed = false;
        for q in prog.ids() {
            let proc = match prog.proc(q) {
                Some(x) => x,
                None => continue,
            };
            for stmt in &proc.stmts {
                let c = match stmt {
                    IpaStmt::Call(c) => c,
                    _ => continue,
                };
                for callee in callees_of(&c.callee, q, cg) {
                    for (j, arg) in c.args.iter().enumerate() {
                        let hits = match arg {
                            IpaExpr::Read(Slot::Global(_)) | IpaExpr::AddrOf(Slot::Global(_)) => {
                                true
                            }
                            IpaExpr::Read(Slot::Param(m)) | IpaExpr::AddrOf(Slot::Param(m)) => {
                                out.get(&q).map(|s| s.contains(m)).unwrap_or(false)
                            }
                            _ => false,
                        };
                        if hits && out.entry(callee).or_default().insert(j) {
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Apply a write of `target` to this procedure's reading map.
///
/// `values` holds what a formal carries *on entry*; `cur` holds what it carries
/// *here*. After `x = e` the two differ — `x = 0; g(x)` must not hand `g` the
/// constant the caller passed in — so the write is tracked, not ignored. A literal
/// right-hand side is nameable and its value is kept; anything else leaves the
/// formal unnamed and becomes `NonConst`. `Top` would be the unsound answer: it is
/// the identity of `meet`, so it would silently keep the entry value alive at every
/// call site below the write. `value: None` is a write whose content the lattice
/// cannot name at all — a call returning into a formal.
fn track_write(
    cur: &mut BTreeMap<(ProcId, usize), AbstractValue>,
    p: ProcId,
    target: &Slot,
    value: Option<&IpaExpr>,
    shared: &BTreeSet<usize>,
) {
    let fresh = match value {
        // Only an assignment whose right-hand side is a literal is nameable here.
        // `x = y + 1` is not tracked: the worklist may visit this body once, so
        // `y` would be read at a value a later statement in the same body already
        // destroyed, and a stale constant is the very error this exists to remove.
        // `NonConst` is sound, and the loss is confined to procedures that
        // overwrite their own parameters.
        Some(v) => match v.as_const() {
            Some(c) => AbstractValue::Const(c),
            None => AbstractValue::NonConst,
        },
        None => AbstractValue::NonConst,
    };
    match target {
        Slot::Param(j) => {
            cur.insert((p, *j), fresh);
        }
        // Shared storage moved: every formal that may name it lost its value.
        Slot::Global(_) => {
            for j in shared {
                cur.insert((p, *j), AbstractValue::NonConst);
            }
        }
        Slot::Local(_) => {}
    }
}

/// A callee that may modify its formal `k` modifies whatever `k` was bound to.
/// When that actual is one of the caller's formals, the caller can no longer name
/// its value at the statements following the call. A callee that may modify a
/// global invalidates the caller's formals that alias one, too — `&g` passed down
/// is the same storage either way.
fn kill_through_callee(
    cur: &mut BTreeMap<(ProcId, usize), AbstractValue>,
    p: ProcId,
    c: &crate::ipa::program::Call,
    callee: ProcId,
    modres: &crate::ipa::mod_analysis::ModAnalysis,
    shared: &BTreeSet<usize>,
) {
    let writes = match modres.mod_sets.get(&callee) {
        Some(w) => w,
        None => return,
    };
    for k in writes {
        match k {
            Slot::Param(j) => {
                if let Some(IpaExpr::Read(Slot::Param(mp)) | IpaExpr::AddrOf(Slot::Param(mp))) =
                    c.args.get(*j)
                {
                    cur.insert((p, *mp), AbstractValue::NonConst);
                }
            }
            Slot::Global(_) => {
                for j in shared {
                    cur.insert((p, *j), AbstractValue::NonConst);
                }
            }
            Slot::Local(_) => {}
        }
    }
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
    use crate::ipa::program::Call;
    use crate::ipa::program::Proc;

    fn proc_with(name: &str, params: Vec<&str>, stmts: Vec<IpaStmt>) -> Proc {
        let mut p = Proc::new(name, params.into_iter().map(String::from).collect(), vec![]);
        p.stmts = stmts;
        p
    }

    fn call1(callee: ProcId, args: Vec<IpaExpr>) -> IpaStmt {
        IpaStmt::Call(Call { callee: Callee::Direct(callee), args, ret: None })
    }

    #[test]
    fn meet_of_different_constants_is_non_const() {
        assert_eq!(meet(AbstractValue::Const(1), AbstractValue::Const(2)), AbstractValue::NonConst);
        assert_eq!(meet(AbstractValue::Top, AbstractValue::Const(1)), AbstractValue::Const(1));
        assert_eq!(meet(AbstractValue::Const(3), AbstractValue::Const(3)), AbstractValue::Const(3));
    }

    #[test]
    fn jump_of_a_constant_and_a_formal() {
        assert_eq!(jump_of_expr(&IpaExpr::Const(7)), JumpFn::Const(7));
        assert_eq!(
            jump_of_expr(&IpaExpr::Read(Slot::Param(2))),
            JumpFn::Function { formal: 2, coeff: 1, offset: 0 }
        );
    }

    #[test]
    fn jump_of_x_plus_one_is_a_shifted_function() {
        let e = IpaExpr::Add(
            Box::new(IpaExpr::Read(Slot::Param(0))),
            Box::new(IpaExpr::Const(1)),
        );
        assert_eq!(jump_of_expr(&e), JumpFn::Function { formal: 0, coeff: 1, offset: 1 });
    }

    #[test]
    fn a_constant_propagates_through_a_call() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Const(5)])]),
                proc_with("g", vec!["x"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        assert_eq!(v[&(ProcId(1), 0)], AbstractValue::Const(5));
    }

    #[test]
    fn a_formal_overwritten_in_the_body_does_not_propagate_its_entry_value() {
        // main() { a(1); }  a(x) { x = 0; b(x); }
        // `x` enters as Const(1) but the body overwrites it before the call to b,
        // so b's formal 0 is Const(0) — the value the run produces. Reading the
        // entry lattice at this call site instead would report Const(1).
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Const(1)])]),
                proc_with(
                    "a",
                    vec!["x"],
                    vec![
                        IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(0) },
                        call1(ProcId(2), vec![IpaExpr::Read(Slot::Param(0))]),
                    ],
                ),
                proc_with("b", vec!["y"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        assert_eq!(v[&(ProcId(1), 0)], AbstractValue::Const(1), "a still enters with 1");
        assert_eq!(v[&(ProcId(2), 0)], AbstractValue::Const(0), "b sees what the body left in x");
    }

    #[test]
    fn a_callee_writing_through_a_formal_invalidates_it_below_the_call() {
        // main() { a(1); }  a(x) { c(&x); b(x); }  c(p) { p = 7; }
        // `c` may modify its formal, bound to a's `x`, so what `a` passes on to
        // `b` is no longer nameable: `NonConst`, never the stale Const(1).
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Const(1)])]),
                proc_with(
                    "a",
                    vec!["x"],
                    vec![
                        call1(ProcId(2), vec![IpaExpr::AddrOf(Slot::Param(0))]),
                        call1(ProcId(3), vec![IpaExpr::Read(Slot::Param(0))]),
                    ],
                ),
                proc_with(
                    "c",
                    vec!["p"],
                    vec![IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(7) }],
                ),
                proc_with("b", vec!["y"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        // `&x` hands c an address, which this scalar lattice cannot name — and the
        // point of the test is what `a` passes on to `b` after the call: not the
        // Const(1) `x` entered with, since `c` may have overwritten it.
        assert_eq!(v[&(ProcId(3), 0)], AbstractValue::NonConst);
    }

    #[test]
    fn a_global_write_does_not_invalidate_a_formal_that_cannot_alias_it() {
        // main() { a(3); }  a(x) { g = 1; b(x); }
        // `x` was passed by value, so it is a private cell that cannot alias the
        // global `g` the body writes, and the constant reaching `b` survives:
        // precision a blunt "any global write kills every formal" rule would lose.
        let prog = IpaProgram::new(
            vec!["g".to_string()],
            vec![
                proc_with(
                    "main",
                    vec![],
                    vec![call1(ProcId(1), vec![IpaExpr::Const(3)])],
                ),
                proc_with(
                    "a",
                    vec!["x"],
                    vec![
                        // `x` was passed by value: it is a private cell, so the
                        // store to `g` cannot reach it.
                        IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(1) },
                        call1(ProcId(2), vec![IpaExpr::Read(Slot::Param(0))]),
                    ],
                ),
                proc_with("b", vec!["y"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        assert_eq!(v[&(ProcId(2), 0)], AbstractValue::Const(3));
    }

    #[test]
    fn a_global_write_does_invalidate_a_formal_bound_to_that_global() {
        // main() { a(&g); }  a(x) { g = 1; b(x); } — here `x` *is* `g`, so the
        // body's write to the global is a write through the formal.
        let prog = IpaProgram::new(
            vec!["g".to_string()],
            vec![
                proc_with(
                    "main",
                    vec![],
                    vec![call1(ProcId(1), vec![IpaExpr::AddrOf(Slot::Global(0))])],
                ),
                proc_with(
                    "a",
                    vec!["x"],
                    vec![
                        IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(1) },
                        call1(ProcId(2), vec![IpaExpr::Read(Slot::Param(0))]),
                    ],
                ),
                proc_with("b", vec!["y"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        assert_eq!(v[&(ProcId(2), 0)], AbstractValue::NonConst);
    }

    #[test]
    fn a_global_write_after_reassigning_an_aliased_formal_kills_it() {
        // main() { a(&g); }  a(x) { x = 5; g = 7; b(x); }
        // `x` *is* `g`, so the assignment to `x` and the store to `g` are writes to
        // one location. Reading them in order, `b` receives 7 — but the lattice
        // cannot name `g`'s value, so after the global store `x` is `NonConst`.
        // Reporting the Const(5) the body left in `cur` would be unsound.
        let prog = IpaProgram::new(
            vec!["g".to_string()],
            vec![
                proc_with(
                    "main",
                    vec![],
                    vec![call1(ProcId(1), vec![IpaExpr::AddrOf(Slot::Global(0))])],
                ),
                proc_with(
                    "a",
                    vec!["x"],
                    vec![
                        IpaStmt::Assign { target: Slot::Param(0), value: IpaExpr::Const(5) },
                        IpaStmt::Assign { target: Slot::Global(0), value: IpaExpr::Const(7) },
                        call1(ProcId(2), vec![IpaExpr::Read(Slot::Param(0))]),
                    ],
                ),
                proc_with("b", vec!["y"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        assert_eq!(v[&(ProcId(2), 0)], AbstractValue::NonConst);
    }

    #[test]
    fn a_callers_formal_plus_one_propagates() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with(
                    "main",
                    vec!["x"],
                    vec![call1(
                        ProcId(1),
                        vec![IpaExpr::Add(
                            Box::new(IpaExpr::Read(Slot::Param(0))),
                            Box::new(IpaExpr::Const(1)),
                        )],
                    )],
                ),
                proc_with("g", vec!["y"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[(ProcId(0), vec![AbstractValue::Const(4)])]);
        assert_eq!(v[&(ProcId(1), 0)], AbstractValue::Const(5));
    }

    #[test]
    fn two_different_constants_collapse_to_non_const() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with(
                    "main",
                    vec![],
                    vec![
                        call1(ProcId(1), vec![IpaExpr::Const(1)]),
                        call1(ProcId(1), vec![IpaExpr::Const(2)]),
                    ],
                ),
                proc_with("g", vec!["x"], vec![]),
            ],
            ProcId(0),
        );
        let cg = callgraph::build(&prog);
        let v = analyze(&prog, &cg, &[]);
        assert_eq!(v[&(ProcId(1), 0)], AbstractValue::NonConst);
    }
}
