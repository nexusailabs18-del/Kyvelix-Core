//! Phase 7 feature tests — the §7.2–§7.8 APIs, exercised directly.
//!
//! These are unit-level checks of the public surface: the points-to graph and
//! its transfer functions, the fixpoint, the alias query against its
//! brute-force oracle, dope-vector association, pointer reconstruction,
//! allocation dependence, and the certificate replay. They complement
//! `phase7_fuzz.rs` (differential) and `phase7_e2e.rs` (the real checker).

use m0::points::alias::{arrays_disjoint, enumerate_alias, Alias};
use m0::points::allocdep::{
    allocation_dependence, escaping_vars, AllocDependence, AllocStmt, Language,
};
use m0::points::certificate::{check_alias_claim, points_to, AliasBasis, AliasClaim};
use m0::points::dopvec::{associate, Association, Dim, DopeVector};
use m0::points::fixpoint::{analyze, PtrBlock, PtrDecl, PtrProgram};
use m0::points::ptrarith::{reconstruct, ArrayTable, PtrStep, ReconstructedRef};
use m0::points::relation::{Definiteness, PointsToGraph, VarId};
use m0::points::transfer::{apply_effect, effect_of_pointer_copy, Effect};

fn v(n: u32) -> VarId {
    VarId(n)
}

fn one_block(decls: Vec<PtrDecl>, stmts: Vec<Effect>) -> PtrProgram {
    PtrProgram::new(decls, 0, vec![PtrBlock::new(0, vec![], stmts)])
}

// ---------------------------------------------------------------------------
// §7.2 — the relation and the graph
// ---------------------------------------------------------------------------

#[test]
fn definiteness_join_is_definite_only_when_both_agree() {
    assert_eq!(Definiteness::Definite.join(Definiteness::Definite), Definiteness::Definite);
    assert_eq!(Definiteness::Definite.join(Definiteness::Possible), Definiteness::Possible);
    assert_eq!(Definiteness::Possible.join(Definiteness::Definite), Definiteness::Possible);
}

#[test]
fn the_graph_keeps_both_indexes_in_sync() {
    let mut g = PointsToGraph::empty();
    g.add_relation(v(0), v(1), Definiteness::Definite);
    g.add_relation(v(2), v(1), Definiteness::Possible);
    let ps = g.pointers_of(v(1)).unwrap();
    assert_eq!(ps.len(), 2);
    assert!(ps.contains(&(v(0), Definiteness::Definite)));
    assert!(ps.contains(&(v(2), Definiteness::Possible)));
    g.kill_targets_of(v(0));
    assert_eq!(g.pointers_of(v(1)).unwrap().len(), 1);
}

#[test]
fn must_point_to_needs_a_single_definite_target() {
    let mut g = PointsToGraph::empty();
    g.add_relation(v(1), v(10), Definiteness::Definite);
    assert!(g.must_point_to(v(1), v(10)));
    g.add_relation(v(1), v(11), Definiteness::Possible);
    assert!(!g.must_point_to(v(1), v(10)));
    assert!(g.may_point_to(v(1), v(11)));
}

#[test]
fn meet_shrinks_a_target_a_predecessor_did_not_establish() {
    let mut a = PointsToGraph::empty();
    a.add_relation(v(0), v(1), Definiteness::Definite);
    let mut b = PointsToGraph::empty();
    b.add_relation(v(0), v(2), Definiteness::Definite);
    a.meet_with(&b);
    assert!(a.may_point_to(v(0), v(1)));
    assert!(!a.must_point_to(v(0), v(1)));
    assert!(a.may_point_to(v(0), v(2)));
}

// ---------------------------------------------------------------------------
// §7.3 — the four transfer cases
// ---------------------------------------------------------------------------

#[test]
fn transfer_covers_all_four_wolfe_cases_and_the_two_extras() {
    let mut g = PointsToGraph::empty();
    apply_effect(&mut g, &Effect::DefiniteLhs { lhs: v(0), rhs: v(1) });
    assert!(g.must_point_to(v(0), v(1)));
    apply_effect(&mut g, &Effect::AddressOf { lhs: v(2), target: v(5) });
    assert!(g.must_point_to(v(2), v(5)));
    apply_effect(&mut g, &Effect::Kill { lhs: v(0) });
    assert!(!g.may_point_to(v(0), v(1)));
    apply_effect(&mut g, &Effect::Clobber);
    assert!(g.may_be_unknown(v(2)));
}

#[test]
fn pointer_copy_case_is_chosen_by_the_rhs_definiteness() {
    let mut g = PointsToGraph::empty();
    assert!(matches!(
        effect_of_pointer_copy(&g, v(0), v(1)),
        Effect::PossibleLhs { .. }
    ));
    g.add_relation(v(1), v(9), Definiteness::Definite);
    let e = effect_of_pointer_copy(&g, v(0), v(1));
    apply_effect(&mut g, &e);
    assert!(g.must_point_to(v(0), v(9)));
}

// ---------------------------------------------------------------------------
// §7.4 — the fixpoint
// ---------------------------------------------------------------------------

