//! Phase 9 §9.3 evidence, part 2: the parser against realistic kernel source.
//!
//! The unit tests in `phase9_parser.rs` check one construct at a time. That is
//! necessary but not sufficient: the analyzers consume whole functions, and a
//! parser that passes every isolated rule can still fail on how real code
//! composes them — a `for` body that opens with `{`, a `let` with two
//! declarators, a call whose single argument is an `in` test.
//!
//! So the fixtures under `tests/js_kernels/` are written the way the Phase 1–8
//! corpora write loops: BLAS-like stencils and reductions, each with the
//! dependence structure the analyzers are meant to find. These tests require the
//! parse to *succeed* and to put the loops at the nesting depth the source says.
//!
//! A failure here means the frontend cannot ingest code an analyzer can already
//! reason about, which is the gap Phase 9 exists to close.

use m0::frontend::ast::{BinOp, Expr, ForHead, Program, Stmt};
use m0::frontend::parser::parse;

fn fixture(name: &str) -> &'static str {
    match name {
        "syr2k" => include_str!("js_kernels/syr2k.js"),
        "triangle" => include_str!("js_kernels/triangle.js"),
        "jaccard" => include_str!("js_kernels/jaccard.js"),
        "stencil2d" => include_str!("js_kernels/stencil2d.js"),
        "mix" => include_str!("js_kernels/mix.js"),
        other => panic!("no fixture named {other}"),
    }
}

fn parse_fixture(name: &str) -> Program {
    match parse(fixture(name)) {
        Ok(p) => p,
        Err(e) => panic!("{name}: {e}"),
    }
}

/// The function body of a one-function fixture.
fn kernel_body(name: &str) -> Vec<Stmt> {
    let p = parse_fixture(name);
    match p.body.as_slice() {
        [Stmt::FunctionDecl { body, .. }] => body.clone(),
        other => panic!("{} is not one function: {other:?}", name),
    }
}

/// The loop nest depths in source order, where depth 1 is an outermost loop.
///
/// Depth is what these tests check rather than a loop count: a triply-nested
/// kernel whose innermost loop silently became a statement of the middle one
/// would still have three loops, and still parse. Only the nesting reveals it.
fn loop_depths(body: &[Stmt]) -> Vec<usize> {
    fn walk(s: &Stmt, d: usize, out: &mut Vec<usize>) {
        // (is_this_a_loop, its body) — every other kind recurses through children.
        let inner: Option<(&Stmt, bool)> = match s {
            Stmt::While { body, .. } => Some((body, true)),
            Stmt::DoWhile { body, .. } => Some((body, true)),
            Stmt::For { body, .. } | Stmt::ForOf { body, .. } | Stmt::ForIn { body, .. } => {
                Some((body, true))
            }
            Stmt::If { then, .. } => Some((then, false)),
            _ => None,
        };
        let (is_loop, body) = match inner {
            Some((b, l)) => (l, Some(b)),
            None => (false, None),
        };
        let nd = if is_loop { d + 1 } else { d };
        if is_loop {
            out.push(nd);
        }
        if let Some(b) = body {
            walk(b, nd, out);
        }
        // An `if` also has an else branch that can nest loops.
        if let Stmt::If { els: Some(e), .. } = s {
            walk(e, nd, out);
        }
        // Blocks (including a function body) carry siblings at the same depth.
        if let Some(block) = s.as_block() {
            for c in block {
                walk(c, nd, out);
            }
        }
        if let Stmt::FunctionDecl { body, .. } = s {
            for c in body {
                walk(c, d, out);
            }
        }
    }
    let mut out = Vec::new();
    for s in body {
        walk(s, 0, &mut out);
    }
    out
}
// ---------------------------------------------------------------------------
// Nesting structure of the corpus kernels
// ---------------------------------------------------------------------------

#[test]
fn syr2k_keeps_its_three_level_nest() {
    // The Phase 5 array-of-structs kernel: j, then i, then k, with the k-loop the
    // only reduction-carried one.
    assert_eq!(loop_depths(&kernel_body("syr2k")), vec![1, 2, 3]);
}

