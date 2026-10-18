//! Custom Phase 2 verification: the six solver modules and their wiring,
//! checked against hand-computed expected values.
//!
//! Every expected number/verdict below is stated as a literal, independent of
//! the crate's own tests, so a regression in the math engine is caught here
//! even if the in-crate property tests were altered.

use m0::solver::dark_shadow::{dark_shadow_holds, solve_inequalities};
use m0::solver::extreme::{bounds_test_holds, extreme_1};
use m0::solver::gcd::{findgcd, gcd, gcd_many, gcd_test_2, general_solution_2};
use m0::solver::hnf::{hermite, verify_hnf};
use m0::solver::integer::{solve_integer_mixed, solve_integer_system, IntSolveResult};
use m0::solver::inverse::{adjugate, determinant, is_inverse, is_unimodular, rank, Matrix};

fn m(rows: &[&[i64]]) -> Matrix {
    let owned: Vec<Vec<i64>> = rows.iter().map(|r| r.to_vec()).collect();
    Matrix::from_rows(&owned).unwrap()
}

// ---- gcd.rs ---------------------------------------------------------------

#[test]
fn gcd_known_values() {
    assert_eq!(gcd(12, 18), 6);
    assert_eq!(gcd(-8, 12), 4);
    assert_eq!(gcd(7, 0), 7);
    assert_eq!(gcd(0, 5), 5);
    assert_eq!(gcd(0, 0), 0);
    assert_eq!(gcd(17, 5), 1); // coprime
}

#[test]
fn gcd_many_known() {
    assert_eq!(gcd_many(&[12, 18, 24]), 6);
    assert_eq!(gcd_many(&[7]), 7);
    assert_eq!(gcd_many(&[]), 0);
    assert_eq!(gcd_many(&[9, 12, 15]), 3);
}

#[test]
fn findgcd_bezout_identity() {
    // 240 = 5·46 + 10, 46 = 4·10 + 6, 10 = 1·6 + 4, 6 = 1·4 + 2 → g = 2.
    let (g, x, y) = findgcd(240, 46);
    assert_eq!(g, 2);
    assert_eq!(240 * x + 46 * y, g, "Bézout identity");

    let (g, x, y) = findgcd(12, 18);
    assert_eq!(g, 6);
    assert_eq!(12 * x + 18 * y, 6);

    // Negative operand.
    let (g, x, y) = findgcd(-8, 12);
    assert_eq!(g, 4);
    assert_eq!(-8 * x + 12 * y, 4);
}

#[test]
fn gcd_test_examples() {
    assert!(gcd_test_2(6, 10, 4)); // gcd 2 divides 4
    assert!(!gcd_test_2(6, 10, 3)); // 2 ∤ 3
    assert!(gcd_test_2(0, 0, 0)); // 0 = 0 solvable
    assert!(!gcd_test_2(0, 0, 5)); // 0 = 5 impossible
}

#[test]
fn general_solution_family_satisfies_equation() {
    // 6x + 10y = 4  (gcd 2 | 4)
    let (x0, y0, dx, dy) = general_solution_2(6, 10, 4).unwrap();
    for t in [-3i64, -1, 0, 1, 5] {
        let x = x0 + dx * t;
        let y = y0 + dy * t;
        assert_eq!(6 * x + 10 * y, 4, "family member at t={}", t);
    }
    // Unsolvable: 6x + 10y = 3
    assert!(general_solution_2(6, 10, 3).is_none());
}

// ---- inverse.rs -----------------------------------------------------------

#[test]
fn determinant_known() {
    assert_eq!(determinant(&m(&[&[1, 2], &[3, 4]])), Some(-2));
    assert_eq!(determinant(&m(&[&[2, 1], &[1, 1]])), Some(1));
    assert_eq!(determinant(&m(&[&[1, 0, 0], &[0, 2, 0], &[0, 0, 3]])), Some(6));
    assert_eq!(determinant(&m(&[&[1, 2], &[2, 4]])), Some(0)); // singular
}

