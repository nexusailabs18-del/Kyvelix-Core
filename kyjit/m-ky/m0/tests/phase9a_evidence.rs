//! Phase 9a end-to-end: the **production** path emits and verifies the two
//! Phase 4 loop-level claims.
//!
//! Phase 4 added [`Claim::TripCount`] and [`Claim::ScalarDependence`] with
//! checker arms that rebuild the FUD chains, `induct`, `scalar_dd` and the trip
//! count from a CFG the certificate carries. Until Phase 9a nothing in
//! `m0-decide` constructed either claim — the analyses ran on a single-block CFG
//! with no back edge, so `induct` could not prove an induction variable and
//! `scalar_dd` could not see anything loop-carried.
//!
//! These tests run the real binaries. Each one:
//!
//!   1. writes a loop to a temp directory,
//!   2. runs `m0-decide`, which now calls `decide::decide_evidence` alongside
//!      the access-pair decision and writes every certificate it has,
//!   3. runs `m0-check` on the resulting file and requires ACCEPT.
//!
//! Step 3 is the load-bearing one: `m0-check` re-derives each evidence claim
//! from the CFG inside it, so an ACCEPT is a statement that the *emitter* and
//! the *rebuilder* agree, not that a producer asserted a number. The tamper
//! tests then break the emitted certificate in ways that are only detectable by
//! that recomputation and require the checker to reject.
//!
//! # The certificate file has two shapes
//!
//! `cert.json` holds a bare object when there is no evidence to report, exactly
//! as it did before Phase 9a, so the per-level harnesses under `tests/` that
//! only ever saw one certificate are untouched. When evidence exists the file is
//! an array `[decision_cert, evidence...]` and the checker verifies **every**
//! element — a file is a conjunction of claims. The corpus sweep in
//! `total_phase4_production_round_trip` exercises both shapes on every loop in
//! the repo.

use std::path::{Path, PathBuf};
use std::process::Command;

use m0::certificate::{Certificate, Claim};
use m0::decide::decide_evidence;
use m0::ir::*;

// ---------------------------------------------------------------------------
// Loop builders
// ---------------------------------------------------------------------------

fn acc(kind: AccessKind, index: i64, offset: i64) -> Access {
    Access {
        kind,
        base: "A".into(),
        index: vec![index],
        elem_size: 8,
        offset: OffsetValue::Const(offset),
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    }
}

fn loop_with(ivs: Vec<IV>, body: Vec<Access>) -> Loop {
    Loop {
        depth: ivs.len(),
        ivs,
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    }
}

/// `A[i+1] = A[i+2]`-style flow: an ordinary affine loop with a real IV.
fn affine_loop() -> Loop {
    loop_with(
        vec![IV::new("i", 0, 8, 1)],
        vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
    )
}

/// The same body presented as a recursion. Its JSON `ivs` are `upper: 0`
/// placeholders — the recursion metadata *is* the range.
fn recursion_loop() -> Loop {
    let mut l = loop_with(
        vec![IV::new("i", 0, 0, 1)],
        vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
    );
    l.recursion = Some(Recursion {
        func: "f".into(),
        iv_name: "i".into(),
        iv_init: 0,
        iv_step: 1,
        iv_bound: 16,
        bound_op: CmpOp::Lt,
        other_params: vec![],
        companion: None,
    });
    l
}

/// A bounded pointer chase, also counted from placeholders.
fn chase_loop() -> Loop {
    let mut l = loop_with(
        vec![IV::new("i", 0, 4, 1)],
        vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)],
    );
    l.chase = Some(PointerChase {
        var: "p".into(),
        table: vec![1, 2, 3, 4],
        table_expr: None,
        init: 0,
        max_steps: 4,
        accessed_base: "A".into(),
    });
    l
}

/// An M8 recurrence-indexed loop.
fn recurrence_loop() -> Loop {
    let rec = RecurrenceId { initial: vec![0, 1], coeffs: vec![1, 1] };
    let mut a = acc(AccessKind::Write, 0, 0);
    a.index_kind = IndexKind::Recurrence(rec.clone());
    let mut b = acc(AccessKind::Read, 0, 0);
    b.index_kind = IndexKind::Recurrence(rec);
    loop_with(vec![IV::new("i", 0, 8, 1)], vec![a, b])
}

