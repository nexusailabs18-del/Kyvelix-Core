//! Phase 7 differential fuzz — points-to and aliasing.
//!
//! Two drivers, each with an **independent oracle** so a bug in the analysis
//! cannot hide behind the same bug in the check:
//!
//! 1. `alias_fuzz` compares the algebraic disjointness rule
//!    ([`m0::points::alias::arrays_disjoint`]) against brute-force enumeration
//!    of both index sets ([`m0::points::alias::enumerate_alias`]). A `NoAlias` or
//!    `MustAlias` the rule reports must survive enumeration.
//! 2. `fixpoint_fuzz` runs the points-to fixpoint and compares it against a
//!    concrete interpreter that tracks exactly which single target each pointer
//!    holds for a straight-line program of `AddressOf` / `PointerAssign` / `Kill`
//!    — a semantics that shares no code with the lattice.
//!
//! Every driver takes a `bug` flag. `seed_and_revert_*` runs each driver with a
//! deliberately-wrong oracle, asserts the violations appear, and then with the
//! correct oracle, asserting they vanish. An oracle that can never fail is not
//! an oracle, so this is asserted rather than assumed.

use m0::points::alias::{enumerate_alias, query_refs, Alias};
use m0::points::fixpoint::{analyze, PtrBlock, PtrDecl, PtrProgram};
use m0::points::ptrarith::ReconstructedRef;
use m0::points::relation::VarId;
use m0::points::transfer::Effect;

/// A tiny deterministic linear-congruential generator, so a failing case is
/// reproducible from its seed alone.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407))
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// A value in `[0, n)`.
    fn below(&mut self, n: i64) -> i64 {
        (self.next() >> 33) as i64 % n
    }
}

fn reference(rng: &mut Lcg) -> ReconstructedRef {
    ReconstructedRef {
        array: VarId(rng.below(2) as u32),
        coeff: rng.below(3),  // 0, 1 or 2
        offset: rng.below(6), // 0..5
        elem_size: 8,
        pointer: VarId(9),
    }
}

/// The correct algebraic answer, or a deliberately-unsound one when `bug`.
fn algebraic(a: &ReconstructedRef, b: &ReconstructedRef, bug: bool) -> Alias {
    if bug {
        // The seed: "same array with different strides is always disjoint",
        // which is false whenever the two progressions share an element.
        if a.array == b.array && a.coeff != b.coeff {
            Alias::NoAlias
        } else {
            query_refs(a, b)
        }
    } else {
        query_refs(a, b)
    }
}

/// Count soundness violations: a `NoAlias`/`MustAlias` the algebra reports that
/// the enumeration contradicts.
fn alias_fuzz(seed: u64, iters: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed);
    let mut violations = 0;
    for _ in 0..iters {
        let a = reference(&mut rng);
        let b = reference(&mut rng);
        let brute = enumerate_alias(&a, &b, 0, 6);
        let alg = algebraic(&a, &b, bug);
        if alg == Alias::NoAlias && brute != Alias::NoAlias {
            violations += 1;
        }
        if alg == Alias::MustAlias && brute != Alias::MustAlias {
            violations += 1;
        }
    }
    violations
}

/// A straight-line program of `nptr` pointers and `ntgt` targets, with the
/// concrete state a real interpreter would hold.
fn random_straight_line(rng: &mut Lcg, nptr: usize, ntgt: usize) -> (PtrProgram, Vec<Option<u32>>) {
    let mut decls: Vec<PtrDecl> = (0..ntgt)
        .map(|i| PtrDecl::target(format!("t{}", i)))
        .collect();
    decls.extend((0..nptr).map(|i| PtrDecl::pointer(format!("p{}", i))));
    let mut stmts: Vec<Effect> = Vec::new();
    let mut state: Vec<Option<u32>> = vec![None; nptr];
    let nops = 1 + rng.below(6) as usize;
    for _ in 0..nops {
        let p = rng.below(nptr as i64) as usize;
        let pid = VarId((ntgt + p) as u32);
        match rng.below(3) {
            0 => {
                let t = rng.below(ntgt as i64) as u32;
                stmts.push(Effect::AddressOf { lhs: pid, target: VarId(t) });
                state[p] = Some(t);
            }
            1 => {
                let q = rng.below(nptr as i64) as usize;
                stmts.push(Effect::PointerAssign { lhs: pid, rhs: VarId((ntgt + q) as u32) });
                state[p] = state[q];
            }
            _ => {
                stmts.push(Effect::Kill { lhs: pid });
                state[p] = None;
            }
        }
    }
    (PtrProgram::new(decls, 0, vec![PtrBlock::new(0, vec![], stmts)]), state)
}

