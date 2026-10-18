//! M1 Level 1 CLI: analyze the parametric distance of a loop.
//! Usage: m1-distance <loop.json>

use std::env;
use std::fs;

use m0::decide::m1::{analyze_distance_level2, ParametricDistance};
use m0::ir::*;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: m1-distance <loop.json>");
        std::process::exit(2);
    }

    let json_str = fs::read_to_string(&args[1]).expect("read loop");
    let loop_param: LoopParam = serde_json::from_str(&json_str).expect("parse LoopParam");

    println!("Loop: depth={}, parametric={}", loop_param.depth, loop_param.is_parametric());

    let n = loop_param.body.len();
    for p in 0..n {
        for q in 0..n {
            if p == q { continue; }
            match analyze_distance_level2(&loop_param, p, q) {
                ParametricDistance::Const(d) => {
                    println!("pair ({}, {}): CONST {:?}", p, q, d);
                }
                ParametricDistance::Function(f) => {
                    println!("pair ({}, {}): FUNCTION {}", p, q, f);
                }
                ParametricDistance::None => {
                    println!("pair ({}, {}): NONE", p, q);
                }
                ParametricDistance::Unknown(r) => {
                    println!("pair ({}, {}): UNKNOWN ({})", p, q, r);
                }
            }
        }
    }
}
