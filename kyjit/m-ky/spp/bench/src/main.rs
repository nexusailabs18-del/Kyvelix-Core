//! SPP Measurement Harness - Phase 2 Runner
//!
//! Runs the measurement harness for SPP kernels.

mod protocol;
mod generate;
mod csv;
mod validity;
mod sanity;
mod fast_corpus;

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <command>", args[0]);
        eprintln!("Commands: generate, run, sanity, quick");
        process::exit(1);
    }

    match args[1].as_str() {
        "generate" => {
            protocol::run_measurements();
        }
        "run" => {
            protocol::run_measurements();
        }
        "sanity" => {
            sanity::run_sanity_checks();
        }
        "quick" => {
            fast_corpus::generate_fast_corpus().expect("Fast corpus generation failed");
            // Also run sanity on fast corpus
            sanity::run_sanity_checks();
        }
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}