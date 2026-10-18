//! Parallel batch mode using rayon (Lever 3).
//! Usage: m0-parallel <dir> [--bincode]

use std::env;
use std::fs;
use std::time::Instant;

use rayon::prelude::*;

use m0::decide::{decide, Decision};
use m0::ir::*;

#[derive(Copy, Clone)]
enum Format { Json, Bincode }

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: m0-parallel <dir> [--bincode]");
        std::process::exit(2);
    }
    let dir = &args[1];
    let format = if args.len() >= 3 && args[2] == "--bincode" {
        Format::Bincode
    } else {
        Format::Json
    };

    let t0 = Instant::now();

    let ext = match format {
        Format::Json => "json",
        Format::Bincode => "bin",
    };

    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == ext))
        .collect();
    entries.sort();

    let results: Vec<(u64, u64, u64, u64)> = entries
        .par_iter()
        .map(|path| {
            let loop_ = match format {
                Format::Json => {
                    let s = match fs::read_to_string(path) { Ok(s) => s, Err(_) => return (0,0,0,1) };
                    match serde_json::from_str::<Loop>(&s) { Ok(l) => l, Err(_) => return (0,0,0,1) }
                }
                Format::Bincode => {
                    let b = match fs::read(path) { Ok(b) => b, Err(_) => return (0,0,0,1) };
                    match Loop::from_bincode(&b) { Ok(l) => l, Err(_) => return (0,0,0,1) }
                }
            };
            if loop_.validate().is_err() {
                return (0, 0, 0, 1);
            }
            let n = loop_.body.len();
            let mut overall: &str = "NONE";
            for p in 0..n {
                for q in 0..n {
                    if p == q { continue; }
                    match decide(&loop_, p, q).0 {
                        Decision::Exact(_) => overall = "EXACT",
                        Decision::None => {}
                        Decision::Unknown(_) => {
                            if overall != "EXACT" { overall = "UNKNOWN"; }
                        }
                    }
                }
            }
            match overall {
                "EXACT" => (1, 0, 0, 0),
                "NONE" => (0, 1, 0, 0),
                _ => (0, 0, 1, 0),
            }
        })
        .collect();

    let (exact, none, unknown, crashes) = results.iter().fold((0u64,0u64,0u64,0u64), |acc, r| {
        (acc.0 + r.0, acc.1 + r.1, acc.2 + r.2, acc.3 + r.3)
    });

    let count = exact + none + unknown;
    let t_total = t0.elapsed();
    let per_loop_ns = t_total.as_nanos() / count.max(1) as u128;

    println!("Loops:    {}", count);
    println!("EXACT:    {}", exact);
    println!("NONE:     {}", none);
    println!("UNKNOWN:  {}", unknown);
    println!("Crashes:  {}", crashes);
    println!("Threads:  {}", rayon::current_num_threads());
    println!("Total:    {:?}", t_total);
    println!("Per loop: {} ns ({} us)", per_loop_ns, per_loop_ns / 1000);
}
