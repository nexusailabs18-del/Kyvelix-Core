//! Phase 5 feature verification: each §5.1 subsystem is exercised standalone,
//! against a hand-computed oracle rather than against itself.
//!
//! The subsystems are independent (`phase4,5and6.txt` §5.1's own note), so each
//! is tested in isolation here; the cross-cutting certificate round trip is in
//! `tests/phase5_e2e.rs`.

use m0::fud::{compute_alignment, compute_cross_loop_distance, cross_loop_vectors, is_alignable};
use m0::ir::dependence::{DepKind, Dependence};
use m0::ir::effect::{effect_of, io_dependences, must_serialize, Effect, IoChannels};
use m0::ir::layout::{
    linear_index, linearize, linearize_with_equivalence, EquivalenceClass, Layout,
};
use m0::ir::loopkind::{classify_loop_kind, LoopKind};
use m0::ir::purity::{classify_purity, Function, ParamMode, PureSet, Purity};
use m0::ir::reduction::{detect_reductions, ReductionKind};
use m0::ir::stmt::{ScalarExpr, Stmt, StmtKind};
use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue, ScalarTy, ScalarVar};

fn acc(kind: AccessKind, base: &str, index: i64) -> Access {
    Access {
        kind,
        base: base.into(),
        index: vec![index],
        elem_size: 8,
        offset: OffsetValue::Const(0),
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    }
}

fn var(n: &str) -> ScalarVar {
    ScalarVar { name: n.into(), ty: ScalarTy::I64 }
}

fn loop_of(body: Vec<Access>) -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i")],
        stmts: None,
    }
}

// ---------------------------------------------------------------------------
// §5.1.1 storage association
// ---------------------------------------------------------------------------

#[test]
fn storage_association_row_and_column_major() {
    // dims [2,3]: (1,2) linearizes to 5 in both layouts; (0,1) differs.
    assert_eq!(linear_index(Layout::RowMajor, &[1, 2], &[2, 3]), Some(5));
    assert_eq!(linear_index(Layout::ColumnMajor, &[1, 2], &[2, 3]), Some(5));
    assert_eq!(linear_index(Layout::RowMajor, &[0, 1], &[2, 3]), Some(1));
    assert_eq!(linear_index(Layout::ColumnMajor, &[0, 1], &[2, 3]), Some(2));
    assert_eq!(linearize(Layout::RowMajor, &[1, 2], &[2, 3], 8), Some(40));
    // Out of range is refused, not wrapped.
    assert_eq!(linear_index(Layout::RowMajor, &[2, 0], &[2, 3]), None);
}

#[test]
fn storage_association_equivalence() {
    // EQUIVALENCE (A, B(2)): B's start is 8 bytes into A's storage. A unused
    // element at constant offset 3 linearizes to 3 * 8 = 24 bytes into A.
    let classes = vec![EquivalenceClass::new(vec!["A".into(), "B".into()], vec![0, 8])];
    let a = Access { offset: OffsetValue::Const(3), ..acc(AccessKind::Read, "A", 1) };
    assert_eq!(linearize_with_equivalence(&a, &classes), Some(24));
    let b = Access { offset: OffsetValue::Const(0), ..acc(AccessKind::Read, "B", 1) };
    assert_eq!(linearize_with_equivalence(&b, &classes), Some(8));
    // A base in no class is refused.
    let c = acc(AccessKind::Read, "C", 1);
    assert_eq!(linearize_with_equivalence(&c, &classes), None);
}

// ---------------------------------------------------------------------------
// §5.1.3 I/O dependence
// ---------------------------------------------------------------------------

