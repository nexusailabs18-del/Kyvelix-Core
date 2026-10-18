//! Phase 9 §9.3 smoke checks for the recursive-descent parser.

use m0::frontend::ast::{Expr, ForHead, Pattern, Stmt};
use m0::frontend::parser::parse;

fn ok(src: &str) -> m0::frontend::ast::Program {
    match parse(src) {
        Ok(p) => p,
        Err(e) => panic!("parse failed for {src:?}: {e}"),
    }
}

fn stmt(src: &str) -> Stmt {
    let mut p = ok(src);
    assert_eq!(p.body.len(), 1, "expected one statement in {src:?}");
    p.body.remove(0)
}

fn expr(src: &str) -> Expr {
    match stmt(&format!("{src};")) {
        Stmt::Expr(e) => e,
        other => panic!("expected an expression statement, got {other:?} for {src:?}"),
    }
}

#[test]
fn every_statement_kind_parses() {
    assert!(matches!(stmt("let x = 1;"), Stmt::Let { .. }));
    assert!(matches!(stmt("if (a) b = 1; else c = 2;"), Stmt::If { .. }));
    assert!(matches!(stmt("while (a) { b = 1; }"), Stmt::While { .. }));
    assert!(matches!(stmt("do { b = 1; } while (a);"), Stmt::DoWhile { .. }));
    assert!(matches!(stmt("for (let i = 0; i < n; i++) { s = 1; }"), Stmt::For { .. }));
    assert!(matches!(stmt("for (;;) { x = 1; }"), Stmt::For { .. }));
    assert!(matches!(stmt("for (const v of a) { x = v; }"), Stmt::ForOf { .. }));
    assert!(matches!(stmt("for (const k in o) { x = k; }"), Stmt::ForIn { .. }));
    assert!(matches!(stmt("function f(a, b) { return a; }"), Stmt::FunctionDecl { .. }));
    assert!(matches!(stmt("class A { }"), Stmt::Class { .. }));
    assert!(matches!(stmt("return;"), Stmt::Return(None, _)));
    assert!(matches!(stmt("break;"), Stmt::Break(None, _)));
    assert!(matches!(stmt("continue loop1;"), Stmt::Continue(Some(_), _)));
    assert!(matches!(stmt("throw e;"), Stmt::Throw(..)));
    assert!(matches!(stmt("try { x = 1; } catch (e) { y = 2; }"), Stmt::Try { .. }));
    assert!(matches!(stmt("switch (x) { case 1: y = 2; break; }"), Stmt::Switch { .. }));
    assert!(matches!(stmt("{ x = 1; }"), Stmt::Block(..)));
    assert!(matches!(stmt(";"), Stmt::Empty(_)));
    assert!(matches!(stmt("import { a } from 'm';"), Stmt::Import { .. }));
    assert!(matches!(stmt("export let x = 1;"), Stmt::Export { .. }));
}

#[test]
fn every_expression_kind_parses() {
    assert!(matches!(expr("a + b * c"), Expr::Binary { .. }));
    assert!(matches!(expr("a = b"), Expr::Assign { .. }));
    assert!(matches!(expr("a ? b : c"), Expr::Conditional { .. }));
    assert!(matches!(expr("a, b"), Expr::Seq(..)));
    assert!(matches!(expr("!a"), Expr::Unary { .. }));
    assert!(matches!(expr("i++"), Expr::Update { .. }));
    assert!(matches!(expr("f(1)"), Expr::Call { .. }));
    assert!(matches!(expr("a.b"), Expr::Member { .. }));
    assert!(matches!(expr("a[0]"), Expr::Index { .. }));
    assert!(matches!(expr("new Date(1)"), Expr::New { .. }));
    assert!(matches!(expr("[1, 2]"), Expr::ArrayLiteral { .. }));
    assert!(matches!(expr("({ a: 1 })"), Expr::ObjectLiteral { .. }));
    assert!(matches!(expr("x => x"), Expr::Arrow { .. }));
    assert!(matches!(expr("(a, b) => a"), Expr::Arrow { .. }));
    // A nameless `function` is only legal in expression position — at the start of
    // a statement JS requires a name — hence the parentheses.
    assert!(matches!(expr("(function (a) { return a; })"), Expr::Function { .. }));
    assert!(matches!(expr("f = function (a) { return a; }"), Expr::Assign { .. }));
    assert!(matches!(expr("`abc`"), Expr::Template { .. }));
    assert!(matches!(expr("typeof a"), Expr::Unary { .. }));
    assert!(matches!(expr("a ?? b"), Expr::Binary { .. }));
    assert!(matches!(expr("a instanceof b"), Expr::Binary { .. }));
    assert!(matches!(expr("a in b"), Expr::Binary { .. }));
}

#[test]
fn malformed_input_is_always_rejected() {
    for src in [
        "let x = ;",
        "if (a) {",
        "while (a",
        "function f(",
        "a.",
        "x =",
        "const c;",
        "let x = 1\nlet y = 2;",
    ] {
        assert!(parse(src).is_err(), "accepted malformed input: {src:?}");
    }
}

#[test]
fn destructuring_patterns_keep_holes_and_rest() {
    match stmt("let [a, , b] = arr;") {
        Stmt::Let { pattern: Pattern::Array { elements, .. }, .. } => {
            assert_eq!(elements.len(), 3);
            assert!(elements[1].is_none());
        }
        other => panic!("{other:?}"),
    }
    match stmt("let { a, b: c, d = 1 } = o;") {
        Stmt::Let { pattern: Pattern::Object { props, .. }, .. } => {
            assert_eq!(props.len(), 3);
            assert!(props[0].shorthand);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_for_header_forms_are_distinguished() {
    match stmt("for (a in b) { x = 1; }") {
        Stmt::ForIn { .. } => {}
        other => panic!("`for (a in b)` parsed as {other:?}"),
    }
    match stmt("for ((a in b); ;) { x = 1; }") {
        Stmt::For { head: ForHead::Classical { .. }, .. } => {}
        other => panic!("{other:?}"),
    }
}
