use std::env;
use std::fs;

use m0::certificate::Certificate;
use m0::decide::{decide, decide_with_env, Decision};
use m0::ir::*;

/// Parse `M0_ENV="p=8,modulus=4"` into `[("p", 8), ("modulus", 4)]`.
/// Returns an empty vec if the variable is unset or malformed.
fn parse_m0_env() -> Vec<(String, i64)> {
    let raw = match std::env::var("M0_ENV") {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for pair in raw.split(',') {
        let pair = pair.trim();
        if pair.is_empty() { continue; }
        if let Some((k, v)) = pair.split_once('=') {
            if let Ok(n) = v.trim().parse::<i64>() {
                out.push((k.trim().to_string(), n));
            }
        }
    }
    out
}

/// The certificate file's shape, decided once per run.
///
/// `cert.json` has always held a single object, and every harness in `tests/`
/// feeds that file to `m0-check`. Phase 9a adds two *loop-level* claims
/// (`TripCount`, `ScalarDependence`) that are not the access-pair decision's
/// witness and cannot replace it, so they ride along as extra elements:
///
/// - **no evidence** → a bare object, byte-identical to the pre-9a output;
/// - **with evidence** → `[decision_cert, evidence...]`.
///
/// Keeping the single-object form whenever there is nothing new to say means the
/// corpus scripts that only ever saw one certificate are untouched; `m0-check`
/// grew array support for the other case.
fn write_certs(certs: &[Certificate], evidence: &[Certificate]) {
    let total = certs.len() + evidence.len();
    if total == 1 {
        // Exactly one certificate (whether it is the decision or an evidence
        // claim) keeps the bare-object shape the corpus scripts expect.
        let single = certs.first().or_else(|| evidence.first()).unwrap();
        let out = serde_json::to_string_pretty(single).unwrap();
        fs::write("cert.json", out).expect("write cert");
    } else {
        let mut all: Vec<&Certificate> = Vec::with_capacity(total);
        all.extend(certs.iter());
        all.extend(evidence.iter());
        let out = serde_json::to_string_pretty(&all).unwrap();
        fs::write("cert.json", out).expect("write cert");
    }
    println!("wrote cert.json");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: m0-decide <loop.json>");
        std::process::exit(2);
    }

    let t0 = std::time::Instant::now();
    let loop_str = fs::read_to_string(&args[1]).expect("read loop");
    let t_read = t0.elapsed();

    let loop_: Loop = serde_json::from_str(&loop_str).expect("parse loop");
    let t_parse = t0.elapsed();

    if let Err(e) = loop_.validate() {
        eprintln!("invalid loop: {}", e);
        std::process::exit(2);
    }
    let t_validate = t0.elapsed();

    let n = loop_.body.len();
    let mut all_certs = vec![];
    let mut overall = "NONE";

    for p in 0..n {
        for q in 0..n {
            if p == q {
                continue;
            }
            let env = parse_m0_env();
            let (d, cert) = decide_with_env(&loop_, p, q, &env);
            match d {
                Decision::Exact(dist) => {
                    println!("pair ({}, {}): EXACT distance {:?}", p, q, dist);
                    overall = "EXACT";
                    if let Some(c) = cert {
                        all_certs.push(c);
                    }
                }
                Decision::None => {
                    println!("pair ({}, {}): NONE", p, q);
                    if let Some(c) = cert {
                        all_certs.push(c);
                    }
                }
                Decision::Unknown(r) => {
                    println!("pair ({}, {}): UNKNOWN ({})", p, q, r);
                    if overall != "EXACT" {
                        overall = "UNKNOWN";
                    }
                }
            }
        }
    }

    println!("overall: {}", overall);

    // Phase 9a §4.2: the loop-level claims a decided loop also warrants. These
    // ride alongside the access-pair certificates rather than replacing one —
    // a trip count is not evidence about a pair of accesses.
    let evidence = m0::decide::decide_evidence(&loop_);

    if !all_certs.is_empty() {
        write_certs(&all_certs, &evidence);
    } else if !evidence.is_empty() {
        write_certs(&evidence, &[]);
    }

    let t_total = t0.elapsed();
    if std::env::var("M0_TIMING").is_ok() {
        eprintln!(
            "timing: read={:?} parse={:?} validate={:?} total={:?}",
            t_read,
            t_parse - t_read,
            t_validate - t_parse,
            t_total
        );
    }
}