#[test]
fn rank_known() {
    assert_eq!(rank(&m(&[&[1, 2], &[2, 4]])), 1);
    assert_eq!(rank(&m(&[&[1, 2], &[3, 4]])), 2);
    assert_eq!(rank(&m(&[&[1, 0, 0], &[0, 2, 0], &[0, 0, 3]])), 3);
    assert_eq!(rank(&m(&[&[0, 0], &[0, 0]])), 0);
}

#[test]
fn adjugate_satisfies_A_adj_equals_det_I() {
    let a = m(&[&[1, 2], &[3, 4]]);
    let (adj, det) = adjugate(&a).unwrap();
    assert_eq!(det, -2);
    let prod = a.matmul(&adj).unwrap();
    let want = m(&[&[-2, 0], &[0, -2]]); // det · I
    assert_eq!(prod, want);
}

#[test]
fn unimodular_and_integer_inverse() {
    let a = m(&[&[2, 1], &[1, 1]]); // det 1
    assert!(is_unimodular(&a));
    let inv = m(&[&[1, -1], &[-1, 2]]);
    assert!(is_inverse(&a, &inv));
    // 2·2 matrix, nonsingular but not unimodular (det -2)
    assert!(!is_unimodular(&m(&[&[1, 2], &[3, 4]])));
}

// ---- hnf.rs ---------------------------------------------------------------

#[test]
fn hnf_verify_and_AU_equals_H() {
    let a = m(&[&[2, 4], &[6, 8]]);
    let (h, u) = hermite(&a).unwrap();
    verify_hnf(&a, &h, &u).expect("hermite must verify");
    // A·U = H by construction.
    assert_eq!(a.matmul(&u).unwrap(), h);
    // H is lower-triangular with positive diagonal (Wolfe convention).
    assert!(h.get(0, 0) > 0 && h.get(1, 1) > 0);
    assert_eq!(h.get(0, 1), 0);
}

#[test]
fn lattice_index_of_full_rank_is_determinant_magnitude() {
    let a = m(&[&[2, 0], &[0, 3]]);
    assert_eq!(m0::solver::hnf::lattice_index(&a), Some(6));
}
// ---- integer.rs -----------------------------------------------------------

#[test]
fn solve_integer_system_known_cases() {
    // 2x = 4  → x = 2
    match solve_integer_system(&m(&[&[2]]), &[4]) {
        IntSolveResult::Solutions { particular, .. } => assert_eq!(particular, vec![2]),
        other => panic!("expected a solution, got {:?}", other),
    }
    // 2x = 3  → proven no integer solution
    assert_eq!(solve_integer_system(&m(&[&[2]]), &[3]), IntSolveResult::NoSolution);
    // x + y = 3  → a particular solution satisfies the equation
    match solve_integer_system(&m(&[&[1, 1]]), &[3]) {
        IntSolveResult::Solutions { particular, nullspace } => {
            assert_eq!(particular[0] + particular[1], 3);
            assert_eq!(nullspace.len(), 1); // one free variable
        }
        other => panic!("expected a solution, got {:?}", other),
    }
    // Dependent rows, consistent: x + y = 3, 2x + 2y = 6
    match solve_integer_system(&m(&[&[1, 1], &[2, 2]]), &[3, 6]) {
        IntSolveResult::Solutions { particular, .. } => {
            assert_eq!(particular[0] + particular[1], 3);
        }
        other => panic!("expected a solution, got {:?}", other),
    }
    // Dependent rows, inconsistent: x + y = 3, 2x + 2y = 7
    assert_eq!(
        solve_integer_system(&m(&[&[1, 1], &[2, 2]]), &[3, 7]),
        IntSolveResult::NoSolution
    );
}

#[test]
fn solve_integer_mixed_witness_satisfies_all() {
    // x + y = 3,  x <= 1  (i.e. [1,0]·x <= 1)
    match solve_integer_mixed(&m(&[&[1, 1]]), &[3], &m(&[&[1, 0]]), &[1]) {
        IntSolveResult::Solutions { particular, .. } => {
            assert_eq!(particular[0] + particular[1], 3);
            assert!(particular[0] <= 1);
        }
        other => panic!("expected a solution, got {:?}", other),
    }
    // Inconsistent inequalities: x <= 1 and -x <= -5  → x >= 5, impossible.
    assert_eq!(
        solve_integer_mixed(
            &m(&[&[1, 1]]),
            &[3],
            &m(&[&[1, 0], &[-1, 0]]),
            &[1, -5]
        ),
        IntSolveResult::NoSolution
    );
}

