use std::env;
use std::fs;
use std::time::Instant;

use m0::decide::{decide, Decision};
use m0::ir::*;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: m0-batch <directory>");
        std::process::exit(2);
    }
    let dir = &args[1];

    let t0 = Instant::now();
    let mut count: u64 = 0;
    let mut exact: u64 = 0;
    let mut none: u64 = 0;
    let mut unknown: u64 = 0;
    let mut crashes: u64 = 0;

    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "json"))
        .collect();
    entries.sort();

    for path in entries {
        let loop_str = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                crashes += 1;
                continue;
            }
        };
        let loop_: Loop = match serde_json::from_str(&loop_str) {
            Ok(l) => l,
            Err(_) => {
                crashes += 1;
                continue;
            }
        };
        if loop_.validate().is_err() {
            crashes += 1;
            continue;
        }

        let n = loop_.body.len();
        let mut overall: &str = "NONE";
        for p in 0..n {
            for q in 0..n {
                if p == q {
                    continue;
                }
                match decide(&loop_, p, q).0 {
                    Decision::Exact(_) => overall = "EXACT",
                    Decision::None => {}
                    Decision::Unknown(_) => {
                        if overall != "EXACT" {
                            overall = "UNKNOWN";
                        }
                    }
                }
            }
        }
        match overall {
            "EXACT" => exact += 1,
            "NONE" => none += 1,
            _ => unknown += 1,
        }
        count += 1;
    }

    let t_total = t0.elapsed();
    let per_loop_ns = t_total.as_nanos() / count.max(1) as u128;

    println!("Loops:    {}", count);
    println!("EXACT:    {}", exact);
    println!("NONE:     {}", none);
    println!("UNKNOWN:  {}", unknown);
    println!("Crashes:  {}", crashes);
    println!("Total:    {:?}", t_total);
    println!("Per loop: {} ns ({} us)", per_loop_ns, per_loop_ns / 1000);
}
