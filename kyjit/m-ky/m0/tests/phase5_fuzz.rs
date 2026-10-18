//! Phase 5 §5.x — independent-oracle fuzz tests for the Phase 5 subsystems.
//!
//! Each subsystem is checked against an oracle that shares **no code** with it:
//! a brute-force enumeration or closed-form assertion computed inline. Random
//! loops/accesses are generated from a deterministic LCG, so a failure is
//! reproducible from its seed.
//!
//! # Seed-and-revert discipline
//!
//! Each oracle takes a `bug` flag that selects a deliberately-wrong oracle, one
//! per subsystem: an off-by-one in the layout linearizer, a dropped sign in the
//! alignment check, a flipped loop-kind and an added term in the cross-loop
//! distance. `seed_and_revert_reports_violations` runs each with `bug = true`,
//! asserts it fires, then with `bug = false`, asserts silence — proof that no
//! oracle here is vacuous.

use m0::fud::{compute_alignment, compute_cross_loop_distance, cross_loop_vectors};
use m0::ir::dependence::{DepKind, Dependence};
use m0::ir::layout::{linear_index, Layout};
use m0::ir::loopkind::{classify_loop_kind, LoopKind};
use m0::ir::{IV, Loop};

/// A tiny deterministic LCG. No dependency on `rand`, so the fuzz is
/// byte-identical across builds and platforms.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(2654435761).wrapping_add(1))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    /// A value in `[lo, hi]` inclusive.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }
}

const CASES: u64 = 4_000;

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

// ---------------------------------------------------------------------------
// §5.1.1 layout
// ---------------------------------------------------------------------------

