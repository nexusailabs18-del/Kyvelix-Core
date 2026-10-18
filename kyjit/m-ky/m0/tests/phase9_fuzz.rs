//! Phase 9 §9.12 — differential fuzzing of the lowering.
//!
//! The oracle is the **generator's own bookkeeping**: each case is built from a
//! known nest depth, known IV bounds and known affine index expressions, so the
//! `Loop` the case must lower to is computed from the generator's parameters
//! rather than from the lowering's output. Comparing the two is what makes this a
//! test of lowering instead of a test of itself.
//!
//! Three properties are exercised:
//!
//! 1. **Differential** — a generated nest lowers to exactly the IV chain and
//!    access coefficients that were emitted, padded to the nest's depth.
//! 2. **Robustness** — arbitrary text never panics the parser or the lowering;
//!    the answer is a `Result`, never an abort.
//! 3. **Partial analysis** — a file holding one good nest *and* one construct
//!    outside §9.14 keeps the nest and names the construct. Both facts survive,
//!    which is the rule §9.14 states and the one a whole-file refusal would break.

use m0::frontend::lower::{lower_program, lower_source, Env};
use m0::frontend::parse;
use m0::ir::{AccessKind, Loop, OffsetValue};

const CASES: u64 = 400;

/// xorshift64: deterministic, and no dependency on a RNG crate.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// One access the generator emitted, and what it must lower to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Emitted {
    base: String,
    /// The coefficient of the emitted level's own IV, at that level's position.
    coef: i64,
    offset: i64,
    /// 0-based level at which the statement was emitted.
    level: usize,
    /// True when the source wrote a read (`= B[...]`), false for a bare write.
    read: bool,
}

/// A generated program, the accesses it contains, and the nest's depth.
fn generate(seed: u64) -> (String, Vec<Emitted>, usize) {
    let mut r = Rng::new(seed);
    let depth = 1 + r.below(3) as usize; // 1..=3
    let ivs = ["i", "j", "k"];
    let bases = ["A", "B", "C"];
    let mut src = String::from("function f(A, B, C) {\n");
    let mut emitted = Vec::new();
    // How many statements each level carries, decided up front so the chain's
    // braces can be opened before any statement is written. Emitting each loop's
    // closing brace as its statements finish would make siblings, not a nest —
    // which is exactly what the first version of this generator did.
    let counts: Vec<u64> = (0..depth).map(|_| 1 + r.below(3)).collect();

    for level in 0..depth {
        let iv = ivs[level];
        src.push_str(&format!(
            "{}for (let {iv} = 0; {iv} < 8; {iv}++) {{\n",
            "  ".repeat(level + 1)
        ));
    }

    for level in 0..depth {
        let iv = ivs[level];
        for _ in 0..counts[level] {
            let base = bases[r.below(bases.len() as u64) as usize];
            let scale = 1 + r.below(2) as i64; // 1 or 2
            let c = r.below(3) as i64 - 1; // -1, 0 or 1
            let idx = match (scale, c) {
                (1, 0) => iv.to_string(),
                (s, 0) => format!("{s} * {iv}"),
                (1, k) if k > 0 => format!("{iv} + {k}"),
                (1, k) => format!("{iv} - {}", -k),
                (s, k) if k > 0 => format!("{s} * {iv} + {k}"),
                (s, k) => format!("{s} * {iv} - {}", -k),
            };
            // A read on the right-hand side exercises the other access kind.
            let read = r.below(3) == 0;
            if read {
                src.push_str(&format!("{}{base}[{idx}] = A[0] + 1;\n", "  ".repeat(level + 2)));
                // The RHS reads `A[0]` as well as the target write.
                emitted.push(Emitted { base: "A".into(), coef: 0, offset: 0, level, read: true });
            } else {
                src.push_str(&format!("{}{base}[{idx}] = 0;\n", "  ".repeat(level + 2)));
            }
            emitted.push(Emitted { base: base.into(), coef: scale, offset: c, level, read: false });
        }
    }

    // Close the chain, innermost first.
    for level in (0..depth).rev() {
        src.push_str(&format!("{}}}\n", "  ".repeat(level + 1)));
    }
    src.push_str("}\n");
    (src, emitted, depth)
}

/// The `(base, index, offset, is_read)` the lowering must report, sorted the way
/// `close_nest` sorts (base, kind, index, offset-as-text).
fn expected(emitted: &[Emitted], depth: usize) -> Vec<(String, Vec<i64>, i64, bool)> {
    // `A[0]` on a right-hand side is a separate access per statement that used it,
    // and each such statement also writes its own target.
    let mut v: Vec<(String, Vec<i64>, i64, bool)> = Vec::new();
    for e in emitted {
        let mut idx = vec![0i64; depth];
        idx[e.level] = e.coef;
        v.push((e.base.clone(), idx, e.offset, e.read));
    }
    v.sort();
    v
}

