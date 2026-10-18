//! Phase 6 §6.x — independent-oracle fuzz tests for the region subsystem.
//!
//! The oracles are exact enumerations: `locations_of_access` walks the access
//! over the loop's iteration domain, and the region/triplet claims are checked
//! against that concrete set. Random affine accesses come from a deterministic
//! LCG, so a failure is reproducible from its seed.
//!
//! # Seed-and-revert discipline
//!
//! Each oracle takes a `bug` flag selecting a deliberately-wrong check:
//! a clipped region box, a truncated triplet, and a reversed subset test in
//! `kills`. `seed_and_revert_reports_violations` proves none is vacuous.

use m0::ir::{Access, AccessKind, IndexKind, IV, Loop, OffsetValue};
use m0::region::{kills, locations_of_access, region_of_access, triplet_of_access};

/// A tiny deterministic LCG (no `rand` dependency).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(2654435761).wrapping_add(1))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }
}

/// Iteration domains are kept small so `locations_of_access` always enumerates.
const CASES: u64 = 4_000;
const MAX_LEN: i64 = 16;

fn loop1(lo: i64, hi: i64) -> Loop {
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", lo, hi, 1)],
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
// §6.1.2 region soundness / exactness
// ---------------------------------------------------------------------------

fn fuzz_region_is_the_min_max_box(bug: bool) -> usize {
    let mut rng = Rng::new(0x6E61);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let lo = rng.range(-3, 3);
        let hi = lo + rng.range(1, MAX_LEN);
        let l = loop1(lo, hi);
        let c = rng.range(-3, 3);
        let k = rng.range(-5, 5);
        let a = aff(AccessKind::Write, c, k);
        let Some(r) = region_of_access(&a, &l) else { continue };
        let visited = locations_of_access(&a, &l, 4096).unwrap();
        // Soundness: the box contains every location the loop visits.
        for loc in &visited {
            if !r.contains(&[*loc]) {
                violations += 1;
            }
        }
        // Exactness: the box is exactly [min : max].
        let want_lo = *visited.iter().min().unwrap();
        let want_hi = *visited.iter().max().unwrap();
        let (got_lo, mut got_hi) = r.dims[0];
        if bug {
            got_hi -= 1;
        }
        if (got_lo, got_hi) != (want_lo, want_hi) {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_region_matches_enumeration() {
    assert_eq!(fuzz_region_is_the_min_max_box(false), 0);
}

// ---------------------------------------------------------------------------
// §6.1.3 triplet exactness
// ---------------------------------------------------------------------------

fn fuzz_triplet_equals_enumeration(bug: bool) -> usize {
    let mut rng = Rng::new(0x7213);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let lo = rng.range(-3, 3);
        let hi = lo + rng.range(1, MAX_LEN);
        let l = loop1(lo, hi);
        // A positive coefficient with a zero offset keeps the access an
        // arithmetic progression in `i`, so a triplet exists and is exact.
        let c = rng.range(1, 4);
        let a = aff(AccessKind::Write, c, 0);
        let Some(t) = triplet_of_access(&a, &l) else { continue };
        let visited = locations_of_access(&a, &l, 4096).unwrap();
        let mut tri = t.enumerate(4096).unwrap().into_iter().map(|p| p[0]).collect::<Vec<_>>();
        if bug && !tri.is_empty() {
            tri.pop();
        }
        if tri != visited {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_triplet_matches_enumeration() {
    assert_eq!(fuzz_triplet_equals_enumeration(false), 0);
}

// ---------------------------------------------------------------------------
// §6.1.4 kill soundness
// ---------------------------------------------------------------------------

/// The oracle for `kills(def, previous)`: true iff every location `previous`
/// can write is a location `def` certainly writes. The seeded bug reverses the
/// containment direction.
fn oracle_kills(def: &Access, previous: &Access, l: &Loop, bug: bool) -> Option<bool> {
    use std::collections::HashSet;
    if def.kind != AccessKind::Write || def.base != previous.base {
        return Some(false);
    }
    let prev: HashSet<i64> = locations_of_access(previous, l, 4096)?.into_iter().collect();
    let d: HashSet<i64> = locations_of_access(def, l, 4096)?.into_iter().collect();
    let subset = |a: &HashSet<i64>, b: &HashSet<i64>| a.iter().all(|x| b.contains(x));
    if bug {
        Some(subset(&d, &prev))
    } else {
        Some(subset(&prev, &d))
    }
}

fn fuzz_kills_matches_subset(bug: bool) -> usize {
    let mut rng = Rng::new(0xB111);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let l = loop1(0, rng.range(2, MAX_LEN));
        // A zero coefficient makes a constant (non-injective) write, so the two
        // location sets differ in size and the containment direction matters.
        let def = aff(AccessKind::Write, rng.range(0, 3), rng.range(-2, 2));
        let previous = aff(AccessKind::Write, rng.range(0, 3), rng.range(-2, 2));
        let Some(want) = oracle_kills(&def, &previous, &l, bug) else { continue };
        if kills(&def, &previous, &l) != want {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_kills_matches_the_subset_oracle() {
    assert_eq!(fuzz_kills_matches_subset(false), 0);
}

// ---------------------------------------------------------------------------
// Seed-and-revert
// ---------------------------------------------------------------------------

#[test]
fn seed_and_revert_reports_violations() {
    let seeds: [(&str, fn(bool) -> usize); 3] = [
        ("region", fuzz_region_is_the_min_max_box),
        ("triplet", fuzz_triplet_equals_enumeration),
        ("kills", fuzz_kills_matches_subset),
    ];
    for (name, f) in seeds {
        assert!(f(true) > 0, "seed `{name}` did not fire");
        assert_eq!(f(false), 0, "seed `{name}` fired without a bug");
    }
}