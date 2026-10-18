//! Phase 8 feature tests — the §8.2–§8.10 APIs, exercised directly.
//!
//! These check the public surface of every Phase 8 module: the call graph and
//! its SCC decomposition (§8.2), purity (§8.4), the I/O summaries (§8.3), the
//! interprocedural MOD analysis (§8.5), constant propagation and its jump
//! functions (§8.5.3), bottom-up summaries (§8.7), top-down context refinement
//! (§8.8), summary composition at call sites (§8.9), and the certificate replay
//! (§8.10). They complement `phase8_fuzz.rs` (differential against a concrete
//! interpreter) and `phase8_e2e.rs` (the real `m0-check` binary).
//!
//! Programs are built with the constructors in `m0::ipa::program`, so each test
//! states the source it stands for in a comment. Where an answer is a set, the
//! test compares `BTreeSet`s, not iteration order.

use std::collections::BTreeSet;

use m0::certificate::{Certificate, Claim, Witness};
use m0::ir::purity::Purity;
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue};
use m0::ipa::callgraph::{build, tarjan_scc};
use m0::ipa::certificate::{check_ipa_claim, interprocedural_certificate, IpaClaim};
use m0::ipa::compose::{compose_into, map_symbol};
use m0::ipa::constprop::{
    analyze as analyze_constprop, apply_jump, jump_of_expr, meet, AbstractValue, JumpFn,
};
use m0::ipa::context::{derive_context, refine, specialize, ArgShape, ContextKey, MAX_CONTEXTS};
use m0::ipa::io_effects::{has_io, io_ops, must_serialize, summaries as io_summaries, IoSummary};
use m0::ipa::mod_analysis::{analyze as analyze_mod, local_writes};
use m0::ipa::program::{Call, Callee, IpaExpr, IpaProgram, IpaStmt, IoKind, Proc, ProcId, Slot};
use m0::ipa::pure::classify as classify_purity;
use m0::ipa::summaries::{compute as compute_summaries, local_reads, ProcSummary};

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

fn p(name: &str, params: &[&str], locals: &[&str]) -> Proc {
    Proc::new(
        name,
        params.iter().map(|s| s.to_string()).collect(),
        locals.iter().map(|s| s.to_string()).collect(),
    )
}

fn assign(target: Slot, value: IpaExpr) -> IpaStmt {
    IpaStmt::Assign { target, value }
}

/// `ProcId(i)(...)` with the given arguments, result unused.
fn call(i: usize, args: Vec<IpaExpr>) -> IpaStmt {
    IpaStmt::Call(Call {
        callee: Callee::Direct(ProcId(i)),
        args,
        ret: None,
    })
}

/// A call whose result is stored in `ret`.
fn call_ret(i: usize, args: Vec<IpaExpr>, ret: Slot) -> IpaStmt {
    IpaStmt::Call(Call {
        callee: Callee::Direct(ProcId(i)),
        args,
        ret: Some(ret),
    })
}

fn read(s: Slot) -> IpaExpr {
    IpaExpr::Read(s)
}

fn konst(c: i64) -> IpaExpr {
    IpaExpr::Const(c)
}

/// `g`'s index inside [`chain()`] — `main` → `a` → `b` → `g`.
const G: usize = 3;

/// A procedure whose body is `stmts`, with no params or locals.
fn body(name: &str, stmts: Vec<IpaStmt>) -> Proc {
    Proc { name: name.into(), params: vec![], locals: vec![], stmts, ret: None }
}

/// `main() { a(0); } a(x) { b(x); } b(y) { g(y); } g(z) { t = z; }`
///
/// The reference program of the spec's §8.11 fuzz strategy: a call chain that
/// must carry one argument through three frames before it is written.
fn chain() -> IpaProgram {
    let a = Proc {
        name: "a".into(),
        params: vec!["x".into()],
        locals: vec![],
        stmts: vec![call(2, vec![read(Slot::Param(0))])],
        ret: None,
    };
    let b = Proc {
        name: "b".into(),
        params: vec!["y".into()],
        locals: vec![],
        stmts: vec![call(G, vec![read(Slot::Param(0))])],
        ret: None,
    };
    let g = Proc {
        name: "g".into(),
        params: vec!["z".into()],
        locals: vec!["t".into()],
        stmts: vec![assign(Slot::Local(0), read(Slot::Param(0)))],
        ret: None,
    };
    IpaProgram::new(vec![], vec![body("main", vec![call(1, vec![konst(0)])]), a, b, g], ProcId(0))
}

/// `main() { s = h(2); } h(v) { return v + 1; }` — a pure function.
fn pure_fn() -> IpaProgram {
    let mut h = p("h", &["v"], &[]);
    h.ret = Some(IpaExpr::Add(Box::new(read(Slot::Param(0))), Box::new(konst(1))));
    IpaProgram::new(
        vec![],
        vec![Proc {
            name: "main".into(),
            params: vec![],
            locals: vec!["s".into()],
            stmts: vec![call_ret(1, vec![konst(2)], Slot::Local(0))],
            ret: None,
        }, h],
        ProcId(0),
    )
}

/// `main() { k(0); } k(x) { G = 1; }` — a global write, so impure.
fn global_writer() -> IpaProgram {
    let mut k = p("k", &["x"], &[]);
    k.stmts = vec![assign(Slot::Global(0), konst(1))];
    IpaProgram::new(vec!["G".into()], vec![body("main", vec![call(1, vec![konst(0)])]), k], ProcId(0))
}

/// `main() { write("w"); tmp = 1; read("w"); read("r"); }`
fn io_program() -> IpaProgram {
    let mut main = p("main", &[], &["tmp"]);
    main.stmts = vec![
        IpaStmt::Io { channel: "w".into(), kind: IoKind::Write },
        assign(Slot::Local(0), konst(1)),
        IpaStmt::Io { channel: "w".into(), kind: IoKind::Read },
        IpaStmt::Io { channel: "r".into(), kind: IoKind::Read },
    ];
    IpaProgram::new(vec![], vec![main], ProcId(0))
}

/// `main() { ws(); } ws() { G = 1; }`
fn leaf_split() -> IpaProgram {
    IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![])]), body("ws", vec![assign(Slot::Global(0), konst(1))])],
        ProcId(0),
    )
}

/// A program with a two-procedure cycle (`ping <-> pong`) plus `main -> ping`.
fn mutually_recursive() -> IpaProgram {
    IpaProgram::new(
        vec![],
        vec![
            body("main", vec![call(1, vec![])]),
            body("ping", vec![call(2, vec![])]),
            body("pong", vec![call(1, vec![])]),
        ],
        ProcId(0),
    )
}

