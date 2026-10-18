//! Phase 9 §9.4/§9.6/§9.7/§9.14 — standalone lowering tests.
//!
//! The tests `stubs.md` ("Stub 1 — `frontend/lower.rs`") asks for: the five
//! `tests/js_kernels` fixtures lowered to IR with their IVs, coefficients and
//! offsets asserted field-for-field; one test per named §9.14 refusal; and the two
//! shape rules the module docs promise — sibling loops are *refused*, not merged,
//! and a `for-of` is rewritten as the indexed walk it denotes.
//!
//! Environment: `n = ld = 8`, the corpora's square-matrix convention, so a
//! fixture's bound `n` and stride `ld` resolve. `A.length` is supplied only where
//! a test is about `for-of`.

use m0::frontend::lower::{lower_program, lower_source, Env};
use m0::frontend::parse;
use m0::ir::{AccessKind, Loop, OffsetValue};

/// `n = ld = 8`: the convention every hand-written corpus uses.
fn env() -> Env {
    Env::of(&[("n", 8), ("ld", 8)])
}

fn ivs(l: &Loop) -> Vec<(String, i64, i64, i64)> {
    l.ivs.iter().map(|iv| (iv.name.clone(), iv.lower, iv.upper, iv.step)).collect()
}

/// `(read?, base, index, offset)` per access, in the order the nest holds them.
fn accesses(l: &Loop) -> Vec<(bool, String, Vec<i64>, Option<i64>)> {
    l.body
        .iter()
        .map(|a| {
            let off = match a.offset {
                OffsetValue::Const(c) => Some(c),
                _ => None,
            };
            (matches!(a.kind, AccessKind::Read), a.base.clone(), a.index.clone(), off)
        })
        .collect()
}

fn reasons(k: &m0::frontend::Kernels) -> Vec<String> {
    k.skipped.iter().map(|s| format!("{}", s.reason)).collect()
}

// ---- the fixtures ---------------------------------------------------------

#[test]
fn syr2k_lowers_to_one_depth_three_nest() {
    let k = lower_source(include_str!("js_kernels/syr2k.js"), &env()).expect("parses");
    assert_eq!(k.skipped.len(), 0, "nothing in syr2k is outside the subset: {:?}", reasons(&k));
    assert_eq!(k.loops.len(), 1, "the j/i/k chain is one nest");
    let l = &k.loops[0];
    assert_eq!(l.depth, 3);
    assert_eq!(
        ivs(l),
        vec![
            ("j".to_string(), 0, 8, 1),
            ("i".to_string(), 0, 8, 1),
            ("k".to_string(), 0, 8, 1),
        ]
    );
    // `A[i][j]`, `A[j][i]`, `B[i][j]`, `B[j][i]` read; `C[i*ld + j]` read and
    // written (the compound assignment). Row-major flattening: row * width + col
    // with width = the innermost IV's exclusive bound.
    assert_eq!(
        accesses(l),
        vec![
            (true, "A".to_string(), vec![0, 1, 8], Some(0)),
            (true, "A".to_string(), vec![1, 0, 8], Some(0)),
            (true, "B".to_string(), vec![0, 1, 8], Some(0)),
            (true, "B".to_string(), vec![1, 0, 8], Some(0)),
            (true, "C".to_string(), vec![1, 8, 0], Some(0)),
            (false, "C".to_string(), vec![1, 8, 0], Some(0)),
        ]
    );
}