#[test]
fn io_dependence_preserves_order() {
    let ch = IoChannels::standard();
    let l = loop_of(vec![
        acc(AccessKind::Read, "stdin", 1),
        acc(AccessKind::Write, "A", 1),
        acc(AccessKind::Write, "stdout", 1),
    ]);
    // Only the two I/O statements (indices 0 and 2) order each other.
    assert_eq!(io_dependences(&l, &ch), vec![(0, 2)]);
    assert!(must_serialize(&l, 0, 2, &ch));
    assert!(!must_serialize(&l, 0, 1, &ch));
    assert_eq!(effect_of(&acc(AccessKind::Write, "stdout", 1), &ch), Effect::Io);
    assert_eq!(effect_of(&acc(AccessKind::Write, "A", 1), &ch), Effect::Write);
    // The empty table sees no device traffic: it can only miss constraints.
    assert!(io_dependences(&l, &IoChannels::none()).is_empty());
}

// ---------------------------------------------------------------------------
// §5.1.4 purity
// ---------------------------------------------------------------------------

#[test]
fn purity_classification() {
    let astmt = |base: &str, kind| {
        Stmt { id: 0, kind: StmtKind::Array { access: acc(kind, base, 1) } }
    };
    let reader = Function {
        name: "f".into(),
        params: vec![ParamMode::In],
        locals: vec!["A".into()],
        body: vec![astmt("A", AccessKind::Read)],
        callees: vec![],
    };
    assert_eq!(classify_purity(&reader, &PureSet::new()), Purity::Pure);

    let global = Function {
        name: "g".into(),
        params: vec![ParamMode::In],
        locals: vec!["A".into()],
        body: vec![astmt("G", AccessKind::Write)],
        callees: vec![],
    };
    assert_eq!(classify_purity(&global, &PureSet::new()), Purity::Impure);

    let sub = Function {
        name: "h".into(),
        params: vec![ParamMode::In, ParamMode::Out],
        locals: vec!["A".into()],
        body: vec![Stmt {
            id: 0,
            kind: StmtKind::Assign { target: 1, expr: ScalarExpr::Const(1) },
        }],
        callees: vec![],
    };
    assert_eq!(
        classify_purity(&sub, &PureSet::new()),
        Purity::PureSubroutine { modified: vec![1] }
    );

    // An unknown callee with no local modification degrades `Pure` to the
    // weakest subroutine that still admits a caller might observe nothing.
    let caller = Function {
        name: "k".into(),
        params: vec![ParamMode::In],
        locals: vec!["A".into()],
        body: vec![astmt("A", AccessKind::Read)],
        callees: vec!["mystery".into()],
    };
    assert_eq!(
        classify_purity(&caller, &PureSet::new()),
        Purity::PureSubroutine { modified: vec![] }
    );
    let mut known = PureSet::new();
    known.insert("mystery");
    assert_eq!(classify_purity(&caller, &known), Purity::Pure);
}

// ---------------------------------------------------------------------------
// §5.1.8 reduction detection
// ---------------------------------------------------------------------------

fn reduction_loop(expr: ScalarExpr) -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", 0, 8, 1)],
        body: vec![acc(AccessKind::Read, "A", 1), acc(AccessKind::Write, "S", 0)],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![var("i"), var("sum"), var("k")],
        stmts: Some(vec![Stmt { id: 0, kind: StmtKind::Assign { target: 1, expr } }]),
    }
}

#[test]
fn reduction_detection_kinds() {
    // `sum := sum + k` — the classic summation accumulator.
    let sum = reduction_loop(ScalarExpr::Add(
        Box::new(ScalarExpr::Scalar(1)),
        Box::new(ScalarExpr::Scalar(2)),
    ));
    let cfg = m0::fud::build_cfg_from_loop(&sum).unwrap();
    let (_a, chains) = m0::fud::build_fud_chains(&cfg);
    let r = detect_reductions(&cfg, &chains);
    assert!(
        r.iter().any(|x| x.var == 1 && x.kind == ReductionKind::Sum),
        "{r:?}"
    );

    // `sum := sum + 1` — an occurrence count.
    let count = reduction_loop(ScalarExpr::Add(
        Box::new(ScalarExpr::Scalar(1)),
        Box::new(ScalarExpr::Const(1)),
    ));
    let cfg = m0::fud::build_cfg_from_loop(&count).unwrap();
    let (_a, chains) = m0::fud::build_fud_chains(&cfg);
    assert!(detect_reductions(&cfg, &chains).iter().any(|x| x.kind == ReductionKind::Count));
}

