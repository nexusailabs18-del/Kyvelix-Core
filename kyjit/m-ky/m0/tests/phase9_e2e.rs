//! Phase 9 §9.12 — end to end from **source**.
//!
//! Everything else in this repo starts from a `.js`-free world: the corpora are
//! hand-written `json`, and the analyzers never see a program. This file is the
//! other direction, and it is the headline result of Phase 9:
//!
//! ```text
//! tests/js_kernels/*.js  →  lower_source  →  Loop  →  m0-decide  →  cert.json
//!                                                     ↓
//!                                             m0-check  →  ACCEPT
//! ```
//!
//! Both binaries are the real ones (`CARGO_BIN_EXE_*`), run in a temporary
//! directory, so nothing here can pass by agreeing with an in-process copy of the
//! checker. The negative cases are as important as the positive one: a certificate
//! is only evidence if tampering with it — or with the loop it claims about —
//! makes the checker refuse.

use std::path::{Path, PathBuf};
use std::process::Command;

use m0::frontend::lower::{lower_source, Env};
use m0::ir::Loop;

/// Source → IR through the real lowering. The environment supplies the
/// parameters the kernels' bounds and strides name (`n`, `ld`).
fn lower(src: &str) -> Vec<Loop> {
    let env = Env::of(&[("n", 8), ("ld", 8)]);
    lower_source(src, &env).expect("parses").loops
}

/// The fixtures lowering can state, lowered.
fn lowered_fixtures() -> Vec<(&'static str, Vec<Loop>)> {
    [
        ("syr2k", include_str!("js_kernels/syr2k.js")),
        ("stencil2d", include_str!("js_kernels/stencil2d.js")),
        ("mix", include_str!("js_kernels/mix.js")),
    ]
    .into_iter()
    .map(|(name, src)| (name, lower(src)))
    .collect()
}

/// One `m0-decide` run: its directory, its output, and whether it wrote a cert.
struct Run {
    dir: PathBuf,
    stdout: String,
    ok: bool,
}

impl Run {
    fn loop_path(&self) -> PathBuf {
        self.dir.join("loop.json")
    }
    fn cert_path(&self) -> PathBuf {
        self.dir.join("cert.json")
    }
    /// The certificate, if the decider had anything to claim.
    fn cert(&self) -> Option<String> {
        std::fs::read_to_string(self.cert_path()).ok()
    }
}

