//! Custom Phase 1 verification: the certificate-foundations vocabulary
//! (`Dir`, `Claim::Direction`, `Claim::Level`, `Claim::CrossLoop`) checked
//! against hand-computed expected values, plus serde round-trips.
//!
//! These are written independently of the crate's own tests: every expected
//! value below is stated in the test as a literal, so a regression in the
//! derivation helpers is caught even if the library's own tests were changed.

use m0::certificate::{dependence_level, Claim, Dir};

/// `Claim` has no `PartialEq`; compare structurally through JSON.
fn same(a: &Claim, b: &Claim) -> bool {
    serde_json::to_value(a).unwrap() == serde_json::to_value(b).unwrap()
}

/// The full `Dir::holds` truth table, stated as literals.
#[test]
fn dir_holds_truth_table() {
    // (relation, delta, expected)
    let cases = [
        (Dir::Lt, -5, true), (Dir::Lt, 0, false), (Dir::Lt, 7, false),
        (Dir::Eq, -5, false), (Dir::Eq, 0, true), (Dir::Eq, 7, false),
        (Dir::Gt, -5, false), (Dir::Gt, 0, false), (Dir::Gt, 7, true),
        (Dir::Ne, -5, true), (Dir::Ne, 0, false), (Dir::Ne, 7, true),
        (Dir::Unknown, -5, true), (Dir::Unknown, 0, true), (Dir::Unknown, 7, true),
    ];
    for (r, d, want) in cases {
        assert_eq!(r.holds(d), want, "{:?}.holds({})", r, d);
    }
}

/// `Dir::of_delta` classifies sign only.
#[test]
fn dir_of_delta_classifies_sign() {
    assert_eq!(Dir::of_delta(-1), Dir::Lt);
    assert_eq!(Dir::of_delta(-100), Dir::Lt);
    assert_eq!(Dir::of_delta(0), Dir::Eq);
    assert_eq!(Dir::of_delta(1), Dir::Gt);
    assert_eq!(Dir::of_delta(100), Dir::Gt);
}

/// The dependence level is the leftmost nonzero component index; all-zero
/// means loop-independent, reported as `len`.
#[test]
fn dependence_level_hand_checked() {
    assert_eq!(dependence_level(&[]), 0);
    assert_eq!(dependence_level(&[0]), 1);
    assert_eq!(dependence_level(&[3]), 0);
    assert_eq!(dependence_level(&[0, -5]), 1);
    assert_eq!(dependence_level(&[0, 0, 3, 0]), 2);
    assert_eq!(dependence_level(&[1, 2]), 0);
    assert_eq!(dependence_level(&[0, 0, 0]), 3); // loop-independent
}

/// `Claim::direction_of` yields the strongest relation per component.
#[test]
fn direction_of_hand_checked() {
    assert!(same(
        &Claim::direction_of(&[0, -4, 2]),
        &Claim::Direction(vec![Dir::Eq, Dir::Lt, Dir::Gt])
    ));
    assert!(same(
        &Claim::direction_of(&[0, 0]),
        &Claim::Direction(vec![Dir::Eq, Dir::Eq])
    ));
}

/// `Claim::level_of` matches `dependence_level`.
#[test]
fn level_of_hand_checked() {
    assert!(same(&Claim::level_of(&[0, 0, 3, 0]), &Claim::Level(2)));
    assert!(same(&Claim::level_of(&[0, 0, 0]), &Claim::Level(3)));
    assert!(same(&Claim::level_of(&[9]), &Claim::Level(0)));
}

/// The two derivations agree with the reference definitions for a grid of
/// distance vectors (independent re-derivation inline).
#[test]
fn derivations_agree_with_inline_reference() {
    let deltas = [-9i64, -1, 0, 1, 9];
    for &a in &deltas {
        for &b in &deltas {
            for &c in &deltas {
                let d = vec![a, b, c];
                let want_dir: Vec<Dir> = d
                    .iter()
                    .map(|&x| if x < 0 { Dir::Lt } else if x == 0 { Dir::Eq } else { Dir::Gt })
                    .collect();
                assert!(same(&Claim::direction_of(&d), &Claim::Direction(want_dir)));
                let want_level = d.iter().position(|&x| x != 0).unwrap_or(3);
                assert!(same(&Claim::level_of(&d), &Claim::Level(want_level)));
            }
        }
    }
}