// ---- dark_shadow.rs -------------------------------------------------------

#[test]
fn dark_shadow_single_pair_formula() {
    // reference condition: beta*c_u - alpha*c_l >= alpha*beta - alpha - beta + 1
    let f = |alpha: i64, cu: i64, beta: i64, cl: i64| beta * cu - alpha * cl >= alpha * beta - alpha - beta + 1;
    for &(a, cu, b, cl) in &[(2i64, 1i64, 3i64, 0i64), (2, 0, 2, 0), (3, 1, 3, 1), (1, 1, 1, 0)] {
        assert_eq!(dark_shadow_holds(a, cu, b, cl), f(a, cu, b, cl), "args {:?}", (a, cu, b, cl));
    }
}

#[test]
fn solve_inequalities_sat_and_unsat() {
    // x <= -1  (Sat, e.g. x = -1)
    assert!(solve_inequalities(&m(&[&[1]]), &[-1]).is_sat());
    // -x <= -1  → x >= 1  (Sat)
    assert!(solve_inequalities(&m(&[&[-1]]), &[-1]).is_sat());
    // x <= -1  AND  -x <= -1  → x <= -1 and x >= 1 : unsat
    assert!(solve_inequalities(&m(&[&[1], &[-1]]), &[-1, -1]).is_unsat());
    // 2x <= 1  → x <= 0 (integer), sat
    assert!(solve_inequalities(&m(&[&[2]]), &[1]).is_sat());
}

// ---- extreme.rs -----------------------------------------------------------

#[test]
fn extreme_known_ranges() {
    // 2i for i in [1,3] → {2, 6}
    let b = extreme_1(2, 1, 3);
    assert_eq!((b.lower, b.upper), (2, 6));
    // -2i for i in [1,3] → {-6, -2}
    let b = extreme_1(-2, 1, 3);
    assert_eq!((b.lower, b.upper), (-6, -2));
    // i - j for i,j ∈ {0,1} → [-1, 1]
    let b = m0::solver::extreme::extreme(&[1, -1], 0, &[0, 0], &[1, 1]);
    assert_eq!((b.lower, b.upper), (-1, 1));
}

#[test]
fn bounds_test_refutes_gap_beyond_range() {
    // write A[i+5], read A[i], i in [0,3]  → need = 0 - 5 = -5, outside [-3,3]
    assert!(!bounds_test_holds(&[1], 5, &[1], 0, &[0], &[3], 1));
    // write A[i+2], read A[i]  → need = -2, inside [-3,3]
    assert!(bounds_test_holds(&[1], 2, &[1], 0, &[0], &[3], 1));
}

// ---- DecisionTree (M1 parametric certificates) ----------------------------

use m0::certificate::{DecisionNode, DecisionTree, ParamCond, Terminal};
use m0::param_expr::ParamExpr;

#[test]
fn decision_tree_resolves_at_environments() {
    // if (P == 0) { leaf i=0,j=0,dist=0 } else { no dependence }
    let tree = DecisionTree {
        root: DecisionTree::branch(
            ParamCond::Eq(ParamExpr::Param("P".into()), 0),
            DecisionNode::Leaf(m0::certificate::DecisionLeaf::constant(&[0], &[0], &[0])),
            DecisionNode::NoDependence,
        ),
    };
    let env0 = vec![("P".to_string(), 0i64)];
    match tree.root.resolve_terminal(&env0) {
        Some(Terminal::Dependence(_, leaf)) => assert_eq!(leaf.distance[0].as_const(), Some(0)),
        other => panic!("expected dependence at P=0, got {:?}", other),
    }
    let env5 = vec![("P".to_string(), 5i64)];
    assert!(matches!(tree.root.resolve_terminal(&env5), Some(Terminal::Absent(_))));
}