#[test]
fn triangle_keeps_its_two_level_nest_with_a_dependent_bound() {
    assert_eq!(loop_depths(&kernel_body("triangle")), vec![1, 2]);
}

#[test]
fn jaccard_keeps_the_for_of_and_its_guard() {
    // §9.7 lowers the `for-of`, and it can only do that from a `ForOf` node: a
    // parser that read `for (const item of setA)` as a classical `for` would
    // produce a loop with no IV to synthesize.
    let body = kernel_body("jaccard");
    assert_eq!(loop_depths(&body), vec![1]);
    assert!(matches!(body[1], Stmt::ForOf { .. }), "for-of lost: {:?}", body[1]);
}

#[test]
fn stencil_reads_four_neighbours_in_one_statement() {
    let body = kernel_body("stencil2d");
    assert_eq!(loop_depths(&body), vec![1, 2]);
    // The five-point statement must stay *one* statement: splitting it would
    // invent intermediate writes the source does not perform, and a dependence
    // reported between two halves of it would be an artifact of the parser.
    // `stencil2d` nests two loops, so the assignment is the first statement of
    // the inner loop's body, one level below `body[0]`.
    let outer = match &body[0] {
        Stmt::For { body, .. } => body,
        other => panic!("outer loop of stencil is {other:?}"),
    };
    let inner = match outer.as_block().expect("outer for body").first() {
        Some(Stmt::For { body, .. }) => body,
        other => panic!("inner loop of stencil is {other:?}"),
    };
    match inner.as_block().expect("inner for body").first() {
        Some(Stmt::Expr(Expr::Assign { target, value, .. })) => {
            assert!(matches!(**target, Expr::Index { .. }));
            // Count the `u` reads in the RHS.
            fn reads(e: &Expr, acc: &mut usize) {
                match e {
                    Expr::Ident(n, _) => *acc += (n == "u") as usize,
                    Expr::Binary { left, right, .. } => {
                        reads(left, acc);
                        reads(right, acc);
                    }
                    Expr::Index { obj, index, .. } => {
                        reads(obj, acc);
                        reads(index, acc);
                    }
                    _ => {}
                }
            }
            let mut n = 0;
            reads(value, &mut n);
            assert_eq!(n, 4, "four neighbour reads expected in the RHS");
        }
        other => panic!("stencil inner statement is {other:?}"),
    }
}