#[test]
fn stencil2d_lowers_to_one_depth_two_nest_with_a_five_point_offsets() {
    let k = lower_source(include_str!("js_kernels/stencil2d.js"), &env()).expect("parses");
    assert_eq!(k.skipped.len(), 0, "{:?}", reasons(&k));
    assert_eq!(k.loops.len(), 1);
    let l = &k.loops[0];
    assert_eq!(l.depth, 2);
    // The stencil's bounds are 1 and n-1: the halo is part of the source.
    assert_eq!(
        ivs(l),
        vec![("i".to_string(), 1, 8, 1), ("j".to_string(), 1, 8, 1)]
    );
    let a = accesses(l);
    // Four neighbours plus the centre write: the offsets are the stencil itself.
    let offsets: Vec<Option<i64>> = a.iter().map(|x| x.3).collect();
    for want in [-8, -1, 0, 1, 8] {
        assert!(offsets.contains(&Some(want)), "missing stencil offset {want}: {a:?}");
    }
    assert_eq!(a.iter().filter(|x| !x.0).count(), 1, "one write (u's centre): {a:?}");
    assert!(a.iter().all(|x| x.2 == vec![8, 1]), "flat row-major index: {a:?}");
}

#[test]
fn mix_lowers_the_counted_loop_and_names_every_refusal() {
    // §9.14's rule is *per construct*: mix.js holds one analyzable loop and five
    // constructs that are outside the subset, and lowering has to report both.
    let k = lower_source(include_str!("js_kernels/mix.js"), &env()).expect("parses");
    assert_eq!(k.loops.len(), 1, "{:?}", reasons(&k));
    let l = &k.loops[0];
    // `i += 2`: the step is part of the iteration space, not a rewrite.
    assert_eq!(ivs(l), vec![("i".to_string(), 0, 8, 2)]);
    assert_eq!(accesses(l).len(), 2, "one read, one write: {:?}", accesses(l));
    let r = reasons(&k);
    assert_eq!(r.len(), 5, "{r:?}");
    assert_eq!(r.iter().filter(|s| s.contains("destructuring")).count(), 2, "{r:?}");
    for construct in ["for-in", "while", "do-while"] {
        assert!(
            r.iter().any(|s| s.contains(construct)),
            "`{construct}` must be named, not summarised: {r:?}"
        );
    }
}

#[test]
fn triangle_is_refused_because_its_bound_is_another_iv() {
    // `j <= i` is a nonrectangular region: `IV.upper` is an `i64` and `Guard`
    // holds only IV-vs-constant comparisons, so the nest cannot be stated.
    let k = lower_source(include_str!("js_kernels/triangle.js"), &env()).expect("parses");
    assert_eq!(k.loops.len(), 0, "a nest that cannot be stated is not emitted");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("depends on another IV"), "{r:?}");
    assert!(r[0].contains("§9.14"), "the boundary is named: {r:?}");
}

#[test]
fn jaccard_for_of_needs_its_length_in_the_environment() {
    // `setA.length` is a parameter, not a literal: without it the trip count is
    // not a fact the IR can state, and the refusal says exactly that.
    let src = include_str!("js_kernels/jaccard.js");
    let k = lower_source(src, &env()).expect("parses");
    assert_eq!(k.loops.len(), 0);
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("setA.length"), "{r:?}");
    assert!(r[0].contains("§9.7"), "the rule being applied is named: {r:?}");
}

#[test]
fn jaccard_lowers_the_for_of_and_then_refuses_the_call_in_its_guard() {
    // With the length supplied, the `for-of` is no longer the refusal — the
    // method call in the guard is, which is §9.14's honest answer for this body.
    // The point of the test is that the *first* refusal moved: the `for-of`
    // itself lowered.
    let src = include_str!("js_kernels/jaccard.js");
    let env = Env::of(&[("n", 8), ("ld", 8), ("setA.length", 8), ("setB.length", 8)]);
    let k = lower_source(src, &env).expect("parses");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(!r[0].contains("for-of"), "the for-of lowered: {r:?}");
    assert!(r[0].contains("call"), "the call is what refuses now: {r:?}");
}

// ---- §9.7: `for-of` is a counted loop ---------------------------------------