/// Every Phase 1 claim and its witness carrier survives a JSON round-trip
/// byte-for-byte.
#[test]
fn phase1_claim_serde_round_trip() {
    let claims = [
        Claim::Direction(vec![Dir::Lt, Dir::Eq, Dir::Gt, Dir::Ne, Dir::Unknown]),
        Claim::Level(2),
        Claim::CrossLoop(vec![0, -1]),
        Claim::Exact(vec![-4]),
        Claim::None,
    ];
    for c in claims {
        let s = serde_json::to_string(&c).unwrap();
        let back: Claim = serde_json::from_str(&s).unwrap();
        assert!(same(&back, &c), "claim round-trip changed {}", s);
        let s2 = serde_json::to_string(&back).unwrap();
        assert_eq!(s, s2);
    }
}

// ---- End-to-end through the real m0-check binary ------------------------

use m0::certificate::{Certificate, Witness};
use m0::ir::Loop;
use std::path::PathBuf;
use std::process::Command;

fn run_checker(tag: &str, loop_json: &str, cert_json: &str) -> String {
    let dir = std::env::temp_dir().join(format!("m0-p1f-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lp = dir.join("loop.json");
    let cp = dir.join("cert.json");
    std::fs::write(&lp, loop_json).unwrap();
    std::fs::write(&cp, cert_json).unwrap();
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-check"));
    let out = Command::new(bin).arg(&lp).arg(&cp).output().expect("run m0-check");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    std::fs::remove_dir_all(&dir).ok();
    if text.is_empty() { err } else { text }
}

/// The classic `for (i=0;i<N;i++) A[i+4] = A[i];` loop: distance `[-4]`,
/// so `Direction([Lt])` holds and `Direction([Gt])` does not.
fn distance4_loop() -> Loop {
    let s = std::fs::read_to_string("tests/loops/01_distance4.json").unwrap();
    serde_json::from_str(&s).unwrap()
}

#[test]
fn direction_claim_accepted_by_checker() {
    let l = distance4_loop();
    let cert = Certificate {
        level: "phase1".into(),
        loop_hash: l.hash(),
        pair: (0, 1),
        claim: Claim::Direction(vec![Dir::Lt]),
        witness: Witness::IntSolution { i: vec![4], j: vec![0] },
        checker: "m0-check".into(),
    };
    let out = run_checker("dir-ok", &l.canonical_json(), &serde_json::to_string(&cert).unwrap());
    assert!(out.starts_with("ACCEPT"), "checker said: {}", out);
}

#[test]
fn wrong_direction_claim_rejected_by_checker() {
    let l = distance4_loop();
    let cert = Certificate {
        level: "phase1".into(),
        loop_hash: l.hash(),
        pair: (0, 1),
        claim: Claim::Direction(vec![Dir::Gt]), // distance is -4, so Lt holds, Gt does not
        witness: Witness::IntSolution { i: vec![4], j: vec![0] },
        checker: "m0-check".into(),
    };
    let out = run_checker("dir-bad", &l.canonical_json(), &serde_json::to_string(&cert).unwrap());
    assert!(out.contains("REJECT"), "checker said: {}", out);
}

#[test]
fn level_claim_accepted_and_wrong_level_rejected() {
    let l = distance4_loop();
    let ok = Certificate {
        level: "phase1".into(),
        loop_hash: l.hash(),
        pair: (0, 1),
        claim: Claim::Level(0), // leftmost nonzero of [-4] is index 0
        witness: Witness::IntSolution { i: vec![4], j: vec![0] },
        checker: "m0-check".into(),
    };
    let out = run_checker("lvl-ok", &l.canonical_json(), &serde_json::to_string(&ok).unwrap());
    assert!(out.starts_with("ACCEPT"), "checker said: {}", out);

    let bad = Certificate {
        level: "phase1".into(),
        loop_hash: l.hash(),
        pair: (0, 1),
        claim: Claim::Level(1), // wrong: distance [-4] is level 0
        witness: Witness::IntSolution { i: vec![4], j: vec![0] },
        checker: "m0-check".into(),
    };
    let out = run_checker("lvl-bad", &l.canonical_json(), &serde_json::to_string(&bad).unwrap());
    assert!(out.contains("REJECT"), "checker said: {}", out);
}