#[test]
fn an_inner_bound_dependant_on_the_outer_iv_is_recognized() {
    // `for (let j = 0; j <= i; j++)`: the bound is the outer IV, which is what
    // makes triangle a parametric rather than a rectangular loop. If the parser
    // folded `<= i` into something else, §9.4 could not name the bound.
    // `body` is `let sum`, the outer `for`, `return`; the loop whose bound is the
    // outer IV is the one *inside* the outer `for`, not the outer `for` itself.
    let body = kernel_body("triangle");
    let outer = match &body[1] {
        Stmt::For { body, .. } => body,
        other => panic!("outer loop of triangle is {other:?}"),
    };
    match outer.as_block().expect("outer for body").first() {
        Some(Stmt::For {
            head: ForHead::Classical { cond: Some(Expr::Binary { op: BinOp::Le, right, .. }), .. },
            ..
        }) => {
            assert!(matches!(&**right, Expr::Ident(n, _) if n == "i"));
        }
        other => panic!("inner loop of triangle is {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The forms a single kernel does not exercise
// ---------------------------------------------------------------------------

#[test]
fn mixed_control_forms_all_appear_in_order() {
    // for-in, a while with a compound guard, do-while, the `i += 2` strided step,
    // and destructuring declarations with and without defaults and a rest.
    let p = parse_fixture("mix");
    let kinds: Vec<&str> = p
        .body
        .iter()
        .map(|s| match s {
            Stmt::Const { .. } => "const",
            Stmt::Let { .. } => "let",
            Stmt::ForIn { .. } => "for-in",
            Stmt::While { .. } => "while",
            Stmt::DoWhile { .. } => "do-while",
            Stmt::For { .. } => "for",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["const", "const", "let", "for-in", "while", "do-while", "for"],
        "mix.js statement kinds"
    );
    assert_eq!(loop_depths(&p.body), vec![1, 1, 1, 1], "four loops, none nested");
}

#[test]
fn a_multi_declarator_let_is_one_statement_of_declarations() {
    // `let t1 = 0, t2 = 0;` is one statement in JS. Both names must be declared
    // before the body uses them and in source order, since §9.6 emits one
    // declaration per name and an out-of-order pair would renumber the slots.
    match parse("let t1 = 0, t2 = 0;").unwrap().body[0].clone() {
        Stmt::Block(v, _) => {
            assert_eq!(v.len(), 2);
            assert!(matches!(v[0], Stmt::Let { .. }));
            assert!(matches!(v[1], Stmt::Let { .. }));
        }
        other => panic!("expected a block of declarations, got {other:?}"),
    }
    // A single declarator must stay a single statement, not gain a block wrapper.
    assert!(matches!(parse("let a = 1;").unwrap().body[0], Stmt::Let { .. }));
}

#[test]
fn an_argument_that_is_an_in_test_stays_one_argument() {
    // `[+In]` inside a call: `f(a in b)` passes the result of the test, not two
    // arguments. Splitting it would change the call's arity in the IR.
    match parse("f(a in b);").unwrap().body[0].clone() {
        Stmt::Expr(Expr::Call { args, .. }) => {
            assert_eq!(args.len(), 1, "`in` split a single argument");
        }
        other => panic!("{other:?}"),
    }
    // And a loop *body* must still allow `for-in` after a `for` header disabled it.
    match parse("for (i = 0; i < n; i++) { for (const k in o) { x = k; } }").unwrap().body[0].clone() {
        Stmt::For { body, .. } => match *body {
            Stmt::Block(v, _) => assert!(matches!(v[0], Stmt::ForIn { .. })),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn compound_and_strided_loop_steps_keep_their_operator() {
    // `i += 2` rather than `i++`: §9.4's IV recognition needs the step value, and
    // desugaring it at parse time would hide whether the source wrote `+=` or
    // `+` with an assignment.
    match parse("for (let i = 0; i < n; i += 2) { a[i] = 1; }").unwrap().body[0].clone() {
        Stmt::For { head: ForHead::Classical { update: Some(Expr::Assign { op, target, value, .. }), .. }, .. } => {
            assert_eq!(op, m0::frontend::ast::AssignOp::Add);
            assert!(matches!(*target, Expr::Ident(ref n, _) if n == "i"));
            assert_eq!(value.as_int(), Some(2));
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Out-of-subset constructs: parsed so they can be named, or refused precisely
// ---------------------------------------------------------------------------

#[test]
fn constructs_the_lowering_refuses_are_parsed_by_name() {
    // §9.14's rule is "compiled correctly but not analyzed", and a refusal that
    // names the construct is only possible if the parser recognized it. These all
    // must be `Ok`; refusing them is §9.4's job.
    for src in [
        "class A { constructor(x) { this.x = x; } m() { return this.x; } }",
        "class B extends A { m(a, b) { return a + b; } }",
        "class C { x = 1; }",
        "try { x = 1; } catch (e) { y = e; } finally { z = 3; }",
        "try { x = 1; } catch { y = 2; }",
        "async function f() { await g(); }",
        "let f = async (x) => x * 2;",
        "let g = async x => x * 2;",
        "switch (x) { case 1: y = 2; break; default: y = 3; }",
        "throw new Error('bad');",
        "let v = `a${x}b`;",
        "let t = typeof x;",
        "for (const k in o) { x = k; }",
    ] {
        if let Err(e) = parse(src) {
            panic!("out-of-subset construct should parse, not error: {src}: {e}");
        }
    }
}

#[test]
fn forms_with_no_analyzable_shape_are_refused_at_parse_time() {
    // The opposite half: these are not in the parsed subset either, because there
    // is no tree that would mean them. Refusing here is cheaper than carrying a
    // variant nothing can lower.
    for src in [
        "function* gen() { yield 1; }",
        "let o = { ...rest, a: 1 };",
        "let o = { [k]: 1 };",
        "let o = { m() { return 1; } };",
        "class C { static m() {} }",
    ] {
        assert!(parse(src).is_err(), "should be refused at parse time: {src}");
    }
}

#[test]
fn class_members_are_read_as_methods_and_fields() {
    // Members must not be parsed as ordinary statements: `m(a, b) { }` at
    // statement position would read as a call expression followed by a block,
    // which is a different program.
    match &parse("class A { constructor(x) { this.x = x; } m() { return 1; } }").unwrap().body[0] {
        Stmt::Class { members, .. } => {
            assert_eq!(members.len(), 2, "{members:?}");
            match &members[0] {
                Stmt::FunctionDecl { name, params, body, .. } => {
                    assert_eq!(name, "constructor");
                    assert_eq!(params.len(), 1);
                    assert_eq!(body.len(), 1);
                }
                other => panic!("constructor parsed as {other:?}"),
            }
            match &members[1] {
                Stmt::FunctionDecl { name, .. } => assert_eq!(name, "m"),
                other => panic!("method parsed as {other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    // A field is an assignment to `this`, which is what it means.
    match &parse("class A { x = 1; }").unwrap().body[0] {
        Stmt::Class { members, .. } => match &members[0] {
            Stmt::Expr(Expr::Assign { target, value, .. }) => {
                assert!(matches!(**target, Expr::Member { ref obj, .. } if matches!(**obj, Expr::This(_))));
                assert_eq!(value.as_int(), Some(1));
            }
            other => panic!("field parsed as {other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Scale: the parser must not be the thing that fails on a real file
// ---------------------------------------------------------------------------

#[test]
fn deep_nesting_terminates_rather_than_overflowing() {
    // Recursive descent on `((((...))))` is the input that turns a parser into a
    // segfault, and a segfault in the frontend is indistinguishable from an
    // analyzer crash in the harness — the harness sees a signal, not a location,
    // and reports the parser as broken. So both halves matter: input this deep has
    // to parse, and input deeper has to come back as a `ParseError`.
    //
    // The below-cap half is the one real code exercises. 200 levels of parens and
    // a 60-deep loop nest are both far past a handwritten kernel but well within a
    // generated or minified one, and a refusal at that depth would be a regression
    // rather than a safety gain.
    let parens = format!("let x = {}1{};", "(".repeat(200), ")".repeat(200));
    assert!(parse(&parens).is_ok());
    let chain = "let x = a.b.c.d.e;".repeat(200);
    assert!(parse(&chain).is_ok());
    let nest = format!("{}{}", "for (;;) {".repeat(60), "}".repeat(60));
    assert_eq!(loop_depths(&parse(&nest).unwrap().body), (1..=60).collect::<Vec<_>>());

    // The above-cap half is the one that used to abort the test process instead of
    // failing it, which is why it is asserted here rather than left to the fuzzing
    // in §9.12: the guard is only real if it fires before the stack does. Each
    // shape is fed well past the ceiling, on a thread the size libtest uses, so a
    // regression shows up as a clean failure and not as a lost test binary.
    for (shape, src) in [
        ("parens", format!("let x = {}1{};", "(".repeat(4000), ")".repeat(4000))),
        ("statements", format!("{}{}", "for (;;) {".repeat(4000), "}".repeat(4000))),
        ("call arguments", format!("let x = {}1{};", "f(".repeat(4000), ")".repeat(4000))),
        ("array elements", format!("let x = {}1{};", "[".repeat(4000), "]".repeat(4000))),
        // Interpolation is the shape that could still defeat the guard by
        // construction: `template_parts` re-enters the parser on the substring
        // inside `${...}`, and a re-entry that restarted the count at zero would
        // reset the ceiling at every level. Nesting the parens *inside* one
        // interpolation is therefore refused only if the outer depth is carried in.
        ("interpolation", {
            let mut s = String::from("let x = `${");
            s.push_str(&"(".repeat(4000));
            s.push('1');
            s.push_str(&")".repeat(4000));
            format!("{s}}}`;")
        }),
    ] {
        let err = parse(&src)
            .err()
            .unwrap_or_else(|| panic!("{shape} nested 4000 deep parsed instead of being refused"));
        assert!(
            err.msg.contains("depth limit"),
            "{shape} failed for an unexpected reason: {err}"
        );
    }
}


