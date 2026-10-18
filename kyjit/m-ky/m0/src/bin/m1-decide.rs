//! M1 CLI: read a parametric loop, resolve params from CLI args, decide.
//! Usage: m1-decide <loop.json> KEY=VAL [KEY=VAL]...

use std::env;
use std::fs;

use m0::decide::{Decision, decide};
use m0::ir::*;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: m1-decide <loop.json> [KEY=VAL]...");
        std::process::exit(2);
    }

    let json_str = fs::read_to_string(&args[1]).expect("read loop");
    let loop_param: LoopParam = serde_json::from_str(&json_str).expect("parse LoopParam");

    let mut param_env: Vec<(String, i64)> = Vec::new();
    for a in &args[2..] {
        if let Some((k, v)) = a.split_once('=') {
            if let Ok(n) = v.parse::<i64>() {
                param_env.push((k.to_string(), n));
            }
        }
    }

    println!("Loop: depth={}, parametric={}", loop_param.depth, loop_param.is_parametric());
    println!("Env: {:?}", param_env);

    match loop_param.resolve(&param_env) {
        Some(resolved) => {
            println!("Resolved: depth={}", resolved.depth);
            for iv in &resolved.ivs {
                println!("  IV {}: [{}..{}) step {}", iv.name, iv.lower, iv.upper, iv.step);
            }

            let n = resolved.body.len();
            let mut overall: &str = "NONE";
            for p in 0..n {
                for q in 0..n {
                    if p == q { continue; }
                    match decide(&resolved, p, q).0 {
                        Decision::Exact(dist) => {
                            println!("pair ({}, {}): EXACT {:?}", p, q, dist);
                            overall = "EXACT";
                        }
                        Decision::None => {
                            println!("pair ({}, {}): NONE", p, q);
                        }
                        Decision::Unknown(r) => {
                            println!("pair ({}, {}): UNKNOWN ({})", p, q, r);
                            if overall != "EXACT" { overall = "UNKNOWN"; }
                        }
                    }
                }
            }
            println!("overall: {}", overall);
        }
        None => {
            println!("Could not resolve parameters (missing values)");
            std::process::exit(1);
        }
    }
}