/// Any valid loop; Phase 8 claims ride on the certificate's `loop_hash`.
fn carrier_loop() -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![Access {
            kind: AccessKind::Write,
            base: "A".into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    }
}

// ---------------------------------------------------------------------------
// §8.2 — call graph construction
// ---------------------------------------------------------------------------

#[test]
fn the_call_graph_records_direct_edges_and_sites() {
    let prog = chain();
    let cg = build(&prog);
    assert_eq!(cg.callees(ProcId(0)), BTreeSet::from([ProcId(1)]));
    assert_eq!(cg.callees(ProcId(1)), BTreeSet::from([ProcId(2)]));
    assert_eq!(cg.callees(ProcId(G)), BTreeSet::new());
    // The site of main→a is statement 0 of main.
    assert_eq!(cg.sites(ProcId(0), ProcId(1)), &[0usize]);
    assert_eq!(cg.sites(ProcId(2), ProcId(G)), &[0usize]);
    assert!(cg.unresolved.is_empty(), "no indirect calls to resolve");
}

#[test]
fn callers_is_the_inverse_of_callees() {
    let cg = build(&mutually_recursive());
    assert_eq!(cg.callers(ProcId(1)), BTreeSet::from([ProcId(0), ProcId(2)]));
    assert_eq!(cg.callers(ProcId(0)), BTreeSet::new());
}

#[test]
fn an_indirect_call_through_a_formal_becomes_an_arg_flow_edge() {
    // main() { apply(fn_add); }  apply(f) { f(); }
    //
    // The callee of `apply`'s call is `Read(Param(0))`, so the only way to know
    // what it reaches is the `⟨passed, position, receiver⟩` triplet recorded
    // when main passes `FnAddr`.
    let apply = Proc {
        name: "apply".into(),
        params: vec!["f".into()],
        locals: vec![],
        stmts: vec![IpaStmt::Call(Call {
            callee: Callee::Indirect(read(Slot::Param(0))),
            args: vec![],
            ret: None,
        })],
        ret: None,
    };
    let mut leaf = p("leaf", &[], &[]);
    leaf.stmts = vec![assign(Slot::Global(0), konst(7))];
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![IpaExpr::FnAddr(ProcId(2))])]), apply, leaf],
        ProcId(0),
    );

    let cg = build(&prog);
    assert_eq!(cg.passed_at(ProcId(1), 0), BTreeSet::from([ProcId(2)]));
    // The resolved edge is what makes the leaf's global write visible to main.
    assert!(cg.callees(ProcId(1)).contains(&ProcId(2)));
    assert!(cg.unresolved.is_empty());
}

#[test]
fn an_unresolvable_indirect_call_is_flagged_not_dropped() {
    // main() { f(); } where `f` is a local the graph cannot name a target for.
    let mut main = p("main", &[], &["f"]);
    main.stmts = vec![IpaStmt::Call(Call {
        callee: Callee::Indirect(read(Slot::Local(0))),
        args: vec![],
        ret: None,
    })];
    let prog = IpaProgram::new(vec![], vec![main], ProcId(0));

    let cg = build(&prog);
    assert!(cg.unresolved.contains(&ProcId(0)));
    assert!(cg.callees(ProcId(0)).is_empty());
}

#[test]
fn tarjan_returns_components_in_reverse_topological_order() {
    // 0 → 1 → 2, 2 → 1: so {1, 2} is a cycle and 0 alone reaches it.
    let succ = |v: usize| match v {
        0 => vec![1],
        1 => vec![2],
        _ => vec![1],
    };
    let sccs = tarjan_scc(3, succ);
    assert_eq!(sccs.len(), 2);
    // Successors first: the cycle is emitted before the vertex that enters it.
    assert_eq!(sccs[0], vec![1, 2]);
    assert_eq!(sccs[1], vec![0]);
}

#[test]
fn sccs_and_is_recursive_agree_on_a_cycle() {
    let prog = mutually_recursive();
    let cg = build(&prog);
    let n = prog.procs.len();
    assert!(cg.is_recursive(ProcId(1), n), "ping is in a cycle");
    assert!(cg.is_recursive(ProcId(2), n), "pong is in the same cycle");
    assert!(!cg.is_recursive(ProcId(0), n), "main is not");
}

#[test]
fn self_recursion_is_recursive() {
    let prog = IpaProgram::new(vec![], vec![body("r", vec![call(0, vec![])])], ProcId(0));
    assert!(build(&prog).is_recursive(ProcId(0), 1));
}

// ---------------------------------------------------------------------------
// §8.4 — purity classification
// ---------------------------------------------------------------------------

#[test]
fn a_function_that_only_computes_its_result_is_pure() {
    let prog = pure_fn();
    let cg = build(&prog);
    let kinds = classify_purity(&prog, &cg);
    assert_eq!(kinds[1], Purity::Pure, "h(v) returns v + 1");
    // main stores into its own local and calls only pure procedures.
    assert!(matches!(kinds[0], Purity::Pure), "main: {kinds:?}");
}

#[test]
fn a_global_write_makes_a_procedure_impure_and_propagates_upward() {
    let prog = global_writer();
    let kinds = classify_purity(&prog, &build(&prog));
    assert_eq!(kinds[1], Purity::Impure, "k writes G");
    assert_eq!(kinds[0], Purity::Impure, "main calls k");
}

#[test]
fn writing_only_a_formal_makes_a_pure_subroutine_naming_that_formal() {
    // main(x) { g(x); }  g(x) { x = 1; }
    //
    // g's only effect is its formal, so it is a pure subroutine reporting
    // `modified = [0]`; main reads a global so it is not `Pure` outright, and
    // the test is about g (Wolfe §8.4).
    let mut g = p("g", &["x"], &[]);
    g.stmts = vec![assign(Slot::Param(0), konst(1))];
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![read(Slot::Global(0))])]), g],
        ProcId(0),
    );
    let kinds = classify_purity(&prog, &build(&prog));
    match &kinds[1] {
        Purity::PureSubroutine { modified } => assert_eq!(modified, &vec![0]),
        other => panic!("expected PureSubroutine for g, got {other:?}"),
    }
}

#[test]
fn io_makes_a_procedure_impure() {
    let prog = io_program();
    let kinds = classify_purity(&prog, &build(&prog));
    assert_eq!(kinds[0], Purity::Impure, "main performs I/O");
}

#[test]
fn a_recursive_cycle_converges_and_stays_pure() {
    // Purity is about side effects, not termination: `ping <-> pong` touches
    // nothing, so the SCC fixpoint must converge and classify it `Pure`
    // (Wolfe §8.4 — "no globals, no I/O"). The property under test is that the
    // classification terminates rather than oscillating around the cycle.
    let prog = mutually_recursive();
    let kinds = classify_purity(&prog, &build(&prog));
    assert_eq!(kinds[1], Purity::Pure, "ping has no side effects");
    assert_eq!(kinds[2], Purity::Pure, "pong has no side effects");
}

