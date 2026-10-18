//! Arena-based packed processor.
//! Parses bincode into an arena, processes, resets per batch.
//! Usage: m0-arena [OPTIONS] <file.pack>

use std::env;
use std::fs;
use std::time::Instant;

use rayon::prelude::*;

use m0::arena::Arena;
use m0::arena_parse::parse_loop;
use m0::config::Config;
use m0::decide_ref::decide_ref;
use m0::decide::Decision;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut cli_threads: Option<usize> = None;
    let mut cli_batch: Option<usize> = None;
    let mut show_config = false;
    let mut input_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--show-config" { show_config = true; }
        else if let Some(v) = a.strip_prefix("--threads=") { cli_threads = v.parse().ok(); }
        else if let Some(v) = a.strip_prefix("--batch=") { cli_batch = v.parse().ok(); }
        else if a == "--help" || a == "-h" {
            eprintln!("usage: m0-arena [OPTIONS] <file.pack>");
            std::process::exit(0);
        } else if input_path.is_none() {
            input_path = Some(a.clone());
        }
        i += 1;
    }

    let cfg = Config::load(cli_threads, cli_batch);
    if show_config { cfg.show(); return; }

    let n = cfg.resolved_threads();
    env::set_var("RAYON_NUM_THREADS", n.to_string());

    let path = match input_path {
        Some(p) => p,
        None => { eprintln!("usage: m0-arena [OPTIONS] <file.pack>"); std::process::exit(2); }
    };

    let t0 = Instant::now();
    let data = fs::read(&path).expect("read pack");
    let t_read = t0.elapsed();

    let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut slices: Vec<(usize, usize)> = Vec::with_capacity(count);
    let mut pos = 4;
    for _ in 0..count {
        let len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        slices.push((pos, len));
        pos += len;
    }
    let t_index = t0.elapsed();

    let batch = cfg.batch_size.max(1);

    let results: Vec<(u64, u64, u64, u64)> = slices
        .par_chunks(batch)
        .map(|chunk| {
            // One arena per work item
            let arena = Arena::with_capacity(4 * 1024 * 1024);

            let mut e = 0u64;
            let mut n = 0u64;
            let mut u = 0u64;
            let mut c = 0u64;

            for (off, len) in chunk {
                match parse_loop(&arena, &data[*off..*off+*len]) {
                    Some(loop_ref) => {
                        let nacc = loop_ref.body.len();
                        let mut overall: &str = "NONE";
                        for p in 0..nacc {
                            for q in 0..nacc {
                                if p == q { continue; }
                                match decide_ref(&loop_ref, p, q, &arena) {
                                    Decision::Exact(_) => overall = "EXACT",
                                    Decision::None => {}
                                    Decision::Unknown(_) => {
                                        if overall != "EXACT" { overall = "UNKNOWN"; }
                                    }
                                }
                            }
                        }
                        match overall {
                            "EXACT" => e += 1,
                            "NONE" => n += 1,
                            _ => u += 1,
                        }
                    }
                    None => c += 1,
                }
                // Reset arena between loops (each is independent)
                arena.reset();
            }

            (e, n, u, c)
        })
        .collect();

    let (exact, none, unknown, crashes) = results.iter().fold((0u64,0u64,0u64,0u64), |acc, r| {
        (acc.0 + r.0, acc.1 + r.1, acc.2 + r.2, acc.3 + r.3)
    });

    let t_total = t0.elapsed();
    let per_loop_ns = t_total.as_nanos() / count.max(1) as u128;

    println!("Loops:    {}", count);
    println!("EXACT:    {}", exact);
    println!("NONE:     {}", none);
    println!("UNKNOWN:  {}", unknown);
    println!("Crashes:  {}", crashes);
    println!("Threads:  {}", rayon::current_num_threads());
    println!("Batch:    {}", batch);
    println!("read:     {:?}", t_read);
    println!("index:    {:?}", t_index - t_read);
    println!("decide:   {:?}", t_total - t_index);
    println!("Total:    {:?}", t_total);
    println!("Per loop: {} ns ({} us)", per_loop_ns, per_loop_ns / 1000);
}