#[test]
fn for_of_over_a_named_array_is_an_indexed_walk() {
    // The binding has no representable value, so it has a *location*: a read of
    // `v` is a read of `A` at the synthesized index IV.
    let src = "
        function f(A, B) {
            for (const v of A) {
                B[0] = v;
            }
        }";
    let env = Env::of(&[("A.length", 4)]);
    let k = lower_source(src, &env).expect("parses");
    assert_eq!(k.skipped.len(), 0, "{:?}", reasons(&k));
    assert_eq!(k.loops.len(), 1);
    let l = &k.loops[0];
    assert_eq!(l.depth, 1);
    assert_eq!(ivs(l), vec![("v#of".to_string(), 0, 4, 1)], "the trip count comes from `A.length`");
    assert_eq!(
        accesses(l),
        vec![
            (true, "A".to_string(), vec![1], Some(0)),  // reading `v`
            (false, "B".to_string(), vec![0], Some(0)), // writing `B[0]`
        ]
    );
}

#[test]
fn nested_for_of_is_one_depth_two_space() {
    let src = "
        function f(A) {
            for (const p of A) {
                for (const q of A) {
                    A[0] = q;
                }
            }
        }";
    let env = Env::of(&[("A.length", 4)]);
    let k = lower_source(src, &env).expect("parses");
    assert_eq!(reasons(&k), Vec::<String>::new());
    assert_eq!(k.loops.len(), 1);
    let l = &k.loops[0];
    assert_eq!(l.depth, 2);
    assert_eq!(ivs(l), vec![("p#of".to_string(), 0, 4, 1), ("q#of".to_string(), 0, 4, 1)]);
    // Both accesses are on `A`, so the nest orders them by kind (reads before
    // writes), not by the order they were written in.
    assert_eq!(
        accesses(l),
        vec![
            (true, "A".to_string(), vec![0, 1], Some(0)), // `q` is the inner index
            (false, "A".to_string(), vec![0, 0], Some(0)),
        ]
    );
}

#[test]
fn a_zero_length_for_of_contributes_nothing() {
    // Zero trips: the body never executes. Not a refusal — diagnosing code that
    // cannot run would be a false diagnosis.
    let src = "function f(A) { for (const v of A) { A[0] = 0; } }";
    let env = Env::of(&[("A.length", 0)]);
    let k = lower_source(src, &env).expect("parses");
    assert_eq!(k.loops.len(), 0);
    assert_eq!(reasons(&k), Vec::<String>::new());
}

#[test]
fn a_for_of_over_a_call_result_is_refused_by_name() {
    let src = "function f(A) { for (const v of A.slice(0, 4)) { A[v] = 0; } }";
    let k = lower_source(src, &env()).expect("parses");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("not a plain array name"), "{r:?}");
}

// ---- the shape rules the module docs promise ---------------------------------

#[test]
fn sibling_loops_are_refused_rather_than_merged() {
    // `for j` and `for k` sit in the same body. Charging both to one `Loop` would
    // emit depth 2 with `k` playing `j`'s role — a dependence between iterates
    // that do not coexist. Refused, and the refusal names the rule.
    let src = "
        function f(A, B) {
            for (let i = 0; i < 8; i++) {
                for (let j = 0; j < 8; j++) { A[i] = 0; }
                for (let k = 0; k < 8; k++) { B[k] = 0; }
            }
        }";
    let k = lower_source(src, &env()).expect("parses");
    assert_eq!(k.loops.len(), 0, "a poisoned nest is never half-emitted");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("sibling"), "{r:?}");
    assert!(r[0].contains("§9.14"), "{r:?}");
}

#[test]
fn partial_analysis_keeps_the_loop_and_names_the_rest() {
    let src = "
        function f(A) {
            for (let i = 0; i < 8; i++) { A[i] = 0; }
            for (const k in A) { }
        }";
    let k = lower_source(src, &env()).expect("parses");
    assert_eq!(k.loops.len(), 1, "the analyzable part survives");
    assert_eq!(k.skipped.len(), 1, "and the rest is reported");
    assert!(reasons(&k)[0].contains("for-in"));
}