#[test]
fn a_recursive_cycle_that_writes_a_global_is_impure_throughout() {
    // ping() { G = 1; pong(); }  pong() { ping(); }
    //
    // The impurity must reach every member of the SCC, not just the writer.
    let mut ping = p("ping", &[], &[]);
    ping.stmts = vec![assign(Slot::Global(0), konst(1)), call(2, vec![])];
    let mut pong = p("pong", &[], &[]);
    pong.stmts = vec![call(1, vec![])];
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![])]), ping, pong],
        ProcId(0),
    );
    let kinds = classify_purity(&prog, &build(&prog));
    assert_eq!(kinds[1], Purity::Impure, "ping writes G");
    assert_eq!(kinds[2], Purity::Impure, "pong calls ping");
    assert_eq!(kinds[0], Purity::Impure, "main calls the cycle");
}

// ---------------------------------------------------------------------------
// §8.3 — I/O dependence summaries
// ---------------------------------------------------------------------------

#[test]
fn io_ops_are_listed_in_statement_order() {
    let ops = io_ops(&io_program(), ProcId(0));
    assert_eq!(ops.len(), 3, "the assignment between them is not I/O");
    assert_eq!(ops.iter().map(|o| o.stmt).collect::<Vec<_>>(), vec![0, 2, 3]);
    assert_eq!(ops[0].kind, IoKind::Write);
    assert_eq!(ops[2].channel, "r");
}

#[test]
fn two_operations_must_serialize_exactly_when_they_share_a_channel() {
    let ops = io_ops(&io_program(), ProcId(0));
    // ops[0] writes "w" and ops[1] reads "w" — same channel, order observable.
    assert!(must_serialize(&ops[0], &ops[1]));
    // ops[2] is on "r" — nothing constrains it against the "w" operations.
    assert!(!must_serialize(&ops[0], &ops[2]));
    assert!(!must_serialize(&ops[1], &ops[2]));
    // A write/read pair on one channel serialises in both directions.
    assert!(must_serialize(&ops[1], &ops[0]));
}

#[test]
fn io_summaries_propagate_transitively_through_calls() {
    // main() { worker(); }  worker() { write("out"); }
    let worker = body("worker", vec![IpaStmt::Io { channel: "out".into(), kind: IoKind::Write }]);
    let prog =
        IpaProgram::new(vec![], vec![body("main", vec![call(1, vec![])]), worker], ProcId(0));
    let cg = build(&prog);

    let s = io_summaries(&prog, &cg);
    assert!(s[&ProcId(0)].touches("out"), "main inherits worker's channel");
    assert!(s[&ProcId(1)].touches("out"));
    assert!(!s[&ProcId(1)].touches("in"));
    assert!(has_io(&prog, &cg)[&ProcId(0)], "main transitively does I/O");

    // A program with no I/O anywhere stays clean.
    let prog2 = pure_fn();
    assert!(!has_io(&prog2, &build(&prog2))[&ProcId(0)]);
}

#[test]
fn an_io_summary_records_read_and_write_directions_separately() {
    // `record` is private, so the directions are observed through `summaries`,
    // which is the same data after its transitive closure.
    let prog = io_program();
    let s = io_summaries(&prog, &build(&prog));
    let me = &s[&ProcId(0)];
    assert!(!me.is_empty());
    assert_eq!(me.channels.get("w"), Some(&(true, true)), "w is read and written");
    assert_eq!(me.channels.get("r"), Some(&(true, false)), "r is only read");
    assert_eq!(me.channels.get("z"), None);
    assert!(me.touches("w") && me.touches("r"));
    // The default summary is empty.
    assert!(IoSummary::default().is_empty());
}

// ---------------------------------------------------------------------------
// §8.5 — interprocedural MOD
// ---------------------------------------------------------------------------

#[test]
fn local_writes_are_the_assignments_in_a_procedure_body_only() {
    let prog = chain();
    assert!(local_writes(&prog, ProcId(0)).is_empty());
    assert_eq!(local_writes(&prog, ProcId(G)), BTreeSet::from([Slot::Local(0)]));
    // A call that stores its result counts as a write of that slot.
    assert_eq!(local_writes(&pure_fn(), ProcId(0)), BTreeSet::from([Slot::Local(0)]));
}

#[test]
fn local_reads_include_arguments_and_returned_values() {
    let prog = chain();
    // main's argument `0` is a constant, so main reads nothing.
    assert!(local_reads(&prog, ProcId(0)).is_empty());
    // a's argument is Read(Param(0)).
    assert_eq!(local_reads(&prog, ProcId(1)), BTreeSet::from([Slot::Param(0)]));
    // h's return expression reads its formal.
    assert_eq!(local_reads(&pure_fn(), ProcId(1)), BTreeSet::from([Slot::Param(0)]));
}

#[test]
fn a_local_written_at_the_bottom_stays_local() {
    let prog = chain();
    let res = analyze_mod(&prog, &build(&prog));
    assert_eq!(res.mod_sets[&ProcId(G)], BTreeSet::from([Slot::Local(0)]));
    // a and b only pass their formal down and nothing writes it, so they modify
    // nothing; main is the same.
    assert!(res.mod_sets[&ProcId(1)].is_empty());
    assert!(res.mod_sets[&ProcId(0)].is_empty());
    assert!(res.modified_globals.is_empty());
}

#[test]
fn a_write_to_a_formal_makes_the_callers_actual_modified() {
    // main() { a(&G); }  a(x) { x = 1; }
    //
    // The binding edge runs from a's Param(0) to main's Global(0), so G is
    // modified in the caller even though main never assigns to it.
    let mut a_proc = p("a", &["x"], &[]);
    a_proc.stmts = vec![assign(Slot::Param(0), konst(1))];
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![IpaExpr::AddrOf(Slot::Global(0))])]), a_proc],
        ProcId(0),
    );
    let res = analyze_mod(&prog, &build(&prog));
    assert!(res.modified_globals.contains(&0), "G is modified");
    // Globals are program-wide, so they appear in every procedure's set.
    for id in prog.ids() {
        assert!(res.mod_sets[&id].contains(&Slot::Global(0)), "proc {id:?}");
    }
}