/// What the lowering actually reported.
fn actual(loop_: &Loop) -> Vec<(String, Vec<i64>, i64, bool)> {
    let mut v: Vec<(String, Vec<i64>, i64, bool)> = loop_
        .body
        .iter()
        .map(|a| {
            let off = match &a.offset {
                OffsetValue::Const(c) => *c,
                other => panic!("unexpected offset {other:?}"),
            };
            (a.base.clone(), a.index.clone(), off, matches!(a.kind, AccessKind::Read))
        })
        .collect();
    v.sort();
    v
}

// ---- property 1: the generated nest lowers to what was emitted ----------------

#[test]
fn a_generated_nest_lowers_to_what_the_generator_emitted() {
    let env = Env::empty();
    let mut cases = 0usize;
    let mut accesses = 0usize;
    for seed in 1..=CASES {
        let (src, emitted, depth) = generate(seed);
        let k = lower_source(&src, &env)
            .unwrap_or_else(|e| panic!("seed {seed}: did not parse: {e}\n{src}"));
        let skipped: Vec<String> = k.skipped.iter().map(|s| format!("{}", s.reason)).collect();
        assert_eq!(skipped, Vec::<String>::new(), "seed {seed}: clean nest refused\n{src}");
        assert_eq!(k.loops.len(), 1, "seed {seed}: one nest\n{src}");
        let l = &k.loops[0];
        assert_eq!(l.depth, depth, "seed {seed}\n{src}");
        let want_ivs: Vec<(String, i64, i64, i64)> = (0..depth)
            .map(|i| (["i", "j", "k"][i].to_string(), 0, 8, 1))
            .collect();
        let got_ivs: Vec<(String, i64, i64, i64)> = l
            .ivs
            .iter()
            .map(|iv| (iv.name.clone(), iv.lower, iv.upper, iv.step))
            .collect();
        assert_eq!(got_ivs, want_ivs, "seed {seed}\n{src}");
        assert_eq!(actual(l), expected(&emitted, depth), "seed {seed}\n{src}");
        assert!(l.validate().is_ok(), "seed {seed}: the deciders' precondition\n{src}");
        cases += 1;
        accesses += l.body.len();
    }
    assert_eq!(cases, CASES as usize);
    assert!(
        accesses > CASES as usize,
        "non-vacuity: the corpus produced only {accesses} accesses for {CASES} cases"
    );
}

// ---- property 2: arbitrary text is a `Result`, never a crash -------------------

#[test]
fn arbitrary_text_never_panics_the_frontend() {
    // Garbage in, an answer out. A frontend handed a file it cannot state has to
    // say so; a panic is a crash on input a compiler is expected to survive.
    let alphabet = b"for(){}=[];,const vof<>+-*/abcdefghij\n0123456789`'\"\\ ";
    for seed in 1..=CASES {
        let mut r = Rng::new(seed.wrapping_mul(7919));
        let n = 1 + r.below(200) as usize;
        let s: String = (0..n)
            .map(|_| alphabet[r.below(alphabet.len() as u64) as usize] as char)
            .collect();
        let _ = parse(&s);
        let _ = lower_source(&s, &Env::empty());
    }
}

// ---- property 3: §9.14's rule is per construct --------------------------------

#[test]
fn a_second_construct_keeps_the_good_nest_and_names_the_bad_one() {
    let bad = [
        "while (a) { }",
        "do { } while (a);",
        "for (const q in A) { }",
        "for (const v of A) { }",
        "switch (a) { }",
        "try { } catch (e) { }",
        "class C { }",
        "let [p, q] = A;",
        "B[x] = 0;",
    ];
    for (i, b) in bad.iter().enumerate() {
        let src =
            format!("function f(A) {{\n  for (let i = 0; i < 8; i++) {{ A[i] = 0; }}\n  {b}\n}}\n");
        let k = lower_source(&src, &Env::empty()).expect("parses");
        assert_eq!(k.loops.len(), 1, "case {i}: the good nest survives `{b}`\n{src}");
        assert_eq!(k.skipped.len(), 1, "case {i}: `{b}` is named once\n{src}");
        assert!(
            !format!("{}", k.skipped[0].reason).is_empty(),
            "case {i}: the refusal carries a reason"
        );
    }
}

#[test]
fn the_two_entry_points_agree_where_nothing_is_skipped() {
    for seed in 1..=40 {
        let (src, _, _) = generate(seed);
        let lenient = lower_source(&src, &Env::empty()).expect("parses");
        let again = lower_program(&parse(&src).expect("parses"), &Env::empty());
        assert_eq!(lenient, again, "seed {seed}: lowering is not deterministic");
        if lenient.skipped.is_empty() {
            let strict = m0::frontend::lower_strict(&parse(&src).expect("parses"), &Env::empty())
                .unwrap_or_else(|e| panic!("seed {seed}: strict refused a clean nest: {e}"));
            assert_eq!(strict, lenient.loops, "seed {seed}");
        }
    }
}
