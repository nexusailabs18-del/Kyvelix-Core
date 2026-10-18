//! Phase 4 custom verification: Wolfe's ScalarDD (Figure 6.21) over hand-built
//! CFGs. Every fact the analysis reports is re-derived from the φ-annotated CFG
//! and its forward chains, so the expected edges below are hand-computed from
//! the figure's walk, not asserted from a certificate.

use m0::fud::{
    build_fud_chains, enclosing_loops, project_dd_facts, scalar_dd, DdFact, DependenceKind,
    FudChains, Loc, ScalarDD, ScalarDistance,
};
use m0::ir::cfg::{Block, CFG};
use m0::ir::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar};
use m0::ir::Stmt;

fn var(n: &str) -> ScalarVar {
    ScalarVar { name: n.into(), ty: ScalarTy::I64 }
}

/// One statement: uses of `uses`, then defs of `defs`, canonically numbered.
fn st(id: usize, uses: Vec<usize>, defs: Vec<usize>) -> Stmt {
    Stmt {
        id,
        uses: uses
            .into_iter()
            .enumerate()
            .map(|(o, v)| ScalarUse { var: v, stmt: id, order: o })
            .collect(),
        defs: defs
            .into_iter()
            .enumerate()
            .map(|(o, v)| {
                ScalarDef { var: v, stmt: id, order: o, rhs: ScalarRhs::Unknown }
            })
            .collect(),
    }
}