#[test]
fn mod_on_a_formal_bound_to_another_formal_marks_both_actuals() {
    // swap(a, b) { t = a; a = b; b = t; }  main() { swap(&P, &Q); }
    //
    // Both formals are written, so both globals the call binds are modified.
    let mut swap = p("swap", &["a", "b"], &["t"]);
    swap.stmts = vec![
        assign(Slot::Local(0), read(Slot::Param(0))),
        assign(Slot::Param(0), read(Slot::Param(1))),
        assign(Slot::Param(1), read(Slot::Local(0))),
    ];
    let prog = IpaProgram::new(
        vec!["P".into(), "Q".into()],
        vec![
            body(
                "main",
                vec![call(
                    1,
                    vec![IpaExpr::AddrOf(Slot::Global(0)), IpaExpr::AddrOf(Slot::Global(1))],
                )],
            ),
            swap,
        ],
        ProcId(0),
    );
    let res = analyze_mod(&prog, &build(&prog));
    assert_eq!(res.modified_globals, BTreeSet::from([0usize, 1]));
    assert!(res.mod_sets[&ProcId(1)].contains(&Slot::Param(0)));
    assert!(res.mod_sets[&ProcId(0)].contains(&Slot::Global(1)));
    assert!(!res.sccs.is_empty(), "the binding graph has components");
}

#[test]
fn mod_reports_a_global_written_at_the_bottom_of_a_deep_chain() {
    // main() { one(); } one() { two(); } two() { G = 5; }
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![
            body("main", vec![call(1, vec![])]),
            body("one", vec![call(2, vec![])]),
            body("two", vec![assign(Slot::Global(0), konst(5))]),
        ],
        ProcId(0),
    );
    let res = analyze_mod(&prog, &build(&prog));
    assert_eq!(res.modified_globals, BTreeSet::from([0usize]));
    for id in prog.ids() {
        assert!(res.mod_sets[&id].contains(&Slot::Global(0)), "{id:?} sees the global");
    }
}

// ---------------------------------------------------------------------------
// §8.5.3 — interprocedural constant propagation
// ---------------------------------------------------------------------------

#[test]
fn the_meet_of_two_constants_that_disagree_is_nonconst() {
    use AbstractValue as A;
    assert_eq!(meet(A::Top, A::Const(3)), A::Const(3));
    assert_eq!(meet(A::Const(3), A::Top), A::Const(3));
    assert_eq!(meet(A::Const(3), A::Const(3)), A::Const(3));
    assert_eq!(meet(A::Const(3), A::Const(4)), A::NonConst);
    assert_eq!(meet(A::NonConst, A::Const(3)), A::NonConst);
    assert_eq!(meet(A::Top, A::Top), A::Top);
}

#[test]
fn jump_functions_classify_an_argument_expression() {
    assert_eq!(jump_of_expr(&konst(7)), JumpFn::Const(7));
    assert_eq!(
        jump_of_expr(&read(Slot::Param(2))),
        JumpFn::Function { formal: 2, coeff: 1, offset: 0 }
    );
    assert_eq!(
        jump_of_expr(&IpaExpr::Add(Box::new(read(Slot::Param(1))), Box::new(konst(3)))),
        JumpFn::Function { formal: 1, coeff: 1, offset: 3 }
    );
    assert_eq!(
        jump_of_expr(&IpaExpr::Mul(Box::new(read(Slot::Param(0))), Box::new(konst(2)))),
        JumpFn::Function { formal: 0, coeff: 2, offset: 0 }
    );
    // Something with no affine reading of a formal.
    assert_eq!(jump_of_expr(&read(Slot::Local(0))), JumpFn::NonConst);
}

#[test]
fn applying_a_jump_function_substitutes_the_callers_value() {
    use AbstractValue as A;
    let mut vals = std::collections::BTreeMap::new();
    vals.insert((ProcId(0), 1), A::Const(10));
    let jf = JumpFn::Function { formal: 1, coeff: 2, offset: 1 };
    assert_eq!(apply_jump(&jf, &vals, ProcId(0)), A::Const(21));
    // An unreached caller side says nothing — Top, the identity of meet, so the
    // callee is not poisoned by a visit that happens before its caller's.
    assert_eq!(apply_jump(&jf, &vals, ProcId(5)), A::Top);
    assert_eq!(apply_jump(&JumpFn::NonConst, &vals, ProcId(0)), A::NonConst);
    assert_eq!(apply_jump(&JumpFn::Const(4), &vals, ProcId(0)), A::Const(4));
}

#[test]
fn a_constant_argument_is_known_in_the_callee_down_a_call_chain() {
    // chain(): main calls a(0) — so a's formal 0, and b's and g's, are Const(0).
    let vals = analyze_constprop(&chain(), &build(&chain()), &[]);
    assert_eq!(vals.get(&(ProcId(1), 0)), Some(&AbstractValue::Const(0)));
    assert_eq!(vals.get(&(ProcId(2), 0)), Some(&AbstractValue::Const(0)));
    assert_eq!(vals.get(&(ProcId(G), 0)), Some(&AbstractValue::Const(0)));
}

#[test]
fn two_call_sites_with_different_constants_meet_to_nonconst() {
    // main() { h(1); h(2); }  h(v) { t = v; }
    let mut h = p("h", &["v"], &["t"]);
    h.stmts = vec![assign(Slot::Local(0), read(Slot::Param(0)))];
    let prog = IpaProgram::new(
        vec![],
        vec![body("main", vec![call(1, vec![konst(1)]), call(1, vec![konst(2)])]), h],
        ProcId(0),
    );
    let vals = analyze_constprop(&prog, &build(&prog), &[]);
    assert_eq!(vals.get(&(ProcId(1), 0)), Some(&AbstractValue::NonConst));
}

#[test]
fn the_analysis_is_monotone_in_what_the_entry_is_seeded_with() {
    // main(x) { h(x + 1); }  h(v) { t = v; }
    let mut h = p("h", &["v"], &["t"]);
    h.stmts = vec![assign(Slot::Local(0), read(Slot::Param(0)))];
    let mut main = p("main", &["x"], &[]);
    main.stmts =
        vec![call(1, vec![IpaExpr::Add(Box::new(read(Slot::Param(0))), Box::new(konst(1)))])];
    let prog = IpaProgram::new(vec![], vec![main, h], ProcId(0));
    let cg = build(&prog);

    // Seeded NonConst (unknown at the call boundary): h's formal is unknown too.
    let vals = analyze_constprop(&prog, &cg, &[(ProcId(0), vec![AbstractValue::NonConst])]);
    assert_eq!(vals.get(&(ProcId(1), 0)), Some(&AbstractValue::NonConst));

    // Seeded with a constant: the jump function carries the `+ 1` through.
    let vals = analyze_constprop(&prog, &cg, &[(ProcId(0), vec![AbstractValue::Const(41)])]);
    assert_eq!(vals.get(&(ProcId(1), 0)), Some(&AbstractValue::Const(42)));
}

