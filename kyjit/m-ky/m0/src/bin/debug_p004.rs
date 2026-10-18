use std::fs;
use m0::ir::*;
use m0::decide::m1::{extract_param_distance_1d, analyze_distance_level2, analyze_distance};

fn main() {
    let json = fs::read_to_string("tests/param_loops/p004.json").unwrap();
    let lp: LoopParam = serde_json::from_str(&json).unwrap();

    println!("--- offsets ---");
    for (i, a) in lp.body.iter().enumerate() {
        println!("access {}: offset = {:?}", i, a.offset);
    }

    println!("--- indices ---");
    for (i, a) in lp.body.iter().enumerate() {
        println!("access {}: index = {:?}", i, a.index);
    }

    println!("--- extract_param_distance_1d(0,1) ---");
    println!("{:?}", extract_param_distance_1d(&lp, 0, 1));

    println!("--- extract_param_distance_1d(1,0) ---");
    println!("{:?}", extract_param_distance_1d(&lp, 1, 0));

    println!("--- analyze_distance(0,1) ---");
    println!("{:?}", analyze_distance(&lp, 0, 1));

    println!("--- analyze_distance_level2(0,1) ---");
    println!("{:?}", analyze_distance_level2(&lp, 0, 1));
}
