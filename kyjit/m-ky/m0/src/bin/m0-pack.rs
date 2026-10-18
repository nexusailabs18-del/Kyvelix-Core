//! Pack a directory of bincode loops into one file.
//! Usage: m0-pack <dir> <out.pack>

use std::env;
use std::fs;
use std::io::Write;

use m0::ir::*;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: m0-pack <dir> <out.pack>");
        std::process::exit(2);
    }
    let dir = &args[1];
    let out = &args[2];

    let mut entries: Vec<_> = fs::read_dir(dir).unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "bin"))
        .collect();
    entries.sort();

    let mut file = fs::File::create(out).expect("create pack");
    file.write_all(&(entries.len() as u32).to_le_bytes()).unwrap();

    for path in &entries {
        let bytes = fs::read(path).expect("read bin");
        file.write_all(&(bytes.len() as u32).to_le_bytes()).unwrap();
        file.write_all(&bytes).unwrap();
    }

    println!("Packed {} loops into {}", entries.len(), out);
}