#[test]
fn a_formal_no_caller_ever_reaches_stays_top() {
    // main() { h(1); }  other(v) { t = v; } — `other` is never called, so the
    // lattice never moves off Top for it.
    let mut h = p("h", &["v"], &["t"]);
    h.stmts = vec![assign(Slot::Local(0), read(Slot::Param(0)))];
    let mut other = p("other", &["v"], &["t2"]);
    other.stmts = vec![assign(Slot::Local(0), read(Slot::Param(0)))];
    let prog = IpaProgram::new(
        vec![],
        vec![body("main", vec![call(1, vec![konst(1)])]), h, other],
        ProcId(0),
    );
    let vals = analyze_constprop(&prog, &build(&prog), &[]);
    assert_eq!(vals.get(&(ProcId(1), 0)), Some(&AbstractValue::Const(1)));
    assert_eq!(vals.get(&(ProcId(2), 0)), Some(&AbstractValue::Top));
}

// ---------------------------------------------------------------------------
// §8.7 — bottom-up summaries
// ---------------------------------------------------------------------------

#[test]
fn a_summary_is_the_union_over_a_procedure_and_every_callee_it_reaches() {
    let prog = leaf_split();
    let s = compute_summaries(&prog, &build(&prog));
    // ws writes G and nothing else.
    assert_eq!(s[&ProcId(1)].write_set, BTreeSet::from([Slot::Global(0)]));
    assert!(s[&ProcId(1)].read_set.is_empty());
    assert_eq!(s[&ProcId(1)].depth, 0, "ws is a leaf");
    // main sees ws's write transitively.
    assert_eq!(s[&ProcId(0)].write_set, BTreeSet::from([Slot::Global(0)]));
    assert_eq!(s[&ProcId(0)].callees, BTreeSet::from([ProcId(1)]));
    assert_eq!(s[&ProcId(0)].depth, 1);
}

#[test]
fn the_chain_summary_names_every_reachable_procedure_but_only_its_own_writes() {
    let prog = chain();
    let s = compute_summaries(&prog, &build(&prog));
    // g's local is g-only, so main's write set stays empty; its callee closure
    // must still name everything below it.
    assert_eq!(s[&ProcId(G)].write_set, BTreeSet::from([Slot::Local(0)]));
    assert!(s[&ProcId(0)].write_set.is_empty(), "a callee local is not the caller's");
    assert_eq!(s[&ProcId(0)].callees, BTreeSet::from([ProcId(1), ProcId(2), ProcId(G)]));
    assert_eq!(s[&ProcId(0)].depth, 3);
    // b reads its formal and passes it down.
    assert!(s[&ProcId(2)].read_set.contains(&Slot::Param(0)));
}

#[test]
fn a_summary_records_the_purity_and_io_of_the_whole_subtree() {
    let prog = global_writer();
    let s = compute_summaries(&prog, &build(&prog));
    assert_eq!(s[&ProcId(0)].purity, Purity::Impure, "main's callee writes a global");
    assert!(s[&ProcId(0)].read_set.is_empty(), "a bare call stores nothing");

    // The I/O subtree rides along the same way.
    let iop = io_program();
    let si = compute_summaries(&iop, &build(&iop));
    assert!(si[&ProcId(0)].io.touches("w"), "main inherits its callee's writer");
}

#[test]
fn a_recursive_summary_terminates_and_covers_the_whole_cycle() {
    // ping() { pong(); }  pong() { ping(); }  — mutual recursion.
    let prog = IpaProgram::new(
        vec![],
        vec![body("ping", vec![call(1, vec![])]), body("pong", vec![call(0, vec![])])],
        ProcId(0),
    );
    let s = compute_summaries(&prog, &build(&prog));
    assert_eq!(s[&ProcId(0)].callees, BTreeSet::from([ProcId(0), ProcId(1)]));
    assert_eq!(s[&ProcId(1)].callees, BTreeSet::from([ProcId(0), ProcId(1)]));
    assert_eq!(s[&ProcId(0)].purity, Purity::Pure);
}

#[test]
fn a_leaf_writes_nothing_and_a_deep_caller_reads_through_its_formals() {
    let prog = pure_fn();
    let s = compute_summaries(&prog, &build(&prog));
    // h(v) { return v + 1 } — it reads its formal and writes nothing.
    assert_eq!(s[&ProcId(1)].read_set, BTreeSet::from([Slot::Param(0)]));
    assert!(s[&ProcId(1)].write_set.is_empty(), "a return value is not a write");
    // main stores the call result into its own local.
    assert_eq!(s[&ProcId(0)].write_set, BTreeSet::from([Slot::Local(0)]));
    // The call passes a literal, so h's formal has no caller-side image to read.
    assert!(s[&ProcId(0)].read_set.is_empty());
    assert_eq!(s[&ProcId(0)].purity, Purity::Pure);
}

// ---------------------------------------------------------------------------
// §8.8 — top-down context refinement
// ---------------------------------------------------------------------------

/// The bottom-up summaries of `prog`, for the refinement tests.
fn base_summaries(prog: &IpaProgram) -> std::collections::BTreeMap<ProcId, ProcSummary> {
    compute_summaries(prog, &build(prog))
}

#[test]
fn a_constant_argument_specialises_away_the_formals_it_cannot_write() {
    // Two formals both named in a summary, as if h's caller had passed &x, &w.
    let mut h = p("h", &["x", "w"], &[]);
    h.stmts = vec![assign(Slot::Param(0), konst(1)), assign(Slot::Param(1), konst(2))];
    let prog = IpaProgram::new(vec![], vec![h], ProcId(0));
    let mut s = compute_summaries(&prog, &build(&prog))[&ProcId(0)].clone();
    s.read_set.insert(Slot::Param(0));
    s.read_set.insert(Slot::Param(1));

    let ctx = ContextKey { proc: ProcId(0), args: vec![ArgShape::Const(3), ArgShape::Unknown] };
    let spec = specialize(&s, &ctx);
    assert!(!spec.read_set.contains(&Slot::Param(0)), "arg 0 is a constant");
    assert!(!spec.write_set.contains(&Slot::Param(0)));
    // An unknown argument keeps both sets: the caller could pass an address.
    assert!(spec.read_set.contains(&Slot::Param(1)));
    assert!(spec.write_set.contains(&Slot::Param(1)));
    assert_eq!(s.read_set.len(), 2, "the base summary is untouched");
}