#[test]
fn fixpoint_finds_a_definite_target_on_a_straight_line() {
    let p = one_block(
        vec![PtrDecl::target("tx"), PtrDecl::pointer("p")],
        vec![Effect::AddressOf { lhs: v(1), target: v(0) }],
    );
    let a = analyze(&p).unwrap();
    assert!(a.out_of(0).unwrap().must_point_to(v(1), v(0)));
}

#[test]
fn fixpoint_weakens_at_a_join_with_a_kill_path() {
    let p = PtrProgram::new(
        vec![PtrDecl::target("tx"), PtrDecl::pointer("p")],
        0,
        vec![
            PtrBlock::new(0, vec![], vec![Effect::AddressOf { lhs: v(1), target: v(0) }]),
            PtrBlock::new(1, vec![], vec![Effect::Kill { lhs: v(1) }]),
            PtrBlock::new(2, vec![0, 1], vec![]),
        ],
    );
    let a = analyze(&p).unwrap();
    let g = a.out_of(2).unwrap();
    assert!(!g.must_point_to(v(1), v(0)));
    assert!(g.may_point_to(v(1), v(0)));
}

// ---------------------------------------------------------------------------
// §7.8 — the alias query against its oracle
// ---------------------------------------------------------------------------

fn r(array: u32, coeff: i64, offset: i64) -> ReconstructedRef {
    ReconstructedRef { array: v(array), coeff, offset, elem_size: 8, pointer: v(99) }
}

#[test]
fn alias_query_and_oracle_agree_on_constant_references() {
    let pairs = [
        (r(1, 0, 0), r(1, 0, 1)),
        (r(1, 0, 2), r(1, 0, 2)),
        (r(1, 1, 0), r(2, 1, 0)),
    ];
    for (a, b) in pairs {
        let alg = m0::points::alias::query_refs(&a, &b);
        let brute = enumerate_alias(&a, &b, 0, 8);
        assert_eq!(alg, brute, "algebraic and enumerated answers differ");
        if arrays_disjoint(&a, &b) {
            assert_eq!(brute, Alias::NoAlias);
        }
    }
}

// ---------------------------------------------------------------------------
// §7.5 — dope vectors
// ---------------------------------------------------------------------------

#[test]
fn dope_vector_offsets_and_association() {
    let a = DopeVector::row_major(v(0), 8, &[2, 3]);
    assert_eq!(a.rank(), 2);
    assert_eq!(a.offset_of(&[2, 3]), Some(5));
    assert_eq!(a.offset_of(&[9, 9]), None);

    let head = DopeVector::with_base_offset(v(0), 8, 0, vec![Dim::new(1, 8, 1)]);
    let tail = DopeVector::with_base_offset(v(0), 8, 8, vec![Dim::new(1, 4, 1)]);
    assert_eq!(associate(&head, &tail), Association::Disjoint);
    assert_eq!(associate(&head, &head.clone()), Association::Identical);
}

// ---------------------------------------------------------------------------
// §7.6 — pointer arithmetic reconstruction
// ---------------------------------------------------------------------------

#[test]
fn reconstruction_validates_the_element_size() {
    let arrays = ArrayTable::of(&[(v(0), 8)]);
    assert!(reconstruct(&PtrStep::new(v(1), v(0), 1, 0, 8, 0), &arrays).is_some());
    assert!(reconstruct(&PtrStep::new(v(1), v(0), 1, 0, 4, 0), &arrays).is_none());
    assert!(reconstruct(&PtrStep::new(v(1), v(9), 1, 0, 8, 0), &arrays).is_none());
}

// ---------------------------------------------------------------------------
// §7.7 — allocation dependence
// ---------------------------------------------------------------------------

#[test]
fn allocation_order_is_free_in_fortran_and_observable_in_c() {
    let mut g = PointsToGraph::empty();
    g.add_relation(v(1), g.unknown, Definiteness::Possible);
    let esc = escaping_vars(&g);
    let a1 = AllocStmt::new(v(1), 0);
    let a2 = AllocStmt::new(v(2), 1);
    assert_eq!(
        allocation_dependence(Language::Fortran90, &a1, &a2, &esc),
        AllocDependence::None
    );
    assert_eq!(
        allocation_dependence(Language::C, &a1, &a2, &esc),
        AllocDependence::LoopIndependent
    );
}

// ---------------------------------------------------------------------------
// §7.8 — the certificate replay
// ---------------------------------------------------------------------------

#[test]
fn certificate_replay_accepts_and_rejects() {
    let p = one_block(
        vec![PtrDecl::target("tx"), PtrDecl::target("ty"), PtrDecl::pointer("p")],
        vec![Effect::AddressOf { lhs: v(2), target: v(0) }],
    );
    let ok = AliasClaim::MustAlias { pointer: v(2), target: v(0), block: 0 };
    assert!(check_alias_claim(&ok, &p, &points_to(0)).is_ok());
    let bad = AliasClaim::MustAlias { pointer: v(2), target: v(1), block: 0 };
    assert!(check_alias_claim(&bad, &p, &points_to(0)).is_err());
    let basis = AliasBasis::AllocationOrder {
        language: Language::Fortran90,
        first: AllocStmt::new(v(2), 0),
        second: AllocStmt::new(v(2), 0),
    };
    assert!(check_alias_claim(&ok, &p, &basis).is_err());
}