// ---------------------------------------------------------------------------
// decide_evidence: what the dispatcher attributes to which level
// ---------------------------------------------------------------------------

/// `(level, claim)` summary of a certificate list, for readable assertions.
fn claims(certs: &[Certificate]) -> Vec<(String, String)> {
    certs
        .iter()
        .map(|c| {
            let k = match &c.claim {
                Claim::TripCount { count, exit_value } => {
                    format!("TripCount({count},{exit_value})")
                }
                Claim::ScalarDependence { distance, .. } => {
                    format!("ScalarDependence({distance:?})")
                }
                other => format!("{other:?}"),
            };
            (c.level.clone(), k)
        })
        .collect()
}

fn has(certs: &[Certificate], level: &str, claim: &str) -> bool {
    claims(certs).iter().any(|(l, c)| l == level && c == claim)
}

#[test]
fn affine_loop_yields_m0_trip_count_evidence() {
    let l = affine_loop();
    let certs = decide_evidence(&l);
    // 0..8 step 1 is eight trips, exiting at 8.
    assert!(has(&certs, "M0", "TripCount(8,8)"), "got {:?}", claims(&certs));
    for c in &certs {
        assert_eq!(c.loop_hash, l.hash(), "evidence must bind the file's loop");
    }
}

#[test]
fn recursion_loop_yields_m3_trip_count_evidence() {
    let l = recursion_loop();
    let certs = decide_evidence(&l);
    // The count comes from the recursion's bound (16), not the placeholder
    // `upper: 0` the file carries.
    assert!(has(&certs, "M3", "TripCount(16,16)"), "got {:?}", claims(&certs));
    for c in &certs {
        assert_eq!(c.loop_hash, l.hash());
    }
}

#[test]
fn chase_loop_yields_m5_evidence_with_the_original_hash() {
    let l = chase_loop();
    let certs = decide_evidence(&l);
    // One trip per chase step: the table has four entries.
    assert!(has(&certs, "M5", "TripCount(4,4)"), "got {:?}", claims(&certs));
    // The hash is the file's loop and not the expansion's, which is what lets
    // the checker — which hashes the file it was handed — accept it.
    for c in &certs {
        assert_eq!(c.loop_hash, l.hash());
    }
}

#[test]
fn recurrence_loop_is_attributed_to_m8() {
    let l = recurrence_loop();
    let certs = decide_evidence(&l);
    assert!(has(&certs, "M8", "TripCount(8,8)"), "got {:?}", claims(&certs));
    // Not also claimed as M0: attribution is exclusive.
    assert!(
        !certs.iter().any(|c| c.level == "M0"
            && matches!(c.claim, Claim::TripCount { .. })),
        "a Recurrence loop must not also be counted as M0: {:?}",
        claims(&certs)
    );
}

#[test]
fn scalar_dependence_evidence_names_a_carried_link() {
    let l = affine_loop();
    let certs = decide_evidence(&l);
    let dd: Vec<_> = certs.iter().filter(|c| c.level == "SCALARDD").collect();
    assert_eq!(dd.len(), 1, "expected one carried scalar dependence, got {:?}", claims(&certs));
    assert!(matches!(dd[0].claim, Claim::ScalarDependence { .. }));
    for c in &dd {
        assert_eq!(c.loop_hash, l.hash());
    }
}

#[test]
fn every_emitted_certificate_survives_serialisation() {
    // The production boundary is JSON. A certificate that does not round-trip
    // through serde is a checker rejection waiting to happen.
    for l in [affine_loop(), recursion_loop(), chase_loop(), recurrence_loop()] {
        for c in decide_evidence(&l) {
            let s = serde_json::to_string(&c).unwrap();
            let back: Certificate = serde_json::from_str(&s).unwrap();
            assert_eq!(back.loop_hash, c.loop_hash);
            assert_eq!(back.level, c.level);
            assert_eq!(back.pair, c.pair);
        }
    }
}

// ---------------------------------------------------------------------------
// The real binaries: decide writes, check verifies
// ---------------------------------------------------------------------------

