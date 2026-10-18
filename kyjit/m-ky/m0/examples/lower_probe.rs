use m0::frontend::lower::{lower_program, Env};
use m0::frontend::parse;

fn main() {
    for name in ["syr2k", "triangle", "jaccard", "mix", "stencil2d"] {
        let src = match name {
            "syr2k" => include_str!("../tests/js_kernels/syr2k.js"),
            "triangle" => include_str!("../tests/js_kernels/triangle.js"),
            "jaccard" => include_str!("../tests/js_kernels/jaccard.js"),
            "mix" => include_str!("../tests/js_kernels/mix.js"),
            _ => include_str!("../tests/js_kernels/stencil2d.js"),
        };
        let env = Env::of(&[("n", 8), ("ld", 8), ("i", 4), ("j", 2)]);
        let prog = parse(src).unwrap();
        let k = lower_program(&prog, &env);
        println!("===== {name}: {} loop(s), {} skipped", k.loops.len(), k.skipped.len());
        for l in &k.loops {
            println!("  depth {} ivs {:?}", l.depth,
                l.ivs.iter().map(|iv| (iv.name.clone(), iv.lower, iv.upper, iv.step)).collect::<Vec<_>>());
            for (i, a) in l.body.iter().enumerate() {
                println!("    [{i}] {:?} {} index {:?} offset {:?} guard {:?}",
                    a.kind, a.base, a.index, a.offset, a.guard);
            }
        }
        for s in &k.skipped {
            println!("  SKIP {}:{} {}", s.pos.line, s.pos.col, s.reason);
        }
    }
}