/// Brute-force check that `linear_index` is a bijection from a multi-index box
/// onto `0..product(dims)` for row-major layout.
fn fuzz_layout_matches_enumeration(bug: bool) -> usize {
    let mut rng = Rng::new(0x1A70);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let dims: Vec<usize> = (0..rng.range(1, 3)).map(|_| rng.range(1, 5) as usize).collect();
        let total: i64 = dims.iter().product::<usize>() as i64;
        // Every index in the box linearizes into range, and no two collide.
        let mut seen = std::collections::HashSet::new();
        let mut idx = vec![0i64; dims.len()];
        loop {
            match linear_index(Layout::RowMajor, &idx, &dims) {
                Some(v) => {
                    let v = if bug { v + 1 } else { v };
                    if v < 0 || v >= total || !seen.insert(v) {
                        violations += 1;
                    }
                }
                None => violations += 1,
            }
            // Advance the odometer.
            let mut k = 0;
            while k < dims.len() {
                idx[k] += 1;
                if idx[k] < dims[k] as i64 {
                    break;
                }
                idx[k] = 0;
                k += 1;
            }
            if k == dims.len() {
                break;
            }
        }
        if seen.len() as i64 != total {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_layout_is_a_bijection_onto_the_index_space() {
    assert_eq!(fuzz_layout_matches_enumeration(false), 0);
}

// ---------------------------------------------------------------------------
// §5.1.9 alignment
// ---------------------------------------------------------------------------

/// Build a dependence set from random per-statement shifts, guaranteeing the
/// system is consistent, then check `compute_alignment` removes every edge.
fn fuzz_alignment_removes_every_dependence(bug: bool) -> usize {
    let mut rng = Rng::new(0xA1116);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let n = rng.range(2, 5) as usize;
        let l = loop1(0, 8);
        // A dependence src->dst at distance (shift[dst]-shift[src]) is
        // consistent by construction, so `compute_alignment` always succeeds.
        let shifts: Vec<i64> = (0..n).map(|_| rng.range(-3, 3)).collect();
        let mut deps = Vec::new();
        for _ in 0..rng.range(1, 6) {
            let s = rng.range(0, n as i64 - 1) as usize;
            let d = rng.range(0, n as i64 - 1) as usize;
            if s != d {
                deps.push(Dependence::new(s, d, DepKind::Flow, vec![shifts[d] - shifts[s]]));
            }
        }
        match compute_alignment(&l, &deps) {
            None => violations += 1,
            Some(a) => {
                let shift = |s: usize| a.iter().find(|x| x.stmt == s).map(|x| x.shift).unwrap_or(0);
                for dep in &deps {
                    // New distance = d + shift(src) - shift(dst).
                    let new_d = if bug {
                        dep.distance[0] - shift(dep.src) + shift(dep.dst)
                    } else {
                        dep.distance[0] + shift(dep.src) - shift(dep.dst)
                    };
                    if new_d != 0 {
                        violations += 1;
                    }
                }
            }
        }
    }
    violations
}

#[test]
fn fuzz_alignment_removes_all_dependences() {
    assert_eq!(fuzz_alignment_removes_every_dependence(false), 0);
}

// ---------------------------------------------------------------------------
// §5.1.11 loop kind
// ---------------------------------------------------------------------------

/// The oracle classification, written out from Wolfe's table independently of
/// `classify_loop_kind`.
fn oracle_loop_kind(deps: &[Dependence]) -> LoopKind {
    if deps.is_empty() {
        return LoopKind::Doall;
    }
    let carried_flow: Vec<&Dependence> = deps
        .iter()
        .filter(|d| d.kind == DepKind::Flow && (d.unknown || d.distance.iter().any(|&x| x != 0)))
        .collect();
    if carried_flow.is_empty() {
        return LoopKind::Dopar;
    }
    if carried_flow.iter().all(|d| d.distance.iter().any(|&x| x < 0)) {
        return LoopKind::Forall;
    }
    if !deps.iter().any(|d| matches!(d.kind, DepKind::Output | DepKind::Anti)) {
        return LoopKind::Dosingle;
    }
    LoopKind::Sequential
}

fn fuzz_loop_kind_matches_oracle(bug: bool) -> usize {
    let mut rng = Rng::new(0x1000);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let l = loop1(0, 8);
        let n = rng.range(1, 5);
        let kinds = [DepKind::Flow, DepKind::Anti, DepKind::Output, DepKind::Input];
        let mut deps = Vec::new();
        for _ in 0..n {
            let s = rng.range(0, 3) as usize;
            let d = rng.range(0, 3) as usize;
            let dist = rng.range(-2, 2);
            let k = kinds[rng.range(0, 3) as usize];
            deps.push(Dependence::new(s, d, k, vec![dist]));
        }
        let got = classify_loop_kind(&l, &deps);
        let mut want = oracle_loop_kind(&deps);
        if bug && want == LoopKind::Dopar {
            want = LoopKind::Sequential;
        }
        if got != want {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_loop_kind_matches_the_oracle() {
    assert_eq!(fuzz_loop_kind_matches_oracle(false), 0);
}

// ---------------------------------------------------------------------------
// §5.1.10 cross-loop distance
// ---------------------------------------------------------------------------

fn fuzz_cross_loop_matches_lower_difference(bug: bool) -> usize {
    let mut rng = Rng::new(0xC205);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let la = rng.range(-5, 5);
        let lb = rng.range(-5, 5);
        let a = loop1(la, la + 8);
        let b = loop1(lb, lb + 8);
        let d = compute_cross_loop_distance(&a, &b).unwrap();
        let want = if bug { lb + la } else { lb - la };
        if d.distance != vec![want] {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_cross_loop_distance_matches_the_entry_offset() {
    assert_eq!(fuzz_cross_loop_matches_lower_difference(false), 0);
}

/// `cross_loop_vectors` obeys its algebraic identity on random inputs:
/// `d = (j_T - i_S) - (j_S - i_T)`.
fn fuzz_cross_loop_vectors_identity(bug: bool) -> usize {
    let mut rng = Rng::new(0xDEAD);
    let mut violations = 0usize;
    for _ in 0..CASES {
        let m = 1 + rng.next() % 3;
        let v: Vec<i64> = (0..4 * m).map(|_| rng.range(-20, 20)).collect();
        let (i_s, rest) = v.split_at(m as usize);
        let (i_t, rest) = rest.split_at(m as usize);
        let (j_s, j_t) = rest.split_at(m as usize);
        let got = cross_loop_vectors(i_s, i_t, j_s, j_t).unwrap();
        let want: Vec<i64> = (0..m as usize)
            .map(|k| {
                if bug {
                    (j_t[k] - i_s[k]) + (j_s[k] - i_t[k])
                } else {
                    (j_t[k] - i_s[k]) - (j_s[k] - i_t[k])
                }
            })
            .collect();
        if got != want {
            violations += 1;
        }
    }
    violations
}

#[test]
fn fuzz_cross_loop_vectors_match_the_formula() {
    assert_eq!(fuzz_cross_loop_vectors_identity(false), 0);
}

// ---------------------------------------------------------------------------
// Seed-and-revert
// ---------------------------------------------------------------------------

#[test]
fn seed_and_revert_reports_violations() {
    // Each seeded oracle must fire on `bug = true` and be silent on `false`.
    let seeds: [(&str, fn(bool) -> usize); 5] = [
        ("layout", fuzz_layout_matches_enumeration),
        ("alignment", fuzz_alignment_removes_every_dependence),
        ("loop-kind", fuzz_loop_kind_matches_oracle),
        ("cross-loop", fuzz_cross_loop_matches_lower_difference),
        ("cross-loop-vectors", fuzz_cross_loop_vectors_identity),
    ];
    for (name, f) in seeds {
        let bad = f(true);
        assert!(bad > 0, "seed `{name}` did not fire");
        assert_eq!(f(false), 0, "seed `{name}` fired without a bug");
    }
}