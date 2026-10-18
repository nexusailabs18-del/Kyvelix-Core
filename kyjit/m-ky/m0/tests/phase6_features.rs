//! Phase 6 feature verification: the four §6.1 subsystems run standalone, each
//! against an exact oracle (enumeration) rather than against itself.
//!
//! The cross-cutting certificate round trip is in `tests/phase6_e2e.rs`.

use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue};
use m0::region::{
    access_matrix, dataflow, fullness, is_invariant_write, is_self_conflict_free, killed_region,
    kills, locations_of_access, loop_summary, rank, region_of_access, Fullness, Region,
    RegionProgram, RegionSet, Triplet, TripletRegion,
};

fn loop1(lo: i64, hi: i64, step: i64) -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", lo, hi, step)],
        body: vec![],
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    }
}

fn aff(kind: AccessKind, c: i64, k: i64) -> Access {
    Access {
        kind,
        base: "A".into(),
        index: vec![c],
        elem_size: 8,
        offset: OffsetValue::Const(k),
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    }
}

// ---------------------------------------------------------------------------
// §6.1.2 simple bounds
// ---------------------------------------------------------------------------

#[test]
fn bounds_union_and_intersect_are_exact() {
    let a = Region::span(0, 3);
    let b = Region::span(2, 7);
    // union = [min l : max u]; intersect = [max l : min u].
    assert_eq!(a.union(&b), Some(Region::span(0, 7)));
    assert_eq!(a.intersect(&b), Some(Region::span(2, 3)));
    // Disjoint boxes intersect to the *empty* region, not `None`.
    let disjoint = a.intersect(&Region::span(10, 12)).unwrap();
    assert!(disjoint.is_empty());
    assert!(Region::empty().is_empty());
    assert!(!Region::span(0, 0).is_empty());
}

#[test]
fn bounds_refuse_an_arity_mismatch() {
    let one = Region::span(0, 3);
    let two = Region::new(vec![(0, 1), (0, 1)]);
    assert_eq!(one.union(&two), None);
    assert_eq!(one.intersect(&two), None);
    // A 1-D point never answers a 2-D containment question.
    assert!(!two.contains(&[0]));
    assert!(two.contains(&[0, 1]));
}

#[test]
fn region_of_access_contains_every_visited_location() {
    // The region is the *smallest box* the access's locations sweep, so it must
    // contain every element the loop actually visits.
    let l = loop1(0, 8, 1);
    let a = aff(AccessKind::Write, 2, 1); // A[2i + 1]
    let r = region_of_access(&a, &l).unwrap();
    let visited = locations_of_access(&a, &l, 4096).unwrap();
    for loc in &visited {
        assert!(r.contains(&[*loc]), "location {loc} outside {}", r.to_string_repr());
    }
    // The box is exactly [min:max] of the visited set.
    assert_eq!(
        r,
        Region::span(*visited.iter().min().unwrap(), *visited.iter().max().unwrap())
    );
}

#[test]
fn region_set_widens_overlap_and_keeps_arity_separate() {
    let mut s = RegionSet::new();
    s.insert(Region::span(0, 3));
    s.insert(Region::span(2, 7)); // overlaps -> merged
    assert_eq!(s.regions.len(), 1);
    assert_eq!(s.regions[0], Region::span(0, 7));
    s.insert(Region::new(vec![(0, 1), (0, 1)])); // different arity -> kept
    assert_eq!(s.regions.len(), 2);
    // Empty regions carry no information and are dropped.
    s.insert(Region::empty());
    assert_eq!(s.regions.len(), 2);
}

// ---------------------------------------------------------------------------
// §6.1.3 triplet notation
// ---------------------------------------------------------------------------

#[test]
fn triplet_union_and_intersect_are_exact() {
    let a = Triplet::new(0, 10, 2); // {0,2,4,6,8,10}
    let b = Triplet::new(1, 11, 2); // {1,3,5,7,9,11}
    // gcd(2,2,0-1) = 1, so the union is the full range.
    let u = a.union(&b).unwrap();
    assert_eq!(u.enumerate(100).unwrap(), (0..=11).collect::<Vec<_>>());
    // Opposite parities never meet.
    assert!(a.intersect(&b).is_none());

    let c = Triplet::new(2, 10, 2); // {2,4,6,8,10}
    let i = a.intersect(&c).unwrap();
    assert_eq!(i.enumerate(100).unwrap(), vec![2, 4, 6, 8, 10]);
}

#[test]
fn triplet_of_access_matches_exact_enumeration() {
    // A[2i] over i in [0,8): the *box* wrongly includes odd elements, the
    // triplet does not.
    let l = loop1(0, 8, 1);
    let a = aff(AccessKind::Write, 2, 0);
    let t = m0::region::triplet_of_access(&a, &l).unwrap();
    let tri = t.enumerate(4096).unwrap().into_iter().map(|p| p[0]).collect::<Vec<_>>();
    let visited = locations_of_access(&a, &l, 4096).unwrap();
    assert_eq!(tri, visited);
    let box_ = region_of_access(&a, &l).unwrap();
    // The box contains the odd element 1; the triplet correctly excludes it.
    assert!(box_.contains(&[1]) && !t.contains(&[1]), "triplet is strictly finer than the box");
}

#[test]
fn triplet_refuses_a_non_affine_index() {
    let l = loop1(0, 5, 1);
    let mut a = aff(AccessKind::Read, 0, 0);
    a.index_kind = IndexKind::Product(m0::ir::IndexExpr::Mul(
        Box::new(m0::ir::IndexExpr::IV(0)),
        Box::new(m0::ir::IndexExpr::IV(0)),
    ));
    assert_eq!(m0::region::triplet_of_access(&a, &l), None);
}