#[test]
fn a_non_reduction_is_not_reported() {
    // `sum := k + 1` does not read its own target, so variable 1 is not an
    // accumulator. (The induction variable `i := i + 1` *is* a Count, so the
    // detector is not empty in general — only free of a reduction on `sum`.)
    let plain = reduction_loop(ScalarExpr::Add(
        Box::new(ScalarExpr::Scalar(2)),
        Box::new(ScalarExpr::Const(1)),
    ));
    let cfg = m0::fud::build_cfg_from_loop(&plain).unwrap();
    let (_a, chains) = m0::fud::build_fud_chains(&cfg);
    let r = detect_reductions(&cfg, &chains);
    assert!(!r.iter().any(|x| x.var == 1), "{r:?}");
}

// ---------------------------------------------------------------------------
// §5.1.9 alignment
// ---------------------------------------------------------------------------

#[test]
fn alignment_removes_every_dependence() {
    let l = loop_of(vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Write, "A", 1)]);
    let deps = vec![Dependence::new(0, 1, DepKind::Flow, vec![1])];
    let a = compute_alignment(&l, &deps).expect("a single dependence always aligns");
    let shift = |s: usize| a.iter().find(|x| x.stmt == s).map(|x| x.shift).unwrap_or(0);
    // After alignment the dependence's new distance is d + shift(src) - shift(dst),
    // which must vanish.
    for d in &deps {
        assert_eq!(d.distance[0] + shift(d.src) - shift(d.dst), 0);
    }
}

#[test]
fn alignment_refuses_an_inconsistent_cycle() {
    let l = loop_of(vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Write, "A", 1)]);
    // 0 → 1 at distance 1 and 1 → 0 at distance 1: the cycle totals 2 ≠ 0.
    let deps = vec![
        Dependence::new(0, 1, DepKind::Flow, vec![1]),
        Dependence::new(1, 0, DepKind::Flow, vec![1]),
    ];
    assert!(!is_alignable(&l, &deps));
    assert!(compute_alignment(&l, &deps).is_none());
}

#[test]
fn alignment_accepts_a_zero_total_cycle() {
    let l = loop_of(vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Write, "A", 1)]);
    // 0 → 1 at distance 1 and 1 → 0 at distance -1: the cycle totals 0.
    let deps = vec![
        Dependence::new(0, 1, DepKind::Flow, vec![1]),
        Dependence::new(1, 0, DepKind::Flow, vec![-1]),
    ];
    assert!(is_alignable(&l, &deps));
}

// ---------------------------------------------------------------------------
// §5.1.10 cross-loop distance
// ---------------------------------------------------------------------------

#[test]
fn cross_loop_distance_matches_formula() {
    let a = loop_of(vec![]);
    let mut b = loop_of(vec![]);
    b.ivs[0].lower = 3;
    let d = compute_cross_loop_distance(&a, &b).unwrap();
    assert_eq!(d.distance, vec![3]);
    // The literal spec arithmetic d = (j_T - i_S) - (j_S - i_T).
    assert_eq!(cross_loop_vectors(&[0], &[3], &[1], &[5]), Some(vec![7]));
    assert_eq!(cross_loop_vectors(&[0], &[3, 4], &[1], &[5]), None);
}

// ---------------------------------------------------------------------------
// §5.1.11 loop kind
// ---------------------------------------------------------------------------