/// A single-loop CFG: entry 0 → header 1 → latch 2 → header 1; header → exit 3.
fn loop_cfg(header_stmts: Vec<Stmt>, latch_stmts: Vec<Stmt>) -> CFG {
    let blocks = vec![
        Block { id: 0, preds: vec![], succs: vec![1], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 1, preds: vec![0, 2], succs: vec![2, 3], stmts: header_stmts, phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 2, preds: vec![1], succs: vec![1], stmts: latch_stmts, phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 3, preds: vec![1], succs: vec![], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
    ];
    let cfg = CFG { vars: vec![var("s")], blocks, entry: 0, exit: 3 };
    cfg.validate().expect("loop CFG is valid");
    cfg
}

fn run(cfg: &CFG) -> ScalarDD {
    let (annot, chains): (CFG, FudChains) = build_fud_chains(cfg);
    scalar_dd(&annot, &chains)
}

fn loc(stmt: Option<usize>, variable: usize, block: usize, is_read: bool) -> Loc {
    Loc { stmt, variable, block, is_phi: false, is_read }
}

fn fact(dep_type: &str, src: Loc, dst: Loc, distance: Vec<&str>, loops: Vec<usize>) -> DdFact {
    DdFact {
        dep_type: dep_type.into(),
        source: src,
        sink: dst,
        distance: distance.into_iter().map(String::from).collect(),
        loops,
    }
}

// ---- Straight-line code: no loops, empty distance vector ------------------

#[test]
fn straight_line_flow_is_loop_free() {
    let cfg = CFG::new_single_block(
        vec![var("x"), var("y")],
        vec![st(0, vec![], vec![0]), st(1, vec![0], vec![1])],
    );
    let dd = run(&cfg);
    assert!(dd.loops.is_empty() && dd.headers.is_empty());
    assert_eq!(
        project_dd_facts(&dd),
        vec![fact("flow", loc(Some(0), 0, 1, false), loc(Some(1), 0, 1, true), vec![], vec![])]
    );
}

#[test]
fn self_assignment_in_line_yields_anti_edge() {
    // `x = x + 1` as the first statement of straight-line code: the use has no
    // reaching definition, but the read-before-write order inside the statement
    // is an anti dependence the pairwise pass must find.
    let dd = run(&CFG::new_single_block(vec![var("x")], vec![st(0, vec![0], vec![0])]));
    let facts = project_dd_facts(&dd);
    assert!(
        facts.iter().any(|f| f.dep_type == "anti" && f.source.is_read && !f.sink.is_read),
        "expected an anti edge, got {facts:?}"
    );
}

// ---- A loop: s = s + 1 in the header -------------------------------------

#[test]
fn loop_carried_flow_has_distance_one() {
    let cfg = loop_cfg(vec![st(0, vec![0], vec![0])], vec![]);
    let dd = run(&cfg);
    assert_eq!(dd.headers.len(), 1);
    assert_eq!(dd.loops[0].header, 1);
    // `s` is merged by the header φ; the only definition the loop feeds back is
    // its own, met through Reaching[h] and not directly: situation (1).
    assert_eq!(dd.situations.values().copied().collect::<Vec<_>>(), vec![ScalarDistance::One]);
    let facts = project_dd_facts(&dd);
    assert!(
        facts.iter().any(|f| f.dep_type == "flow"
            && f.distance == vec!["1".to_string()]
            && f.loops == vec![1]),
        "expected a loop-carried flow edge, got {facts:?}"
    );
}

#[test]
fn same_iteration_def_use_is_distance_zero() {
    // Header: stmt 0 defines t; stmt 1 reads t and writes z. Whatever value t
    // holds at the read is written in the same pass, so the flow edge is
    // distance (0) even inside a loop.
    let mut cfg = loop_cfg(vec![], vec![]);
    cfg.vars.push(var("t"));
    cfg.vars.push(var("z"));
    cfg.blocks[1].stmts = vec![st(0, vec![], vec![1]), st(1, vec![1], vec![2])];
    let facts = project_dd_facts(&run(&cfg));
    let flow = facts.iter().find(|f| f.dep_type == "flow").expect("a flow edge");
    assert_eq!(flow.distance, vec!["0"], "all facts: {facts:?}");
}

// ---- φ merge and intervening write kill -----------------------------------

#[test]
fn a_phi_merge_carries_the_source_value_instead_of_killing_it() {
    // Entry 0 → A(1) → header(2) → latch(3) → header; header → exit(4). A defines
    // `s` in block 1; the loop reads it through the header φ. The φ is a *merge*,
    // not a write that overwrites the reaching definition, so the flow edge from
    // block 1's def into the loop body survives crossing the φ — the whole point of
    // a loop-header φ in the chain graph.
    let blocks = vec![
        Block { id: 0, preds: vec![], succs: vec![1], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 1, preds: vec![0], succs: vec![2], stmts: vec![st(0, vec![], vec![0])], phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 2, preds: vec![1, 3], succs: vec![3, 4], stmts: vec![st(0, vec![0], vec![1])], phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 3, preds: vec![2], succs: vec![2], stmts: vec![st(0, vec![1], vec![0])], phi: vec![], upsilon: vec![], guards: vec![] },
        Block { id: 4, preds: vec![2], succs: vec![], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
    ];
    let cfg = CFG { vars: vec![var("s"), var("t")], blocks, entry: 0, exit: 4 };
    cfg.validate().expect("phi-merge CFG is valid");
    let facts = project_dd_facts(&run(&cfg));
    // Block 3 redefines `s` and reads it back through the header φ each trip: a
    // loop-carried flow edge, distance 1 around loop 2, must be reported.
    assert!(
        facts.iter().any(|f| f.dep_type == "flow" && f.distance == vec!["1"] && f.loops == vec![2]),
        "the φ merge must carry a loop-carried flow edge, got {facts:?}"
    );
}

#[test]
fn an_intervening_write_makes_a_later_read_depend_on_it_not_the_earlier_def() {
    // Straight-line: d0 = s (block 1 stmt 0); a second write to the same variable
    // (block 1 stmt 1); a read (block 1 stmt 2). The read depends on the *nearest*
    // reaching def, so exactly one flow edge — the second one — is loop-independent,
    // and there is no direct flow edge from the first def to the read.
    let dd = run(&CFG::new_single_block(
        vec![var("s")],
        vec![st(0, vec![], vec![0]), st(1, vec![], vec![0]), st(2, vec![0], vec![0])],
    ));
    let facts = project_dd_facts(&dd);
    let flows: Vec<&DdFact> = facts.iter().filter(|f| f.dep_type == "flow").collect();
    for f in &flows {
        assert!(
            f.source.stmt != Some(0) || f.sink.stmt == Some(1),
            "def at stmt 0 must not reach a use past the intervening write at stmt 1: {f:?}"
        );
    }
    assert!(
        flows.iter().any(|f| f.source.stmt == Some(1) && f.sink.stmt == Some(2)),
        "the intervening write must be what the later read depends on, got {flows:?}"
    );
}

// ---- Determinism ----------------------------------------------------------

#[test]
fn projection_is_deterministic_across_runs() {
    let cfg = loop_cfg(vec![st(0, vec![0], vec![0])], vec![]);
    let a = project_dd_facts(&run(&cfg));
    let b = project_dd_facts(&run(&cfg));
    assert_eq!(a, b);
    assert!(a
        .windows(2)
        .all(|w| (&w[0].source, &w[0].sink) <= (&w[1].source, &w[1].sink)));
}

#[test]
fn edge_kinds_project_onto_matching_dep_types() {
    // Two uses (or two defs) of one variable inside a single statement share a
    // `Loc`, so several edges can collapse onto one fact: the projection is a
    // subset of the edges, not a copy of them.
    let dd = run(&CFG::new_single_block(
        vec![var("x")],
        vec![st(0, vec![], vec![0]), st(1, vec![0, 0], vec![0])],
    ));
    let facts = project_dd_facts(&dd);
    assert!(facts.len() <= dd.edges.len());
    for f in &facts {
        assert!(
            dd.edges.iter().any(|e| {
                e.kind.as_str() == f.dep_type && e.src == f.source && e.dst == f.sink
            }),
            "fact {f:?} has no supporting edge"
        );
    }
    assert!(dd.edges.iter().all(|e| facts.iter().any(|f| f.dep_type == e.kind.as_str())));
    assert!(facts.iter().any(|f| f.dep_type == DependenceKind::Flow.as_str()));
}


// ---- Fuzz: every reported fact is well-formed and stable -------------------

/// Small xorshift64 PRNG (same construction as the in-crate `testgen`).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
}

/// Random valid CFG with `3..=max_blocks` blocks and back edges (loops).
/// Mirrors `testgen::gen_cfg_with(.., allow_back_edges = true)`: entry 0 is
/// predecessor-free, exit n-1 successor-free, every block reachable and
/// co-reachable.
fn gen_cfg(rng: &mut Rng, max_blocks: usize, max_stmts: usize) -> CFG {
    let n = 3 + rng.below((max_blocks - 3 + 1) as u64) as usize;
    let mut succs: Vec<Vec<usize>> = vec![vec![]; n];
    let mut preds: Vec<Vec<usize>> = vec![vec![]; n];
    for i in 1..n {
        let p = rng.below(i as u64) as usize;
        succs[p].push(i);
        preds[i].push(p);
    }
    for i in 0..n - 1 {
        let s = i + 1 + rng.below((n - 1 - i) as u64) as usize;
        if !succs[i].contains(&s) {
            succs[i].push(s);
            preds[s].push(i);
        }
    }
    for i in 0..n {
        for j in i + 1..n {
            if rng.chance(1, 4) && !succs[i].contains(&j) {
                succs[i].push(j);
                preds[j].push(i);
            }
        }
    }
    for i in 1..n - 1 {
        if rng.chance(1, 6) {
            let j = 1 + rng.below(i as u64) as usize;
            if !succs[i].contains(&j) {
                succs[i].push(j);
                preds[j].push(i);
            }
        }
    }
    for v in succs.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    for v in preds.iter_mut() {
        v.sort_unstable();
        v.dedup();
    }
    let nv = 1 + rng.below(3) as usize;
    let vars: Vec<ScalarVar> =
        (0..nv).map(|i| ScalarVar { name: format!("v{i}"), ty: ScalarTy::I64 }).collect();
    let blocks = (0..n)
        .map(|b| {
            let ns = rng.below((max_stmts + 1) as u64) as usize;
            let stmts = (0..ns)
                .map(|s| {
                    let nu = rng.below(3) as usize;
                    let nd = rng.below(2) as usize;
                    let uses: Vec<usize> = (0..nu).map(|_| rng.below(nv as u64) as usize).collect();
                    let defs: Vec<usize> = (0..nd).map(|_| rng.below(nv as u64) as usize).collect();
                    st(s, uses, defs)
                })
                .collect();
            Block {
                id: b,
                preds: preds[b].clone(),
                succs: succs[b].clone(),
                stmts,
                phi: vec![],
                upsilon: vec![],
                guards: vec![],
            }
        })
        .collect();
    CFG { vars, blocks, entry: 0, exit: n - 1 }
}


/// Every loop enclosing either endpoint, outermost first — the nest the
/// distance vector and the loops field of a fact must both describe.
fn union_nest(loops: &[m0::fud::LoopInfo], a: usize, b: usize) -> Vec<usize> {
    let mut v = enclosing_loops(loops, b);
    for h in enclosing_loops(loops, a) {
        if !v.contains(&h) {
            v.push(h);
        }
    }
    let size = |h: &usize| loops.iter().find(|l| l.header == *h).map_or(0, |l| l.body.len());
    v.sort_by_key(size);
    v
}

#[test]
fn facts_are_wellformed_and_deterministic_over_random_cfgs() {
    let mut checked_loopy = 0usize;
    for seed in 1..=400u64 {
        let cfg = gen_cfg(&mut Rng::new(seed), 6, 4);
        cfg.validate().expect("generator produces valid CFGs");
        let (annot, chains) = build_fud_chains(&cfg);
        let dd = scalar_dd(&annot, &chains);
        let facts = project_dd_facts(&dd);
        // Determinism: the projection depends on nothing but the CFG.
        assert_eq!(facts, project_dd_facts(&scalar_dd(&annot, &chains)), "seed {seed}");
        if !dd.headers.is_empty() {
            checked_loopy += 1;
        }
        for f in &facts {
            // The read/write polarity is exactly the dependence kind.
            let ok = match f.dep_type.as_str() {
                "flow" => !f.source.is_read && f.sink.is_read,
                "anti" => f.source.is_read && !f.sink.is_read,
                "output" => !f.source.is_read && !f.sink.is_read,
                "input" => f.source.is_read && f.sink.is_read,
                _ => false,
            };
            assert!(ok, "seed {seed}: bad kind {f:?}");
            // A fact's source and sink touch the same scalar.
            assert_eq!(f.source.variable, f.sink.variable, "seed {seed}: {f:?}");
            // One distance entry per loop shared by the endpoints, outermost
            // first, and the loops field names exactly those headers.
            let nest = union_nest(&dd.loops, f.source.block, f.sink.block);
            assert_eq!(f.distance.len(), nest.len(), "seed {seed}: {f:?}");
            assert_eq!(f.loops, nest, "seed {seed}: loops of {f:?}");
            for h in &f.loops {
                assert!(dd.headers.contains(h), "seed {seed}: {h} is not a header");
            }
            // A loop-free fact carries no distances.
            if f.loops.is_empty() {
                assert!(f.distance.is_empty(), "seed {seed}: {f:?}");
            }
        }
        // No two facts share a full key: the projection is a set of edges.
        let mut keys: Vec<(&str, &Loc, &Loc, &[String], &[usize])> = facts
            .iter()
            .map(|f| (
                f.dep_type.as_str(),
                &f.source,
                &f.sink,
                f.distance.as_slice(),
                f.loops.as_slice(),
            ))
            .collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "seed {seed}: duplicate facts");
    }
    assert!(checked_loopy > 0, "generator produced no loop-y CFG");
    println!("{checked_loopy}/400 seeds produced loops");
}