#[test]
fn a_guard_is_attached_to_the_accesses_below_it() {
    let src = "
        function f(A) {
            for (let i = 0; i < 8; i++) {
                if (i < 4) { A[i] = 0; }
            }
        }";
    let k = lower_source(src, &env()).expect("parses");
    assert_eq!(reasons(&k), Vec::<String>::new());
    let l = &k.loops[0];
    let guarded: Vec<_> = l.body.iter().filter(|a| a.guard.is_some()).collect();
    assert!(!guarded.is_empty(), "the guard has to reach the access: {:?}", l.body);
    for a in guarded {
        let g = a.guard.as_ref().expect("filtered above");
        assert_eq!(g.constraints.len(), 1, "`i < 4` is one constraint");
    }
}

// ---- §9.14 refusals, each asserted by the name it carries ---------------------

#[test]
fn a_nonlinear_index_is_refused_by_name() {
    let src = "
        function f(A) {
            for (let i = 0; i < 8; i++) {
                for (let j = 0; j < 8; j++) { A[i * j] = 0; }
            }
        }";
    let k = lower_source(src, &env()).expect("parses");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("product of two IV-bearing terms"), "{r:?}");
    assert!(r[0].contains("§9.14"), "{r:?}");
}

#[test]
fn an_index_reading_a_foreign_name_is_refused() {
    let src = "function f(A) { for (let i = 0; i < 8; i++) { A[m] = 0; } }";
    let k = lower_source(src, &env()).expect("parses");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("neither an IV"), "{r:?}");
}

#[test]
fn a_condition_the_ir_cannot_name_refuses_the_nest() {
    let src = "function f(A) { for (let i = 0; i < 8; i++) { if (g(i)) { A[i] = 0; } } }";
    let k = lower_source(src, &env()).expect("parses");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("not a comparison the guard IR can state"), "{r:?}");
}

#[test]
fn an_access_outside_any_loop_is_refused() {
    let src = "function f(A) { A[0] = 0; }";
    let k = lower_source(src, &env()).expect("parses");
    let r = reasons(&k);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].contains("outside any loop"), "{r:?}");
}

#[test]
fn destructuring_heads_are_refused_by_name() {
    for src in [
        "function f(A) { let [a, b] = A; }",
        "function f(A) { for (const [a, b] of A) { } }",
    ] {
        let k = lower_source(src, &env()).expect("parses");
        let r = reasons(&k);
        assert_eq!(r.len(), 1, "{src}: {r:?}");
        assert!(r[0].contains("destructuring"), "{src}: {r:?}");
    }
}

#[test]
fn lower_strict_refuses_the_first_construct_and_says_which() {
    let src = "
        function f(A) {
            for (let i = 0; i < 8; i++) { A[i] = 0; }
            while (true) { }
        }";
    let e = m0::frontend::lower_strict(&parse(src).expect("parses"), &env())
        .expect_err("a `while` is outside the subset");
    assert_eq!(e, m0::frontend::LowerError::OutOfScope("while"));
    // The lenient entry point reports the same fact without discarding the loop.
    let k = lower_source(src, &env()).expect("parses");
    assert_eq!(k.loops.len(), 1);
    assert_eq!(k.skipped.len(), 1);
}

#[test]
fn lowering_is_deterministic() {
    for name in ["syr2k", "stencil2d", "mix", "triangle", "jaccard"] {
        let src = match name {
            "syr2k" => include_str!("js_kernels/syr2k.js"),
            "stencil2d" => include_str!("js_kernels/stencil2d.js"),
            "mix" => include_str!("js_kernels/mix.js"),
            "triangle" => include_str!("js_kernels/triangle.js"),
            _ => include_str!("js_kernels/jaccard.js"),
        };
        let a = lower_program(&parse(src).expect("parses"), &env());
        let b = lower_program(&parse(src).expect("parses"), &env());
        assert_eq!(a, b, "{name} lowered twice differs");
    }
}