/// A per-test temp directory, keyed by tag so parallel tests do not collide.
fn workdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m0-p9a-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `m0-decide` on `loop_json` and return the certificates it wrote.
///
/// The decider writes `cert.json` into its working directory, so it runs in a
/// per-test temp dir — the same convention every harness under `tests/` uses.
fn decide(dir: &Path, loop_json: &str) -> Vec<Certificate> {
    std::fs::write(dir.join("loop.json"), loop_json).unwrap();
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-decide"));
    let out = Command::new(bin)
        .arg(dir.join("loop.json"))
        .current_dir(dir)
        .output()
        .expect("run m0-decide");
    assert!(out.status.success(), "m0-decide failed: {}", String::from_utf8_lossy(&out.stderr));
    let cert_path = dir.join("cert.json");
    if !cert_path.exists() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&cert_path).unwrap();
    // Both file shapes: a bare object, or an array of them.
    serde_json::from_str::<Vec<Certificate>>(&text)
        .or_else(|_| serde_json::from_str::<Certificate>(&text).map(|c| vec![c]))
        .expect("cert.json must be a certificate or an array of them")
}

/// Run the real `m0-check` on the files already in `dir`.
fn check(dir: &Path) -> (bool, String) {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-check"));
    let out = Command::new(bin)
        .arg(dir.join("loop.json"))
        .arg(dir.join("cert.json"))
        .output()
        .expect("run m0-check");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let first = if text.is_empty() { err } else { text };
    (out.status.success(), first)
}