fn workdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("m0-p9-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

/// Write `loop` as the "file on disk" and run the real `m0-decide` on it.
fn decide(tag: &str, loop_: &Loop) -> Run {
    let dir = workdir(tag);
    let lp = dir.join("loop.json");
    std::fs::write(&lp, loop_.canonical_json()).expect("write loop");
    let _ = std::fs::remove_file(dir.join("cert.json"));
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-decide"));
    let out = Command::new(bin)
        .arg(&lp)
        .current_dir(&dir)
        .output()
        .expect("run m0-decide");
    Run {
        dir,
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        ok: out.status.success(),
    }
}

/// Run the real `m0-check` on a `(loop, cert)` pair. Returns `(accepted, last line)`.
fn check(loop_path: &Path, cert_path: &Path) -> (bool, String) {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_m0-check"));
    let out = Command::new(bin)
        .arg(loop_path)
        .arg(cert_path)
        .output()
        .expect("run m0-check");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let last = if text.is_empty() { err } else { text };
    (out.status.success(), last)
}

// ---- the round trip ---------------------------------------------------------

#[test]
fn every_lowered_kernel_is_consumed_by_the_real_decider() {
    // Lowering output has to be IR the decider accepts: `Loop::validate` passes
    // inside the binary, and every access pair reaches a verdict.
    for (name, loops) in lowered_fixtures() {
        assert!(!loops.is_empty(), "{name} lowered to nothing");
        for (i, l) in loops.iter().enumerate() {
            let run = decide(&format!("decide-{name}{i}"), l);
            assert!(run.ok, "{name}[{i}]: m0-decide failed: {}", run.stdout);
            assert!(
                run.stdout.contains("overall:"),
                "{name}[{i}]: no decision summary: {}",
                run.stdout
            );
        }
    }
}

#[test]
fn a_lowered_kernels_certificate_replays_in_the_checker() {
    // The full chain, and the assertion that it actually ran: at least one
    // fixture must have produced a certificate, or this test would be vacuous.
    let mut certificates = 0usize;
    let mut accepted = 0usize;
    for (name, loops) in lowered_fixtures() {
        for (i, l) in loops.iter().enumerate() {
            let run = decide(&format!("replay-{name}{i}"), l);
            assert!(run.ok, "{name}[{i}]: m0-decide failed: {}", run.stdout);
            if run.cert().is_none() {
                continue;
            }
            certificates += 1;
            let (ok, msg) = check(&run.loop_path(), &run.cert_path());
            assert!(ok && msg.starts_with("ACCEPT"), "{name}[{i}]: checker said: {msg}");
            accepted += 1;
        }
    }
    assert!(
        certificates > 0,
        "no fixture produced a certificate: the round trip was never exercised"
    );
    assert_eq!(certificates, accepted, "every produced certificate must be accepted");
}

#[test]
fn a_construct_outside_the_subset_still_decides_nothing_and_says_why() {
    // §9.14's honest answer for a kernel the IR cannot hold: the source parses,
    // and the reason is named. Nothing is guessed, and nothing is decided.
    for (name, src) in [
        ("triangle", include_str!("js_kernels/triangle.js")),
        ("jaccard", include_str!("js_kernels/jaccard.js")),
    ] {
        let env = Env::of(&[("n", 8), ("ld", 8)]);
        let k = lower_source(src, &env).expect("parses");
        assert!(k.loops.is_empty(), "{name}: nothing to decide");
        assert_eq!(k.skipped.len(), 1, "{name}: one named refusal");
        assert!(!format!("{}", k.skipped[0].reason).is_empty());
    }
}

// ---- negatives: a certificate is evidence only if tampering breaks it --------

/// Parse `loop.json`, shrink the first IV's upper bound, re-emit. The certificate
/// still carries the *old* loop's hash, so the binding must break.
fn shrink_first_bound(loop_json: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(loop_json).expect("loop json");
    let ivs = v.get_mut("ivs").expect("an `ivs` array");
    let first = ivs.get_mut(0).expect("at least one IV");
    let upper = first.get_mut("upper").expect("an `upper`");
    let n = upper.as_i64().expect("an integer bound");
    *upper = serde_json::json!(n - 1);
    serde_json::to_string_pretty(&v).expect("reserialize")
}

/// Bend the first claim the certificate makes, so it no longer follows from the
/// loop: a trip count's `count`, or an `Exact` distance vector's first component.
///
/// Either way the checker has to rebuild the fact — `trip_count` from the carried
/// CFG, the distance from the two accesses — so a bent claim must be refused. The
/// previous version of this helper swapped the `pair` instead, and the checker
/// *accepted* it: a pair is a locator, not a fact, and swapping it names the same
/// two accesses in the other order. That is the reason this tamper bites.
fn bend_first_claim(cert_json: &str) -> Option<String> {
    fn bend(v: &mut serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(m) => {
                let kind = m.get("kind").and_then(|k| k.as_str()).map(str::to_string);
                if let Some(kind) = kind {
                    if let Some(val) = m.get_mut("value") {
                        match kind.as_str() {
                            "TripCount" => {
                                if let Some(c) =
                                    val.as_object_mut().and_then(|o| o.get_mut("count"))
                                {
                                    if let Some(n) = c.as_i64() {
                                        *c = serde_json::json!(n + 1);
                                        return true;
                                    }
                                }
                            }
                            "Exact" => {
                                if let Some(first) = val.as_array_mut().and_then(|a| a.first_mut()) {
                                    if let Some(n) = first.as_i64() {
                                        *first = serde_json::json!(n + 1);
                                        return true;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                m.values_mut().any(bend)
            }
            serde_json::Value::Array(a) => a.iter_mut().any(bend),
            _ => false,
        }
    }
    let mut v: serde_json::Value = serde_json::from_str(cert_json).ok()?;
    if bend(&mut v) {
        serde_json::to_string_pretty(&v).ok()
    } else {
        None
    }
}

/// The first fixture run that produced a certificate — what the negatives need.
fn some_run_with_a_certificate() -> Option<(Run, String)> {
    for (name, loops) in lowered_fixtures() {
        for (i, l) in loops.iter().enumerate() {
            let run = decide(&format!("neg-{name}{i}"), l);
            if let Some(c) = run.cert() {
                return Some((run, c));
            }
        }
    }
    None
}

#[test]
fn a_loop_that_does_not_match_the_certificate_is_rejected() {
    let (run, _cert) = some_run_with_a_certificate().expect("a decided fixture");
    let honest = std::fs::read_to_string(run.loop_path()).expect("loop json");
    let tampered = run.dir.join("loop-tampered.json");
    std::fs::write(&tampered, shrink_first_bound(&honest)).expect("write tampered loop");
    let (ok, msg) = check(&tampered, &run.cert_path());
    assert!(!ok && msg.starts_with("REJECT"), "expected rejection, got: {msg}");
}

#[test]
fn a_certificate_claim_that_does_not_follow_is_rejected() {
    let (run, cert) = some_run_with_a_certificate().expect("a decided fixture");
    let tampered = bend_first_claim(&cert).expect("a claim to bend");
    assert_ne!(tampered, cert, "the tamper has to change something");
    let path = run.dir.join("cert-tampered.json");
    std::fs::write(&path, tampered).expect("write tampered cert");
    let (ok, msg) = check(&run.loop_path(), &path);
    assert!(!ok && msg.starts_with("REJECT"), "expected rejection, got: {msg}");
}
