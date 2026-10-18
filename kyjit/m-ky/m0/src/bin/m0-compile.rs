//! Convert a directory of JSON loops to bincode loops.
//! Usage: m0-compile <input_dir> <output_dir>

use std::env;
use std::fs;
use std::path::PathBuf;

use m0::ir::*;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: m0-compile <in_dir> <out_dir>");
        std::process::exit(2);
    }
    let in_dir = &args[1];
    let out_dir = &args[2];
    fs::create_dir_all(out_dir).expect("create out dir");

    let mut count = 0;
    for entry in fs::read_dir(in_dir).expect("read in dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "json") {
            let json_str = fs::read_to_string(&path).expect("read json");
            let loop_: Loop = serde_json::from_str(&json_str).expect("parse json");
            let bytes = loop_.to_bincode();

            let mut out_path = PathBuf::from(out_dir);
            let filename = path.file_stem().unwrap().to_string_lossy().to_string();
            out_path.push(format!("{}.bin", filename));
            fs::write(&out_path, bytes).expect("write bin");
            count += 1;
        }
    }
    println!("Compiled {} loops to bincode in {}", count, out_dir);
}
