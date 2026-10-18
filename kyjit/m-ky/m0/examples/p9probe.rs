//! Depth-limit probe for the Phase 9 parser (§9.3).
//!
//! The recursion the guard in [`m0::frontend::parser`] bounds is a machine-stack
//! recursion, and the only honest way to pick a ceiling is to find where the
//! process actually aborts. This builds one deeply-nested shape and reports
//! whether the parser returned or died, so `MAX_DEPTH` can be set from a
//! measurement rather than a guess.
//!
//! ```text
//! cargo run -q --example p9probe -- parens 500
//! cargo run -q --example p9probe -- interp 300
//! ```

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind: String = args.get(1).cloned().unwrap_or_else(|| "parens".into());
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    let src = build(&kind, n);
    // `stack` runs the parse on a worker thread with an N KiB stack, which is
    // how the test harness runs it: libtest threads are not the main thread, so a
    // ceiling that is safe here must be safe in 2 MiB, not in the 8 MiB default.
    let stack: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2 * 1024);
    let label = kind.clone();
    let out = std::thread::Builder::new()
        .stack_size(stack * 1024)
        .spawn(move || run(&label, n, &src))
        .unwrap()
        .join();
    match out {
        Ok(line) => println!("{line}"),
        Err(_) => println!("{kind} {n}: ABORT (stack overflow)"),
    }
}

fn build(kind: &str, n: usize) -> String {
    match kind {
        // Each of these recurses through parse_unary once per level.
        "parens" => format!("let x = {}1{};", "(".repeat(n), ")".repeat(n)),
        "unary" => format!("let x = {}1;", "!".repeat(n)),
        "arr" => format!("let x = {}1{};", "[".repeat(n), "]".repeat(n)),
        // Object literals nest by property value, not by wrapping parens.
        "obj" => format!("let x = {}1{};", "{a:".repeat(n), "}".repeat(n)),
        "cond" => format!("let x = {}1;", "a?b:".repeat(n)),
        // This one does not: a member chain is a loop, not recursion.
        "chain" => format!("let x = a{};", ".b".repeat(n)),
        // Statement nesting.
        "nest" => format!("{}{}", "for (;;) {".repeat(n), "}".repeat(n)),
        // The other block-carrying statements, measured separately because each
        // kind has its own frame cost and the cap has to clear the worst of them.
        "ifnest" => format!("{}{}", "if (a) {".repeat(n), "}".repeat(n)),
        "whilenest" => format!("{}{}", "while (a) {".repeat(n), "}".repeat(n)),
        "blocknest" => format!("{}{}", "{".repeat(n), "}".repeat(n)),
        "trynest" => format!("{}1;{}", "try {".repeat(n), "} catch (e) { 1; }".repeat(n)),
        // A call whose callee is a call, so the argument list re-enters.
        "args" => format!("let x = {}1{};", "f(".repeat(n), ")".repeat(n)),
        // A ternary whose branch is a sequence: two independent recursions per
        // level, so this reaches the ceiling faster than `parens` does.
        "seq" => format!("let x = {}1;", "(a,b)?(c,d):".repeat(n)),
        // Interpolation re-entry. A nested *literal* (`${`${x}`}``) is not
        // lexable — the lexer ends the span at the first backtick — so the
        // reachable re-entry is one interpolation whose expression nests:
        // `${(((...)))}`. That substring used to be parsed by a top-level
        // `parse()` call, which started a fresh depth counter at zero and so
        // reset [`MAX_DEPTH`] at every interpolation; the guard only counts
        // correctly once the outer parser's depth is carried in.
        "interp" => {
            let mut s = String::from("let x = `${");
            s.push_str(&"(".repeat(n));
            s.push('1');
            s.push_str(&")".repeat(n));
            format!("{s}}}`;")
        }
        // Interpolation inside interpolation's own nesting, as a control: this is
        // the shape that stays shallow in depth but is long in text.
        "interpsib" => format!("let x = `{}`;", "${a}".repeat(n)),
        // The exact shapes phase9_kernels' deep_nesting test feeds the parser.
        "testchain" => "let x = a.b.c.d.e;".repeat(n),
        "testnest" => format!("{}{}", "for (;;) {".repeat(n), "}".repeat(n)),
        other => panic!("unknown shape {other}"),
    }
}

fn run(kind: &str, n: usize, src: &str) -> String {
    match m0::frontend::parser::parse(src) {
        Ok(_) => format!("{kind} {n}: OK"),
        Err(e) => format!("{kind} {n}: ERR {e}"),
    }
}

