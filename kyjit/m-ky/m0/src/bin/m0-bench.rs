use std::env;
use std::fs;
use std::time::Instant;

use m0::decide::decide;
use m0::ir::Loop;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: m0-bench <dir> [reps]");
        std::process::exit(2);
    }
    let dir = &args[1];
    let reps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);

    let mut loops: Vec<Loop> = Vec::new();
    for entry in fs::read_dir(dir).expect("read dir") {
        let p = entry.expect("entry").path();
        if p.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(s) = fs::read_to_string(&p) {
                if let Ok(l) = serde_json::from_str::<Loop>(&s) {
                    loops.push(l);
                }
            }
        }
    }
    if loops.is_empty() {
        eprintln!("no loops in {}", dir);
        std::process::exit(2);
    }

    for l in &loops {
        let n = l.body.len();
        for p in 0..n {
            for q in 0..n {
                if p != q { let _ = decide(l, p, q); }
            }
        }
    }

    let t0 = Instant::now();
    let mut decisions: u64 = 0;
    for _ in 0..reps {
        for l in &loops {
            let n = l.body.len();
            for p in 0..n {
                for q in 0..n {
                    if p == q { continue; }
                    let _ = decide(l, p, q);
                    decisions += 1;
                }
            }
        }
    }
    let t = t0.elapsed();

    let ns_per_decision = t.as_nanos() as f64 / decisions as f64;
    let ns_per_loop = t.as_nanos() as f64 / (reps as f64 * loops.len() as f64);

    println!("dir:             {}", dir);
    println!("loops:           {}", loops.len());
    println!("reps:            {}", reps);
    println!("decisions:       {}", decisions);
    println!("total:           {:?}", t);
    println!("ns/decision:     {:.0}", ns_per_decision);
    println!("ns/loop:         {:.0}  ({:.3} µs)", ns_per_loop, ns_per_loop / 1000.0);
}