/// Count disagreements between the fixpoint and the concrete interpreter.
fn fixpoint_fuzz(seed: u64, iters: usize, bug: bool) -> usize {
    let mut rng = Lcg::new(seed);
    let mut violations = 0;
    let (nptr, ntgt) = (3usize, 3usize);
    for _ in 0..iters {
        let (prog, state) = random_straight_line(&mut rng, nptr, ntgt);
        let analysis = analyze(&prog).expect("a well-formed straight line converges");
        let g = analysis.out_of(0).unwrap();
        for p in 0..nptr {
            let pid = VarId((ntgt + p) as u32);
            for t in 0..ntgt {
                let tid = VarId(t as u32);
                let expected = if bug {
                    // The seed: pretend every assigned pointer points at t0.
                    t == 0 && state[p].is_some()
                } else {
                    state[p] == Some(t as u32)
                };
                if g.must_point_to(pid, tid) != expected || g.may_point_to(pid, tid) != expected {
                    violations += 1;
                }
            }
        }
    }
    violations
}


#[test]
fn alias_fuzz_is_clean_for_many_seeds() {
    for seed in [1u64, 2, 3, 42, 1234, 999_999] {
        assert_eq!(alias_fuzz(seed, 4_000, false), 0, "seed {seed} produced a violation");
    }
}

#[test]
fn fixpoint_fuzz_is_clean_for_many_seeds() {
    for seed in [1u64, 7, 13, 4242, 55_555] {
        assert_eq!(fixpoint_fuzz(seed, 3_000, false), 0, "seed {seed} produced a violation");
    }
}

#[test]
fn the_alias_oracle_is_not_vacuous() {
    // The enumeration must actually see overlapping and disjoint cases, or a
    // "clean" fuzz run would prove nothing.
    let mut rng = Lcg::new(5);
    let (mut no, mut may, mut must) = (0, 0, 0);
    for _ in 0..2_000 {
        let a = reference(&mut rng);
        let b = reference(&mut rng);
        match enumerate_alias(&a, &b, 0, 6) {
            Alias::NoAlias => no += 1,
            Alias::MayAlias => may += 1,
            Alias::MustAlias => must += 1,
        }
    }
    assert!(no > 0, "no disjoint pairs generated");
    assert!(may > 0, "no overlapping pairs generated");
    assert!(must > 0, "no identical pairs generated");
}

#[test]
fn the_fixpoint_oracle_is_not_vacuous() {
    // Some generated programmes must end with a pointer definitely holding a
    // target, or the agreement is between two all-`None` answers.
    let mut rng = Lcg::new(11);
    let mut definite = 0;
    for _ in 0..500 {
        let (_prog, state) = random_straight_line(&mut rng, 3, 3);
        if state.iter().any(|s| s.is_some()) {
            definite += 1;
        }
    }
    assert!(definite > 0, "no generated programme set a pointer");
}

#[test]
fn seed_and_revert_alias_reports_its_seeded_bug() {
    let seeded = alias_fuzz(9, 4_000, true);
    assert!(seeded > 0, "the seeded alias bug was not detected");
    assert_eq!(alias_fuzz(9, 4_000, false), 0, "the corrected rule still reports violations");
}

#[test]
fn seed_and_revert_fixpoint_reports_its_seeded_bug() {
    let seeded = fixpoint_fuzz(9, 3_000, true);
    assert!(seeded > 0, "the seeded fixpoint bug was not detected");
    assert_eq!(fixpoint_fuzz(9, 3_000, false), 0, "the corrected oracle still reports violations");
}