// ---------------------------------------------------------------------------
// §6.1.4 kill analysis
// ---------------------------------------------------------------------------

#[test]
fn rank_is_exact_integer_rank() {
    assert_eq!(rank(&[vec![1, 0]]), 1);
    assert_eq!(rank(&[vec![0, 0]]), 0);
    assert_eq!(rank(&[vec![1, 0], vec![1, 0]]), 1); // duplicate rows
    assert_eq!(rank(&[vec![1, 0], vec![0, 1]]), 2); // independent rows
    assert_eq!(rank(&[]), 0); // no subscripts, no constraints
}

#[test]
fn fullness_distinguishes_injective_from_overwriting_writes() {
    let l = loop1(0, 8, 1);
    // A[i] writes 0..7 once each.
    assert_eq!(fullness(&aff(AccessKind::Write, 1, 0), &l), Fullness::EveryElementOnce);
    // A[0] writes the same element every iteration.
    assert_eq!(fullness(&aff(AccessKind::Write, 0, 0), &l), Fullness::Overwrites);
    // A read is not a write.
    assert_eq!(fullness(&aff(AccessKind::Read, 1, 0), &l), Fullness::Unknown);
    assert!(is_self_conflict_free(&aff(AccessKind::Write, 1, 0), &l));
    assert!(!is_self_conflict_free(&aff(AccessKind::Write, 0, 0), &l));
    assert!(is_invariant_write(&aff(AccessKind::Write, 0, 0), &l));
    assert!(!is_invariant_write(&aff(AccessKind::Write, 1, 0), &l));
}

#[test]
fn kills_requires_full_coverage() {
    let l = loop1(0, 8, 1);
    let full = aff(AccessKind::Write, 1, 0); // writes 0..7
    let wide = aff(AccessKind::Write, 2, 0); // writes 0,2,...,14
    // A full overwrite can kill an identical previous write...
    assert!(kills(&full, &full, &l));
    // ...but a write that misses one of the previous write's elements does not.
    assert!(!kills(&full, &wide, &l), "0,2,..14 is not covered by 0..7");
    // A read is never a killing write.
    assert!(!kills(&aff(AccessKind::Read, 1, 0), &full, &l));
    assert_eq!(killed_region(&full, &l), Some(Region::span(0, 7)));
    assert_eq!(killed_region(&aff(AccessKind::Read, 1, 0), &l), None);
    let _ = access_matrix(&full, &l);
}

// ---------------------------------------------------------------------------
// §6.1.1 MayUse / MayMod / Kill dataflow and the whole-loop summary
// ---------------------------------------------------------------------------

#[test]
fn dataflow_may_use_and_may_mod_union_over_paths() {
    // Block 0 writes A[0..9]; block 1 reads A[0..7]; block 2 writes A[3..10].
    let l = loop1(0, 10, 1);
    let write_lo = aff(AccessKind::Write, 1, 0);
    let read = aff(AccessKind::Read, 1, 0);
    let write_hi = aff(AccessKind::Write, 1, 3);
    let prog = RegionProgram::linear(vec![vec![write_lo], vec![read], vec![write_hi]]);
    prog.validate().expect("linear program is well-formed");
    let df = dataflow(&prog, &l);
    let s = df.summary(2);
    assert!(!s.may_use.is_empty(), "the summary must see the reads and writes");
    assert!(!s.may_mod.is_empty());
    // Block 0 certainly writes A[0], so it is in the exit Kill set.
    assert!(
        s.kill.iter().any(|r| r.contains(&[0])),
        "block 0 certainly writes A[0]; kill = {:?}",
        s.kill
    );
}

#[test]
fn loop_summary_is_read_and_write_regions() {
    let mut l = loop1(0, 8, 1);
    l.body = vec![aff(AccessKind::Read, 1, 0), aff(AccessKind::Write, 1, 2)];
    let s = loop_summary(&l);
    assert!(s.may_use.iter().any(|r| r.contains(&[0])));
    assert!(s.may_mod.iter().any(|r| r.contains(&[2])));
    // A loop body runs every statement each trip, so kill == may_mod here.
    assert_eq!(s.kill, s.may_mod);
}

#[test]
fn region_program_from_loop_builds_the_five_block_shape() {
    let mut l = loop1(0, 8, 1);
    l.body = vec![aff(AccessKind::Write, 1, 0)];
    let prog = RegionProgram::from_loop(&l).expect("a loop with an IV builds a program");
    assert_eq!(prog.blocks.len(), 5);
    assert_eq!(prog.entry, 0);
    assert_eq!(prog.exit, 4);
    assert_eq!(prog.blocks[2].ops.len(), 1, "the body access is block 2");
    prog.validate().expect("the five-block shape is valid");
    // A loop without an IV has no guard/latch and no program.
    let no_iv = Loop { ivs: vec![], ..loop1(0, 8, 1) };
    assert!(RegionProgram::from_loop(&no_iv).is_none());
}

#[test]
fn triplet_region_stride_round_trip() {
    let t = TripletRegion::span(0, 14, 2);
    assert!(t.contains(&[0]) && t.contains(&[14]) && !t.contains(&[1]));
    assert_eq!(t.to_region(), Region::span(0, 14));
    // Arity mismatch refuses union/intersection.
    let two = TripletRegion::new(vec![Triplet::point(0), Triplet::point(0)]);
    assert_eq!(t.union(&two), None);
}