/// Decide and check a loop written in the corpus's own JSON syntax.
fn round_trip(tag: &str, loop_json: &str) -> (Vec<Certificate>, String) {
    let dir = workdir(tag);
    let certs = decide(&dir, loop_json);
    let (ok, out) = check(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert!(ok, "the real checker rejected production output: {out}");
    (certs, out)
}

/// Serialise a loop exactly the way the corpus files are read.
fn json(l: &Loop) -> String {
    serde_json::to_string(l).unwrap()
}

#[test]
fn production_output_of_an_affine_loop_is_accepted() {
    let (certs, _) = round_trip("affine", &json(&affine_loop()));
    assert!(has(&certs, "M0", "TripCount(8,8)"), "got {:?}", claims(&certs));
    assert!(certs.iter().any(|c| c.level == "SCALARDD"), "got {:?}", claims(&certs));
}

#[test]
fn production_output_of_a_recursion_loop_is_accepted() {
    let (certs, _) = round_trip("recursion", &json(&recursion_loop()));
    assert!(has(&certs, "M3", "TripCount(16,16)"), "got {:?}", claims(&certs));
}

#[test]
fn production_output_of_a_chase_loop_is_accepted() {
    let (certs, _) = round_trip("chase", &json(&chase_loop()));
    assert!(has(&certs, "M5", "TripCount(4,4)"), "got {:?}", claims(&certs));
}

#[test]
fn production_output_of_a_recurrence_loop_is_accepted() {
    let (certs, _) = round_trip("recurrence", &json(&recurrence_loop()));
    assert!(has(&certs, "M8", "TripCount(8,8)"), "got {:?}", claims(&certs));
}

#[test]
fn the_decision_certificates_come_first_and_evidence_after() {
    // Phase 9a adds certificates; it does not displace the ones the access-pair
    // question is answered with. Harnesses grep the file for the level string
    // and feed it whole to the checker — both keep working.
    let (certs, _) = round_trip("order", &json(&affine_loop()));
    assert!(!certs.is_empty());
    assert!(
        matches!(certs[0].claim, Claim::Exact(_) | Claim::None | Claim::Parametric(_)),
        "the first certificate must be the decision, got {:?}",
        claims(&certs)
    );
    // Evidence never precedes a decision certificate: the file's first element
    // is what a harness that only ever reads one certificate will see.
    let first_evidence = certs
        .iter()
        .position(|c| matches!(c.claim, Claim::TripCount { .. } | Claim::ScalarDependence { .. }));
    let last_decision = certs
        .iter()
        .rposition(|c| matches!(c.claim, Claim::Exact(_) | Claim::None | Claim::Parametric(_)));
    match (first_evidence, last_decision) {
        (Some(e), Some(d)) => assert!(e > d, "evidence at {e} precedes decision at {d}"),
        (Some(_), None) => panic!("a file of only evidence has no decision certificate"),
        (None, _) => {} // no evidence for this loop: nothing to order
    }
    // Every certificate is the same loop's.
    let l = affine_loop();
    for c in &certs {
        assert_eq!(c.loop_hash, l.hash());
    }
}

// ---------------------------------------------------------------------------
// Tampering: the checker must recompute, not trust
// ---------------------------------------------------------------------------

/// Decide a loop, mutate the emitted certificates, and check again.
fn tamper(tag: &str, l: &Loop, mutate: impl Fn(&mut Vec<serde_json::Value>)) -> (bool, String) {
    let dir = workdir(tag);
    decide(&dir, &json(l));
    let path = dir.join("cert.json");
    let text = std::fs::read_to_string(&path).unwrap();
    // Normalise to an array so a mutation can address any element.
    let mut v: Vec<serde_json::Value> = serde_json::from_str::<Vec<serde_json::Value>>(&text)
        .or_else(|_| serde_json::from_str::<serde_json::Value>(&text).map(|c| vec![c]))
        .unwrap();
    let before = v.len();
    mutate(&mut v);
    assert_eq!(v.len(), before, "the mutation must edit, not add or drop");
    std::fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
    let r = check(&dir);
    std::fs::remove_dir_all(&dir).ok();
    r
}

/// The index of the first `Claim::TripCount` certificate at this level.
///
/// Selecting by level alone is not enough: a recursion loop's *decision*
/// certificate is also labelled `M3`, and its claim and witness have a different
/// shape, so mutating it by trip-count field names edits nothing — a tamper test
/// that did that passed for the wrong reason.
fn trip_count_index(certs: &[serde_json::Value], level: &str) -> usize {
    certs
        .iter()
        .position(|c| {
            c["level"].as_str() == Some(level) && c["claim"]["kind"].as_str() == Some("TripCount")
        })
        .unwrap_or_else(|| panic!("no {level} TripCount certificate in the output"))
}

/// The index of the scalar-dependence certificate.
fn scalar_dd_index(certs: &[serde_json::Value]) -> usize {
    certs
        .iter()
        .position(|c| c["claim"]["kind"].as_str() == Some("ScalarDependence"))
        .unwrap_or_else(|| panic!("no ScalarDependence certificate in the output"))
}

#[test]
fn tampered_trip_count_is_rejected() {
    // The claim says 7 trips; the checker recomputes the real count from the
    // carried CFG and refuses. Nothing in the certificate is believed about
    // arithmetic.
    for field in ["count", "exit_value"] {
        let (ok, out) = tamper(format!("tc-{field}").as_str(), &recursion_loop(), |v| {
            let i = trip_count_index(v, "M3");
            v[i]["claim"]["value"][field] = serde_json::json!(7);
        });
        assert!(!ok, "tampering {field} was accepted: {out}");
        assert!(out.starts_with("REJECT"), "expected REJECT, got: {out}");
        assert!(out.contains("recomputed trip count"), "unexpected message: {out}");
    }
}

#[test]
fn a_nominated_iv_that_induct_does_not_prove_is_rejected() {
    // The witness names a variable and a header; the checker looks the pair up
    // in its own `induct` result, so a certificate cannot nominate a scalar, or
    // move the claim to a header that has no IV.
    for (field, value) in [
        ("iv_var", serde_json::json!(99)),
        ("loop_", serde_json::json!(0)),
    ] {
        let key = format!("iv-{field}");
        let (ok, out) = tamper(&key, &recursion_loop(), |v| {
            let i = trip_count_index(v, "M3");
            v[i]["witness"][field] = value.clone();
        });
        assert!(!ok, "nominating {field}={value} was accepted: {out}");
        assert!(out.contains("does not report"), "unexpected message: {out}");
    }
}


#[test]
fn a_forged_bound_is_rejected() {
    // `bound` is the one input the CFG does not encode, so it is a parameter of
    // the recomputation rather than a fact the checker accepts. Changing it
    // changes what the checker recomputes, and the claim no longer matches.
    let (ok, out) = tamper("bound", &recursion_loop(), |v| {
        let i = trip_count_index(v, "M3");
        v[i]["witness"]["bound"] = serde_json::json!(1000);
    });
    assert!(!ok, "a forged bound was accepted: {out}");
    assert!(out.starts_with("REJECT"), "unexpected outcome: {out}");
}

#[test]
fn a_tampered_scalar_distance_is_rejected() {
    // The checker rebuilds the whole `(def, use) -> situation` table from the
    // carried CFG; asserting an intra-iteration situation for a carried link
    // contradicts its own re-derivation.
    let (ok, out) = tamper("dd", &affine_loop(), |v| {
        let i = scalar_dd_index(v);
        v[i]["claim"]["value"]["distance"] = serde_json::json!("Zero");
    });
    assert!(!ok, "a forged distance was accepted: {out}");
    assert!(out.contains("does not report"), "unexpected message: {out}");
}

#[test]
fn a_stripped_carried_cfg_is_rejected() {
    // The CFG is the ground truth. Empty it and the certificate carries nothing
    // the checker can rebuild from.
    let (ok, out) = tamper("strip", &recursion_loop(), |v| {
        let i = trip_count_index(v, "M3");
        v[i]["witness"]["cfg"]["blocks"] = serde_json::json!([]);
    });
    assert!(!ok, "a stripped CFG was accepted: {out}");
    assert!(out.contains("malformed"), "unexpected message: {out}");
}

#[test]
fn a_wrong_loop_hash_on_an_evidence_certificate_is_rejected() {
    // Every element is bound to the file, not just the first one.
    let (ok, out) = tamper("hash", &recursion_loop(), |v| {
        let i = trip_count_index(v, "M3");
        v[i]["loop_hash"] = serde_json::json!("0".repeat(64));
    });
    assert!(!ok, "a mismatched hash was accepted: {out}");
    assert!(out.contains("loop hash mismatch"), "unexpected message: {out}");
}

#[test]
fn one_bad_certificate_rejects_the_whole_file() {
    // An array is a conjunction. Every element is verified, so corroding any
    // single one — and only that one — must refuse the whole file. Silently
    // skipping an element, or checking only the first, would be the hole.
    //
    // `loop_hash` is the field every certificate shape has, so corrupting it
    // tests the conjunction without depending on any one witness's layout.
    let dir = workdir("conj");
    decide(&dir, &json(&recursion_loop()));
    let path = dir.join("cert.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let all: Vec<Certificate> = serde_json::from_str(&text).unwrap();
    assert!(all.len() >= 3, "expected evidence in the output, got {}", all.len());
    std::fs::remove_dir_all(&dir).ok();

    for bad in 0..all.len() {
        let dir = workdir(&format!("conj{bad}"));
        decide(&dir, &json(&recursion_loop()));
        let text = std::fs::read_to_string(dir.join("cert.json")).unwrap();
        let mut v: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
        v[bad]["loop_hash"] = serde_json::json!("0".repeat(64));
        std::fs::write(dir.join("cert.json"), serde_json::to_string(&v).unwrap()).unwrap();
        let (ok, out) = check(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(!ok, "element {bad} of {} was accepted: {out}", v.len());
        assert!(out.contains("loop hash mismatch"), "element {bad}: {out}");
    }
}

#[test]
fn an_empty_certificate_array_is_rejected_not_vacuously_accepted() {
    let dir = workdir("empty");
    decide(&dir, &json(&affine_loop()));
    std::fs::write(dir.join("cert.json"), "[]").unwrap();
    let (ok, out) = check(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert!(!ok, "an empty array was accepted: {out}");
    assert!(out.contains("empty list"), "unexpected message: {out}");
}



// ---------------------------------------------------------------------------
// Corpus sweep: production output of every loop in the repo still checks
// ---------------------------------------------------------------------------

/// The corpus directories the per-level harnesses drive — the hand-written
/// fixtures each level was built against. The two generated corpora
/// (`tests/real_loops`, 1001 loops, and `tests/stress_loops`, 552) are added
/// when `M0_PHASE9A_WIDE=1`, since they make the sweep take tens of seconds.
const CORPUS: &[&str] = &[
    "tests/loops",
    "tests/m0_step_loops",
    "tests/m2a_loops",
    "tests/m2a5_loops",
    "tests/m2b_loops",
    "tests/m2g_loops",
    "tests/m3_loops",
    "tests/m35_loops",
    "tests/m4_loops",
    "tests/m5_loops",
    "tests/m6_loops",
    "tests/m6c_loops",
    "tests/m7_loops",
    "tests/m8_loops",
    "tests/m9_loops",
    "tests/m10_loops",
    "tests/m12_loops",
    "tests/m13_loops",
    "tests/param_loops",
    "tests/negative",
];

const WIDE_CORPUS: &[&str] = &["tests/real_loops", "tests/stress_loops"];

/// Keep a failure message readable when many loops fail.
fn show(failures: &[String]) -> String {
    const LIMIT: usize = 20;
    if failures.len() <= LIMIT {
        return failures.join("\n");
    }
    let mut s = failures[..LIMIT].join("\n");
    s.push_str(&format!("\n... and {} more", failures.len() - LIMIT));
    s
}

/// Every loop the repo ships is decided by the real decider, and every
/// certificate it writes — the decision *and* any evidence — is verified by the
/// real checker. One rejection fails the test, and the failure names the loop.
///
/// This is the Phase 4 §4.3 requirement read at production scale: the evidence
/// certificates must be sound on loops nobody wrote a case for.
#[test]
fn total_phase4_production_round_trip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = workdir("sweep");
    let wide = std::env::var("M0_PHASE9A_WIDE").is_ok();
    let mut total = 0usize;
    let mut with_evidence = 0usize;
    let mut failures: Vec<String> = Vec::new();

    let mut dirs: Vec<&str> = CORPUS.to_vec();
    if wide {
        dirs.extend(WIDE_CORPUS);
    }

    for sub in dirs {
        let Ok(rd) = std::fs::read_dir(root.join(sub)) else { continue };
        let mut files: Vec<_> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        files.sort();
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            // Only loops the library itself parses; malformed fixtures are the
            // negative-corpus scripts' business, not this test's.
            if serde_json::from_str::<Loop>(&text).is_err() {
                continue;
            }
            total += 1;
            std::fs::write(dir.join("loop.json"), &text).unwrap();
            std::fs::remove_file(dir.join("cert.json")).ok();
            let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-decide"));
            let ok = Command::new(bin)
                .arg(dir.join("loop.json"))
                .current_dir(&dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                continue; // the decider declining to decide is not a checker failure
            }
            let Ok(cert_text) = std::fs::read_to_string(dir.join("cert.json")) else {
                continue; // no certificate written: nothing to verify
            };
            let certs: Vec<Certificate> =
                serde_json::from_str::<Vec<Certificate>>(&cert_text)
                    .or_else(|_| serde_json::from_str::<Certificate>(&cert_text).map(|c| vec![c]))
                    .unwrap_or_else(|e| {
                        failures.push(format!("{}: unparseable cert.json: {e}", path.display()));
                        Vec::new()
                    });
            if certs.is_empty() {
                continue;
            }
            if certs.iter().any(|c| {
                matches!(c.claim, Claim::TripCount { .. } | Claim::ScalarDependence { .. })
            }) {
                with_evidence += 1;
            }
            let (accepted, out) = check(&dir);
            if !accepted {
                failures.push(format!("{}: {out}", path.display()));
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();

    assert!(failures.is_empty(), "{} of {total} loops rejected:\n{}", failures.len(), show(&failures));
    // The sweep must actually have reached the emitters: a corpus that produced
    // no evidence would mean this test verified nothing about Phase 9a.
    assert!(with_evidence > 0, "no corpus loop produced trip-count or scalar-dependence evidence");
    // Report the tally so a later green run cannot be mistaken for a vacuous one.
    println!("swept {total} corpus loops: {with_evidence} produced TripCount or \
              ScalarDependence evidence, all verified by the real m0-check, 0 rejected");
}