#[test]
fn decision_tree_serde_round_trip() {
    let tree = DecisionTree {
        root: DecisionTree::branch(
            ParamCond::Mod(ParamExpr::Param("P".into()), 2),
            DecisionNode::Leaf(m0::certificate::DecisionLeaf::constant(&[1], &[3], &[2])),
            DecisionNode::NoDependence,
        ),
    };
    let s = serde_json::to_string(&tree).unwrap();
    let back: DecisionTree = serde_json::from_str(&s).unwrap();
    assert_eq!(back, tree);
}

/// One random node: a leaf, or a branch over two smaller nodes.
fn random_node(rng: &mut impl FnMut() -> u64, budget: usize) -> DecisionNode {
    if budget == 0 || rng() % 3 == 0 {
        return if rng() % 2 == 0 {
            DecisionNode::Leaf(m0::certificate::DecisionLeaf::constant(
                &[(rng() % 3) as i64],
                &[(rng() % 4) as i64],
                &[(rng() % 4) as i64],
            ))
        } else {
            DecisionNode::NoDependence
        };
    }
    let p = || ParamExpr::Param("P".into());
    let c = (rng() % 5) as i64 - 2;
    let cond = match rng() % 4 {
        0 => ParamCond::Eq(p(), c),
        1 => ParamCond::Le(p(), c),
        2 => ParamCond::Ge(p(), c),
        _ => ParamCond::Mod(p(), (1 + rng() % 3) as i64),
    };
    DecisionTree::branch(cond, random_node(rng, budget - 1), random_node(rng, budget - 1))
}

#[test]
fn every_decision_tree_shape_round_trips() {
    // `stubs.md` §1 asks for this fuzzer by name: "no fuzzer verifies every tree
    // shape round-trips through serialization". Random depths, all four condition
    // kinds, both leaf forms, and every tree the constructors can build — because
    // a certificate that loses a branch on the way to the checker is unsound in
    // exactly the way a wrong answer is.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut rng = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut shapes = 0usize;
    for case in 0..500 {
        let depth = (rng() % 4) as usize; // 0..=3
        let tree = DecisionTree { root: random_node(&mut rng, depth) };
        let s = serde_json::to_string(&tree).unwrap_or_else(|e| panic!("case {case}: {e}"));
        let back: DecisionTree =
            serde_json::from_str(&s).unwrap_or_else(|e| panic!("case {case}: {e}\n{s}"));
        assert_eq!(back, tree, "case {case}: {s}");
        shapes += 1;
    }
    assert_eq!(shapes, 500);
}
// ---- M0 wiring: the exact bounds test reaches the decision ----------------

use m0::decide::{decide, Decision};
use m0::ir::{Access, AccessKind, IndexKind, Loop, OffsetValue, IV};

fn affine_loop(write_off: i64, read_off: i64, lo: i64, hi: i64) -> Loop {
    let acc = |kind: AccessKind, off: i64| Access {
        kind,
        base: "A".into(),
        index: vec![1],
        elem_size: 8,
        offset: OffsetValue::Const(off),
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    };
    Loop {
        depth: 1,
        ivs: vec![IV::new("i", lo, hi, 1)],
        body: vec![acc(AccessKind::Write, write_off), acc(AccessKind::Read, read_off)],
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,}
}

#[test]
fn m0_exact_bounds_test_decides_none_and_exact() {
    // A[i+2] = A[i] over i in [0,4): write loc = i+2, read loc = j.
    // For pair (0,1) the distance j - i = 2 → EXACT [2].
    let (d, cert) = decide(&affine_loop(2, 0, 0, 4), 0, 1);
    assert!(matches!(d, Decision::Exact(ref v) if *v == vec![2]), "got {:?}", d);
    assert!(cert.is_some());
    // The reversed pair (1,0) is the negated distance.
    let (d, _) = decide(&affine_loop(2, 0, 0, 4), 1, 0);
    assert!(matches!(d, Decision::Exact(ref v) if *v == vec![-2]), "got {:?}", d);

    // A[i+5] = A[i] over i in [0,4): gap of 5 exceeds the range → NONE
    let (d, _) = decide(&affine_loop(5, 0, 0, 4), 0, 1);
    assert!(matches!(d, Decision::None), "got {:?}", d);
}