#[test]
fn a_context_flows_a_callers_constant_down_a_chain() {
    // main() { a(0); } a(x) { b(x); } b(y) { g(y); } g(z) { t = z; }
    let prog = chain();
    let refined = refine(&prog, &build(&prog), &base_summaries(&prog), ProcId(0));
    for (ctx, _) in &refined {
        if ctx.proc != ProcId(0) {
            assert_eq!(ctx.args, vec![ArgShape::Const(0)], "{ctx:?} carries main's 0");
        }
    }
    assert!(refined.iter().any(|(c, _)| c.proc == ProcId(G)));
    assert_eq!(refined.len(), 4, "one context per procedure in the chain");
}

#[test]
fn refinement_recovers_precision_the_bottom_up_union_lost() {
    // main() { h(&G); h(5); }  h(v) { v = 1; }
    //
    // Context-free, h's summary must assume its formal can be written (the &G
    // site). Context-wise, the `h(5)` frame cannot write anything.
    let mut h = p("h", &["v"], &[]);
    h.stmts = vec![assign(Slot::Param(0), konst(1))];
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![IpaExpr::AddrOf(Slot::Global(0))]), call(1, vec![konst(5)])]), h],
        ProcId(0),
    );
    let base = base_summaries(&prog);
    assert!(base[&ProcId(1)].write_set.contains(&Slot::Param(0)), "the union says h writes");

    let refined = refine(&prog, &build(&prog), &base, ProcId(0));
    let const_ctx = refined
        .iter()
        .find(|(c, _)| c.proc == ProcId(1) && c.args == vec![ArgShape::Const(5)])
        .expect("h(5) has its own context");
    assert!(const_ctx.1.write_set.is_empty(), "a constant cannot be written");
    let addr_ctx = refined
        .iter()
        .find(|(c, _)| c.proc == ProcId(1) && c.args == vec![ArgShape::Unknown])
        .expect("h(&G) is an unknown shape");
    assert!(addr_ctx.1.write_set.contains(&Slot::Param(0)));
}

#[test]
fn refinement_is_deterministic_and_bounded_by_the_context_cap() {
    let prog = chain();
    let cg = build(&prog);
    let base = base_summaries(&prog);
    let once = refine(&prog, &cg, &base, ProcId(0));
    let twice = refine(&prog, &cg, &base, ProcId(0));
    assert_eq!(once, twice, "refinement is a function of the program");

    // main calls g from MAX_CONTEXTS + 4 distinct constant sites.
    let stmts: Vec<IpaStmt> =
        (0..(MAX_CONTEXTS + 4)).map(|k| call(1, vec![konst(k as i64)])).collect();
    let fanout = IpaProgram::new(vec![], vec![body("main", stmts), p("g", &["x"], &[])], ProcId(0));
    let r = refine(&fanout, &build(&fanout), &base_summaries(&fanout), ProcId(0));
    let g_ctx = r.iter().filter(|(c, _)| c.proc == ProcId(1)).count();
    assert!(g_ctx <= MAX_CONTEXTS, "{g_ctx} contexts for g");
    assert!(r.iter().all(|(c, _)| c.proc != ProcId(0) || c.args.is_empty()));
}

#[test]
fn the_entry_is_entered_with_unknown_arguments() {
    // main(x) { h(x); } — the root context cannot name main's own formal.
    let h = p("h", &["v"], &[]);
    let main = Proc {
        name: "main".into(),
        params: vec!["x".into()],
        locals: vec![],
        stmts: vec![call(1, vec![read(Slot::Param(0))])],
        ret: None,
    };
    let prog = IpaProgram::new(vec![], vec![main, h], ProcId(0));
    let refined = refine(&prog, &build(&prog), &base_summaries(&prog), ProcId(0));
    let root = &refined[0];
    assert_eq!(root.0.proc, ProcId(0));
    assert_eq!(root.0.args, vec![ArgShape::Unknown]);
    // `main(x)` with x unknown makes h's formal unknown, so it keeps its shape.
    assert!(refined.iter().any(|(c, _)| c.proc == ProcId(1) && c.args == vec![ArgShape::Unknown]));
}

// ---------------------------------------------------------------------------
// §8.9 — summary composition at a call site
// ---------------------------------------------------------------------------

/// `main() { h(&G, q); }` — the single call site the composition tests use.
fn compose_case() -> (IpaProgram, Call) {
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![IpaExpr::AddrOf(Slot::Global(0)), read(Slot::Local(0))])])],
        ProcId(0),
    );
    let call = match &prog.proc(ProcId(0)).unwrap().stmts[0] {
        IpaStmt::Call(c) => c.clone(),
        _ => unreachable!(),
    };
    (prog, call)
}

#[test]
fn a_global_passes_through_unchanged_and_a_local_never_does() {
    let (prog, call) = compose_case();
    assert_eq!(map_symbol(&prog, &call, Slot::Global(3)), Some(Slot::Global(3)));
    // A callee's private local has no meaning in the caller's frame.
    assert_eq!(map_symbol(&prog, &call, Slot::Local(7)), None);
}

#[test]
fn a_formal_maps_to_the_actual_written_at_the_call_site() {
    let (prog, call) = compose_case();
    // arg 0 is `&G`, arg 1 is a read of the caller's local.
    assert_eq!(map_symbol(&prog, &call, Slot::Param(0)), Some(Slot::Global(0)));
    assert_eq!(map_symbol(&prog, &call, Slot::Param(1)), Some(Slot::Local(0)));
    // A literal argument cannot be named by the callee, so nothing maps.
    assert_eq!(map_symbol(&prog, &call, Slot::Param(2)), None);
    let lit = Call { callee: Callee::Direct(ProcId(1)), args: vec![konst(4)], ret: None };
    assert_eq!(map_symbol(&prog, &lit, Slot::Param(0)), None);
}

#[test]
fn composing_folds_the_callees_effects_into_the_callers_sets() {
    let (prog, call) = compose_case();
    let reads = BTreeSet::from([Slot::Param(1), Slot::Global(2)]);
    let writes = BTreeSet::from([Slot::Param(0), Slot::Local(0)]);
    let mut cr = BTreeSet::from([Slot::Local(3)]);
    let mut cw = BTreeSet::new();
    assert!(compose_into(&prog, &call, &reads, &writes, &mut cr, &mut cw));
    assert_eq!(cr, BTreeSet::from([Slot::Local(3), Slot::Local(0), Slot::Global(2)]));
    assert_eq!(cw, BTreeSet::from([Slot::Global(0)]), "Param(0) is &G");
    // Param(1) is the caller's own local, which it already reads by passing it.
    assert!(!cr.contains(&Slot::Param(1)), "the callee's frame does not leak up");
    assert!(!cw.contains(&Slot::Local(0)), "a callee local stays private");
}