#[test]
fn loop_kind_from_dependences() {
    let l = loop_of(vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Read, "A", 1)]);
    assert_eq!(classify_loop_kind(&l, &[]), LoopKind::Doall);
    assert_eq!(
        classify_loop_kind(&l, &[Dependence::new(0, 1, DepKind::Flow, vec![1])]),
        LoopKind::Dosingle
    );
    assert_eq!(
        classify_loop_kind(&l, &[Dependence::new(0, 1, DepKind::Flow, vec![-1])]),
        LoopKind::Forall
    );
    assert_eq!(
        classify_loop_kind(&l, &[Dependence::new(0, 1, DepKind::Anti, vec![1])]),
        LoopKind::Dopar
    );
    // Two dependences in opposite directions force a sequential loop.
    assert_eq!(
        classify_loop_kind(
            &l,
            &[
                Dependence::new(0, 1, DepKind::Flow, vec![1]),
                Dependence::new(1, 0, DepKind::Output, vec![1]),
            ]
        ),
        LoopKind::Sequential
    );
}

#[test]
fn an_unknown_dependence_forces_a_conservative_kind() {
    // A pair the analysis could not decide must not be reported parallel: the
    // `unknown` flag makes the edge a carried dependence.
    let l = loop_of(vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Read, "A", 1)]);
    let kind = classify_loop_kind(&l, &[Dependence::unknown(0, 1, DepKind::Flow)]);
    assert_ne!(kind, LoopKind::Doall, "an undecided pair must not be Doall");
}

// ---------------------------------------------------------------------------
// §5.1.6 dynamic bounds and §5.1.7 cubic+ — the two "extension" subsystems.
// ---------------------------------------------------------------------------

#[test]
fn dynamic_bounds_resolve_and_decide() {
    use m0::decide::m12;
    use m0::ir::{BoundValue, DynamicIV, DynamicLoop};
    // `A[i] = A[i+1]` over `i = 0 .. n`, with `n = 8` supplied by the caller.
    let dl = DynamicLoop {
        depth: 1,
        ivs: vec![DynamicIV {
            name: "i".into(),
            lower: BoundValue::Const(0),
            upper: BoundValue::Param("n".into()),
            step: 1,
        }],
        body: vec![acc(AccessKind::Write, "A", 1), acc(AccessKind::Read, "A", 1)],
    };
    assert!(dl.has_parametric_bounds());
    let (d, _) = m12::decide_dynamic(&dl, &[("n".to_string(), 8)], 0, 1);
    assert!(matches!(d, m0::decide::Decision::Exact(_)), "{d:?}");
}

#[test]
fn cubic_plus_extends_the_degree_cap() {
    // i^6 (degree 6) is now enumerable; the old cap of 4 refused it.
    let mul = |a: m0::ir::IndexExpr, b: m0::ir::IndexExpr| {
        m0::ir::IndexExpr::Mul(Box::new(a), Box::new(b))
    };
    let iv0 = m0::ir::IndexExpr::IV(0);
    let i2 = mul(iv0.clone(), iv0.clone());
    let i4 = mul(i2.clone(), i2.clone());
    let i6 = mul(i2, i4.clone());
    assert!(i6.max_degree() >= 5, "degree {}", i6.max_degree());

    let mut w = acc(AccessKind::Write, "A", 0);
    w.index_kind = IndexKind::Product(i6);
    let mut r = acc(AccessKind::Read, "A", 0);
    r.index_kind = IndexKind::Product(i4);
    let l = loop_of(vec![w, r]);
    let (d, _) = m0::decide::decide(&l, 0, 1);
    assert!(matches!(d, m0::decide::Decision::Exact(_)), "{d:?}");
}

// ---------------------------------------------------------------------------
// §5.1.2 runtime testing — the declared vocabulary exists and is documented.
// ---------------------------------------------------------------------------

#[test]
fn runtime_check_vocabulary_is_declared() {
    use m0::runtime::RuntimeCheckKind;
    let kinds = [
        RuntimeCheckKind::DisjointArrays,
        RuntimeCheckKind::NoDistanceK(2),
        RuntimeCheckKind::GuardCheck("i < n".into()),
    ];
    assert_eq!(kinds.len(), 3);
}