#[test]
fn composing_reports_no_change_when_everything_maps_to_something_already_known() {
    let (prog, call) = compose_case();
    let reads = BTreeSet::from([Slot::Param(0)]);
    let writes = BTreeSet::from([Slot::Param(0)]);
    let mut cr = BTreeSet::from([Slot::Global(0)]);
    let mut cw = BTreeSet::from([Slot::Global(0)]);
    assert!(
        !compose_into(&prog, &call, &reads, &writes, &mut cr, &mut cw),
        "the fixed point has already been reached"
    );
}

#[test]
fn an_unnamed_callee_effect_is_dropped_conservatively() {
    // The model cannot resolve a computed actual, so a write through that formal
    // is dropped rather than guessed at — the same choice Phase 7 makes.
    let prog = IpaProgram::new(
        vec!["G".into()],
        vec![body("main", vec![call(1, vec![IpaExpr::Add(Box::new(read(Slot::Local(0))), Box::new(konst(1)))])])],
        ProcId(0),
    );
    let call = match &prog.proc(ProcId(0)).unwrap().stmts[0] {
        IpaStmt::Call(c) => c.clone(),
        _ => unreachable!(),
    };
    let writes = BTreeSet::from([Slot::Param(0)]);
    let mut cr = BTreeSet::new();
    let mut cw = BTreeSet::new();
    assert!(!compose_into(&prog, &call, &BTreeSet::new(), &writes, &mut cr, &mut cw));
    assert!(cw.is_empty());
}

// ---------------------------------------------------------------------------
// §8.10 — certificate replay
// ---------------------------------------------------------------------------

/// Replay `claim` in both directions: the analyzer must accept a claim it
/// entails and reject one it contradicts.
fn replay(claim: IpaClaim, prog: &IpaProgram) -> Result<(), String> {
    check_ipa_claim(&claim, prog)
}

#[test]
fn a_purity_claim_replays_against_the_rebuilt_classification() {
    let prog = global_writer();
    let ok = IpaClaim::Purity { proc: ProcId(0), purity: Purity::Impure };
    assert_eq!(replay(ok, &prog), Ok(()));
    let bad = IpaClaim::Purity { proc: ProcId(0), purity: Purity::Pure };
    assert!(replay(bad, &prog).unwrap_err().contains("rebuilt purity"));
}

#[test]
fn a_mod_set_claim_must_name_exactly_the_rebuilt_set() {
    let prog = leaf_split();
    let ok = IpaClaim::ModSet { proc: ProcId(1), symbols: vec![Slot::Global(0)] };
    assert_eq!(replay(ok, &prog), Ok(()));
    // Under-claiming is rejected: the checker recomputes and compares sets.
    let missing = IpaClaim::ModSet { proc: ProcId(1), symbols: vec![] };
    assert!(replay(missing, &prog).unwrap_err().contains("disagrees"));
    // So is over-claiming, and the order of the claim does not matter.
    let extra = IpaClaim::ModSet { proc: ProcId(1), symbols: vec![Slot::Global(0), Slot::Global(0)] };
    assert_eq!(replay(extra, &prog), Ok(()), "a set, not a list");
}

#[test]
fn an_io_order_claim_needs_two_statements_on_one_channel() {
    // main's I/O statements are 0 (write "w"), 2 (read "w"), 3 (read "r").
    let prog = io_program();
    assert_eq!(replay(IpaClaim::IoOrder { proc: ProcId(0), first: 0, second: 2 }, &prog), Ok(()));
    assert!(replay(IpaClaim::IoOrder { proc: ProcId(0), first: 0, second: 3 }, &prog)
        .unwrap_err()
        .contains("not on the same channel"));
    assert!(replay(IpaClaim::IoOrder { proc: ProcId(0), first: 1, second: 2 }, &prog)
        .unwrap_err()
        .contains("not an I/O operation"), "statement 1 is an assignment");
}

#[test]
fn a_formal_value_claim_replays_the_constant_propagation() {
    // main() { g(5); } g(x) { } — g's formal is Const(5).
    let prog = IpaProgram::new(
        vec![],
        vec![body("main", vec![call(1, vec![konst(5)])]), p("g", &["x"], &[])],
        ProcId(0),
    );
    assert_eq!(
        replay(IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(5) }, &prog),
        Ok(())
    );
    let err = replay(
        IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(6) },
        &prog,
    )
    .unwrap_err();
    assert!(err.contains("rebuilt formal value"), "{err}");
    // A formal that does not exist is refused rather than read as Top.
    assert!(replay(
        IpaClaim::FormalValue { proc: ProcId(1), formal: 9, value: AbstractValue::Top },
        &prog
    )
    .unwrap_err()
    .contains("no formal"));
}

#[test]
fn a_summary_claim_must_match_both_rebuilt_sets() {
    let prog = leaf_split();
    let ok = IpaClaim::Summary { proc: ProcId(1), read: vec![], write: vec![Slot::Global(0)] };
    assert_eq!(replay(ok, &prog), Ok(()));
    let wrong = IpaClaim::Summary { proc: ProcId(1), read: vec![], write: vec![Slot::Local(0)] };
    assert!(replay(wrong, &prog).unwrap_err().contains("disagrees with the claim"));
}

#[test]
fn a_claim_about_an_out_of_range_procedure_is_refused() {
    let prog = leaf_split();
    let err = replay(IpaClaim::Purity { proc: ProcId(99), purity: Purity::Pure }, &prog).unwrap_err();
    assert!(err.contains("out of range"), "{err}");
}

#[test]
fn a_certificate_over_a_malformed_program_is_rejected_before_any_analysis() {
    // main reads local 0 of a procedure that declares no locals.
    let mut main = p("main", &[], &[]);
    main.stmts = vec![assign(Slot::Local(0), konst(1))];
    let prog = IpaProgram::new(vec![], vec![main], ProcId(0));
    let err = replay(IpaClaim::Purity { proc: ProcId(0), purity: Purity::Impure }, &prog)
        .unwrap_err();
    assert!(err.contains("malformed"), "{err}");
}

// --- JSON round-trip -------------------------------------------------------

#[test]
fn a_formal_value_certificate_survives_a_json_round_trip() {
    // This is the case an internally tagged `AbstractValue` could not represent:
    // a newtype variant has no field to hold the tag in, so serde refused to
    // serialize the certificate at all.
    let prog = IpaProgram::new(
        vec![],
        vec![body("main", vec![call(1, vec![konst(5)])]), p("g", &["x"], &[])],
        ProcId(0),
    );
    let claim = IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(5) };
    let l = carrier_loop();
    let cert = interprocedural_certificate(&l, &prog, claim.clone(), "P8").expect("g(5) holds");

    let json = serde_json::to_string(&cert).expect("serializable");
    let back: Certificate = serde_json::from_str(&json).expect("deserializable");
    // Certificate has no PartialEq (a witness carries a whole program), so the
    // round-trip is checked by re-encoding: same bytes in, same bytes out.
    assert_eq!(serde_json::to_string(&back).unwrap(), json);
    // The rebuilt certificate still replays against the rebuilt program.
    match back.claim {
        Claim::Interprocedural(c) => assert_eq!(check_ipa_claim(&c, &prog), Ok(())),
        _ => panic!("the claim kind changed"),
    }
    // The program inside the witness round-trips to the same analysis result.
    match back.witness {
        Witness::IpaEvidence { program } => {
            assert_eq!(*program, prog);
            assert_eq!(check_ipa_claim(&claim, &program), Ok(()));
        }
        _ => panic!("the witness kind changed"),
    }
}

#[test]
fn every_lattice_value_and_jump_function_survives_json() {
    // Adjacently tagged serde means the newtype variant `Const(i64)` has a field
    // to put the tag in; this is the regression guard for that choice.
    let values = [
        AbstractValue::Top,
        AbstractValue::Const(-9_223_372_036_854_775_808),
        AbstractValue::Const(0),
        AbstractValue::Const(i64::MAX),
        AbstractValue::NonConst,
    ];
    for v in values {
        let json = serde_json::to_string(&v).expect("serializable");
        let back: AbstractValue = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(back, v, "{json}");
    }
    let jumps = [
        JumpFn::Const(7),
        JumpFn::Const(i64::MIN),
        JumpFn::NonConst,
        JumpFn::Function { formal: 2, coeff: -3, offset: 11 },
    ];
    for j in jumps {
        let json = serde_json::to_string(&j).expect("serializable");
        let back: JumpFn = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(back, j, "{json}");
    }
}

#[test]
fn every_certificate_shape_round_trips_and_still_replays() {
    let prog = global_writer();
    let l = carrier_loop();
    // Only claims the checker ought to accept; the emitter refuses the rest.
    let claims = vec![
        IpaClaim::Purity { proc: ProcId(1), purity: Purity::Impure },
        IpaClaim::ModSet { proc: ProcId(1), symbols: vec![Slot::Global(0)] },
        IpaClaim::Summary { proc: ProcId(1), read: vec![], write: vec![Slot::Global(0)] },
        // main() { k(0); } — k's formal 0 is Const(0).
        IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(0) },
    ];
    for claim in claims {
        let cert = interprocedural_certificate(&l, &prog, claim.clone(), "P8")
            .unwrap_or_else(|| panic!("checker rejected the claim {claim:?}"));
        let json = serde_json::to_string(&cert).expect("serializable");
        let back: Certificate = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(serde_json::to_string(&back).unwrap(), json, "{claim:?}");
        let restored = match back.claim {
            Claim::Interprocedural(c) => c,
            _ => unreachable!(),
        };
        assert_eq!(check_ipa_claim(&restored, &prog), Ok(()), "{claim:?}");
    }
}

#[test]
fn a_certificate_the_checker_would_reject_is_never_emitted() {
    let prog = global_writer();
    let l = carrier_loop();
    // main is impure, so a Pure claim must not produce a certificate.
    assert!(interprocedural_certificate(
        &l,
        &prog,
        IpaClaim::Purity { proc: ProcId(0), purity: Purity::Pure },
        "P8"
    )
    .is_none());
    // A malformed program produces none either, whatever the claim.
    let mut bad = p("main", &[], &[]);
    bad.stmts = vec![assign(Slot::Local(5), konst(1))];
    let bad = IpaProgram::new(vec![], vec![bad], ProcId(0));
    assert!(interprocedural_certificate(
        &l,
        &bad,
        IpaClaim::Purity { proc: ProcId(0), purity: Purity::Impure },
        "P8"
    )
    .is_none());
}

#[test]
fn a_tampered_certificate_fails_the_replay_after_decoding() {
    let prog = global_writer();
    let l = carrier_loop();
    let claim = IpaClaim::ModSet { proc: ProcId(1), symbols: vec![Slot::Global(0)] };
    let cert = interprocedural_certificate(&l, &prog, claim, "P8").expect("k writes G");
    let mut v: serde_json::Value = serde_json::to_value(&cert).unwrap();
    // Drop the symbol from the claim without touching the carried program.
    v["claim"]["value"]["symbols"] = serde_json::json!([]);
    let tampered: Certificate = serde_json::from_value(v).unwrap();
    let restored = match tampered.claim {
        Claim::Interprocedural(c) => c,
        _ => unreachable!(),
    };
    assert!(check_ipa_claim(&restored, &prog).is_err(), "the checker catches it");
}

#[test]
fn an_ipa_program_round_trips_through_json() {
    for prog in [chain(), io_program(), mutually_recursive(), pure_fn(), leaf_split()] {
        let json = serde_json::to_string(&prog).expect("serializable");
        let back: IpaProgram = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(back, prog);
        assert_eq!(back.validate(), Ok(()));
        // And every analysis agrees on the decoded copy, so the wire format
        // carries the whole program rather than a lossy sketch of it.
        let a = build(&prog);
        let b = build(&back);
        assert_eq!(a.edges, b.edges);
        assert_eq!(classify_purity(&prog, &a), classify_purity(&back, &b));
        assert_eq!(analyze_mod(&prog, &a).mod_sets, analyze_mod(&back, &b).mod_sets);
        assert_eq!(analyze_constprop(&prog, &a, &[]), analyze_constprop(&back, &b, &[]));
    }
}

#[test]
fn a_derived_context_names_the_callees_formals_from_the_call_site() {
    let caller_ctx = ContextKey { proc: ProcId(0), args: vec![ArgShape::Const(7)] };
    let call = Call {
        callee: Callee::Direct(ProcId(1)),
        args: vec![read(Slot::Param(0)), konst(2), IpaExpr::AddrOf(Slot::Global(0))],
        ret: None,
    };
    let ctx = derive_context(&caller_ctx, &call, ProcId(1), 3);
    assert_eq!(ctx.proc, ProcId(1));
    assert_eq!(
        ctx.args,
        vec![ArgShape::Const(7), ArgShape::Const(2), ArgShape::Unknown],
        "an address is a shape the model cannot pin down"
    );
    // Fewer formals than arguments truncates; more pads with Unknown.
    assert_eq!(derive_context(&caller_ctx, &call, ProcId(1), 1).args, vec![ArgShape::Const(7)]);
    assert_eq!(
        derive_context(&caller_ctx, &call, ProcId(1), 5).args[3..],
        [ArgShape::Unknown, ArgShape::Unknown]
    );
}












