//! Phase 9 §9.3 — the recursive-descent parser.
//!
//! One function per grammar rule, as the spec asks. Precedence is handled by
//! precedence climbing rather than by a rule per level, so §9.3's `parse_expr`
//! stays one function and the level table is data ([`prec_of`]) rather than call
//! nesting.
//!
//! ## Why this parser refuses instead of recovering
//!
//! A production frontend wants error recovery: a syntax error in one function
//! should not hide a second one. This one does not recover, and the reason is the
//! contract in [`crate::frontend`] — the frontend may only hand the analyzer IR it
//! is certain about. Recovery is how a parser invents structure it did not read:
//! `for (i = 0; i < n; i++` repaired into a loop is a loop the source does not
//! contain, and a dependence result about it is a claim about nothing. So anything
//! malformed is a hard [`ParseError`].
//!
//! ## Automatic semicolon insertion
//!
//! Real JS relies on ASI. Implementing it is out of scope and, more importantly,
//! unsafe for this frontend: ASI is line-terminator driven and context-sensitive,
//! so a partial implementation silently changes what a program means. The parser
//! requires explicit `;` and rejects the input when one is missing rather than
//! guessing. The lowering never sees an ambiguous statement.

use crate::frontend::ast::{
    ArrowBody, AssignOp, BinOp, CatchClause, Expr, ForHead, ObjectProp, Pattern, Program,
    Position, Stmt, SwitchCase, TemplatePart, UnOp, UpdateOp,
};
use crate::frontend::lexer::{Keyword, Lexer, Punct, Token, TokenKind};

/// A syntax error: what was expected, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub msg: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for ParseError {}

type P<T> = Result<T, ParseError>;

struct Prec {
    level: u8,
    /// `true` for right-associative operators (`**`, assignment).
    right: bool,
}

/// The precedence ladder: `None` for a token that is not an infix operator.
///
/// The numbering follows ECMAScript's own table rather than being compacted,
/// because a mis-associated tree is far easier to spot against the numbers the
/// spec uses.
///
/// `no_in` implements the spec's `[~In]` grammar parameter. ECMAScript parses a
/// classical `for` initializer *without* allowing `in`, which is what makes
/// `for (a in b)` unambiguously a for-in loop: if the initializer had consumed
/// `in`, the header could not be an iteration head. An unknown operator here
/// would silently turn one loop form into the other.
fn prec_of(kind: &TokenKind, no_in: bool) -> Option<Prec> {
    if let TokenKind::Keyword(Keyword::Instanceof) | TokenKind::Keyword(Keyword::In) = kind {
        if matches!(kind, TokenKind::Keyword(Keyword::In)) && no_in {
            return None;
        }
        return Some(Prec { level: 7, right: false });
    }
    let p = match kind {
        TokenKind::Punct(p) => *p,
        _ => return None,
    };
    Some(match p {
        Punct::PipePipe | Punct::Nullish => Prec { level: 2, right: false },
        Punct::AmpAmp => Prec { level: 3, right: false },
        Punct::Pipe => Prec { level: 4, right: false },
        Punct::Caret => Prec { level: 5, right: false },
        Punct::Amp => Prec { level: 6, right: false },
        Punct::EqEq | Punct::EqEqEq | Punct::Ne | Punct::NeNe => Prec { level: 7, right: false },
        Punct::Lt | Punct::Le | Punct::Gt | Punct::Ge => Prec { level: 8, right: false },
        Punct::ShiftLeft | Punct::ShiftRight | Punct::UShiftRight => Prec { level: 9, right: false },
        Punct::Plus | Punct::Minus => Prec { level: 10, right: false },
        Punct::Star | Punct::Slash | Punct::Percent => Prec { level: 11, right: false },
        Punct::StarStar => Prec { level: 12, right: true },
        _ => return None,
    })
}

/// The cursor over the token stream.
struct Parser {
    toks: Vec<Token>,
    i: usize,
    /// The `[~In]` parameter, set only while parsing a classical-`for` position.
    no_in: bool,
    /// Current recursion depth, guarded by [`MAX_DEPTH`]. See [`Parser::enter`].
    depth: usize,
}

/// The ceiling on recursive-descent depth.
///
/// Recursive descent uses the machine stack, and a stack overflow is not an error
/// a caller can handle: the process aborts. A frontend that aborts on
/// `(((((((...` reports as an analyzer crash rather than as what it is — bad
/// input — and §9.12's fuzzing against Node would find it on the first pass. So
/// the parser bounds its own depth and returns a [`ParseError`] instead.
///
/// The ceiling is paired with [`PARSE_STACK`]: a limit that is only safe on a big
/// stack is not a limit, so [`parse`] provides that stack rather than trusting the
/// caller to have one. Both numbers come from `examples/p9probe.rs`, which parses
/// each nesting shape with the guard switched off and reads off where the process
/// dies. In a debug build the worst shape is nested statements — 28 KiB a level,
/// aborting near 600 on [`PARSE_STACK`]'s 16 MiB — while nested parens, arrays,
/// object values, call arguments and interpolation expressions each die near 750.
/// 256 is therefore a factor of two below the first shape to fail, and the margin
/// is what keeps the guard firing on stack rather than the guard being overtaken by
/// a shape measured on a different frame chain.
///
/// 256 levels is also far past any handwritten source: the fixtures here nest 3,
/// the deepest generated kernel 5, and the stress test in `phase9_kernels` nests 60
/// `for` loops and 200 parentheses — all of which must parse, because a refusal at
/// a depth real input reaches is a regression, not a safety improvement.
const MAX_DEPTH: usize = 256;

/// The machine stack [`parse`] runs the parser on, measured rather than guessed.
///
/// `examples/p9probe.rs` parses each nesting shape with the depth guard switched
/// off and reads off the depth at which the process aborts. In a debug build the
/// cost is linear in the nesting depth at about 28 KiB a level for nested
/// statements — the frame chain is the whole rule set with nothing inlined, and a
/// `Vec` is pushed at each step — and that figure holds whether the stack is 2 MiB
/// (aborting near 75 levels) or 16 MiB (near 600), which is the point: bytes, not
/// frames, decide it. So a permitted parse wants at most `MAX_DEPTH × 28 KiB` ≈
/// 7 MiB, and 16 MiB leaves a factor of two over the worst measured shape.
///
/// The parser cannot treat that as a caller's problem. The main thread's stack is
/// the platform default — 8 MiB on Linux, already inside the margin — and libtest
/// gives every `#[test]` thread 2 MiB, at which a 60-deep nest aborts long before
/// the guard can fire. Providing the stack here is what makes the ceiling mean the
/// same thing in the CLI, in a test, and under a fuzzer.
const PARSE_STACK: usize = 16 * 1024 * 1024;

impl Parser {
    fn new(src: &str) -> Parser {
        Parser { toks: Lexer::tokenize(src), i: 0, no_in: false, depth: 0 }
    }

    /// Enter one level of recursion, refusing rather than risking a stack
    /// overflow. The parser's expression chain
    /// (`expr → assign → conditional → binary → unary → postfix → call_member →
    /// primary`) is eight frames wide per nesting level, so unbounded recursion
    /// on `(((...)))` aborts the process — which a caller cannot distinguish from
    /// an analyzer crash, and which §9.12's fuzz run would hit on the first pass.
    /// The paired [`Parser::leave`] keeps the count exact on every exit path.
    fn enter(&mut self) -> P<()> {
        if self.depth >= MAX_DEPTH {
            return self.err("expression or statement nesting exceeds the parser's depth limit");
        }
        self.depth += 1;
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn cur(&self) -> &Token {
        // `tokenize` always appends an Eof, so the stream is never empty; the
        // index clamps rather than bounds-checking at every call site.
        self.toks.get(self.i).unwrap_or_else(|| self.toks.last().unwrap())
    }

    fn kind(&self) -> &TokenKind {
        &self.cur().kind
    }

    fn bump(&mut self) -> Token {
        let t = self.cur().clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn eat(&mut self, p: Punct) -> bool {
        if self.cur().is_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: Keyword) -> bool {
        if self.cur().is_keyword(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn pos(&self) -> Position {
        let t = self.cur();
        Position::new(t.line, t.col)
    }

    fn err<T>(&self, msg: impl Into<String>) -> P<T> {
        let t = self.cur();
        Err(ParseError { msg: msg.into(), line: t.line, col: t.col })
    }

    /// Consume `p` or fail naming it.
    fn expect(&mut self, p: Punct) -> P<()> {
        if self.eat(p) {
            return Ok(());
        }
        self.err(format!("expected `{}`, found {}", p.as_str(), self.cur().describe()))
    }

    fn expect_kw(&mut self, k: Keyword) -> P<()> {
        if self.eat_kw(k) {
            return Ok(());
        }
        self.err(format!("expected `{}`, found {}", k.as_str(), self.cur().describe()))
    }

    fn ident(&mut self) -> P<String> {
        let t = self.cur().clone();
        if let TokenKind::Ident = t.kind {
            self.bump();
            Ok(t.text)
        } else {
            Err(ParseError {
                msg: format!("expected an identifier, found {}", t.describe()),
                line: t.line,
                col: t.col,
            })
        }
    }

    /// A lexer `Bad` token is always a parse error, never skipped: a skipped bad
    /// token lets the parser read a program the source does not contain.
    fn check_bad(&self) -> P<()> {
        if let TokenKind::Bad(m) = self.kind() {
            let t = self.cur();
            return Err(ParseError { msg: m.clone(), line: t.line, col: t.col });
        }
        Ok(())
    }
}

impl Parser {
    fn parse_program(&mut self) -> P<Program> {
        let mut body = Vec::new();
        while !matches!(self.kind(), TokenKind::Eof) {
            self.check_bad()?;
            body.push(self.parse_stmt()?);
        }
        Ok(Program::new(body))
    }

    /// §9.3's `parse_block`.
    fn parse_block(&mut self) -> P<Vec<Stmt>> {
        self.expect(Punct::LBrace)?;
        // A block always starts a fresh statement context, so the `[~In]` flag a
        // `for` header set cannot leak into the body: `for (;;) { if (k in o) {} }`
        // must parse the `in` test. Restored on exit because a block can be nested
        // inside a `for` clause (`for (({x: 1}.x); ;)`).
        let saved = std::mem::replace(&mut self.no_in, false);
        let mut stmts = Vec::new();
        while !self.eat(Punct::RBrace) {
            if matches!(self.kind(), TokenKind::Eof) {
                return self.err("unterminated block: `{` has no matching `}`");
            }
            self.check_bad()?;
            stmts.push(self.parse_stmt()?);
        }
        self.no_in = saved;
        Ok(stmts)
    }

    /// Statement nesting guard. `parse_stmt → parse_block → parse_stmt` and
    /// `parse_stmt → parse_if → parse_sub_stmt → parse_stmt` are the two ways the
    /// statement grammar recurses, so one counter here bounds block nesting.
    fn parse_stmt(&mut self) -> P<Stmt> {
        self.enter()?;
        let r = self.parse_stmt_inner();
        self.leave();
        r
    }

    fn parse_stmt_inner(&mut self) -> P<Stmt> {
        let pos = self.pos();
        match self.kind().clone() {
            TokenKind::Keyword(k) => match k {
                Keyword::Let | Keyword::Const | Keyword::Var => self.parse_decl(),
                Keyword::If => self.parse_if(),
                Keyword::While => self.parse_while(),
                Keyword::Do => self.parse_do_while(),
                Keyword::For => self.parse_for(),
                Keyword::Function => self.parse_function_decl(),
                Keyword::Class => self.parse_class(),
                Keyword::Return => self.parse_return(),
                Keyword::Break => self.parse_break_continue(true),
                Keyword::Continue => self.parse_break_continue(false),
                Keyword::Throw => self.parse_throw(),
                Keyword::Try => self.parse_try(),
                Keyword::Switch => self.parse_switch(),
                Keyword::Import => self.parse_import(),
                Keyword::Export => self.parse_export(),
                _ => self.parse_expr_stmt(pos),
            },
            TokenKind::Punct(Punct::LBrace) => Ok(Stmt::Block(self.parse_block()?, pos)),
            TokenKind::Punct(Punct::Semi) => {
                self.bump();
                Ok(Stmt::Empty(pos))
            }
            _ => self.parse_expr_stmt(pos),
        }
    }

    /// A statement whose body is a block or a single statement: `if`, `for` and
    /// friends all accept `for (...) x = 1;` as well as a block.
    fn parse_sub_stmt(&mut self) -> P<Stmt> {
        if self.cur().is_punct(Punct::LBrace) {
            let pos = self.pos();
            Ok(Stmt::Block(self.parse_block()?, pos))
        } else {
            self.parse_stmt()
        }
    }

    fn parse_expr_stmt(&mut self, pos: Position) -> P<Stmt> {
        let e = self.parse_expr()?;
        self.semicolon()?;
        let _ = pos;
        Ok(Stmt::Expr(e))
    }

    /// `let`/`const`/`var` with a pattern on the left (§9.6's destructuring
    /// entry point), which is why this takes a [`Pattern`] rather than a name.
    fn parse_decl(&mut self) -> P<Stmt> {
        let pos = self.pos();
        let kw = match self.kind() {
            TokenKind::Keyword(k) => *k,
            _ => return self.err("expected `let`, `const` or `var`"),
        };
        self.bump();
        let mut decls = Vec::new();
        loop {
            decls.push(self.parse_one_declarator(kw, pos)?);
            if !self.eat(Punct::Comma) {
                break;
            }
        }
        self.semicolon()?;
        // `let a = 1, b = 2;` is one statement in JS and several in the AST. A
        // `Block` of declarations preserves both the order of initialization and
        // the single scope, because a block declares nothing of its own; the
        // alternative — refusing it — would reject a shape that is common in
        // numerical code (`let t1 = 0, t2 = 0;`).
        if decls.len() == 1 {
            return Ok(decls.pop().unwrap());
        }
        Ok(Stmt::Block(decls, pos))
    }

    /// One `pattern [= init]` of a declaration. Split out so a comma-separated
    /// list becomes one statement per declarator.
    fn parse_one_declarator(&mut self, kw: Keyword, pos: Position) -> P<Stmt> {
        // The head only: an `=` after a *declaration* pattern is the initializer
        // (`let [a, b] = arr`), not a default (`let [a = 1] = arr` has its default
        // inside the brackets). Reading the default here would turn the first form
        // into a binding whose initializer is missing.
        let pattern = self.parse_pattern_head()?;
        // `parse_assign`, not `parse_expr`: inside a declaration list the comma
        // separates declarators (`let a = 1, b = 2`), it is *not* the comma
        // operator. Reading an expression here would make `let t1 = 0, t2 = 0`
        // one declaration whose initializer is a sequence, silently turning a
        // declaration of `t2` into an assignment to it.
        let init = if self.eat(Punct::Eq) { Some(self.parse_assign()?) } else { None };
        match kw {
            // `const` must have an initializer; JS makes it a SyntaxError, and a
            // `None` here would make the lowering invent a value.
            Keyword::Const => match init {
                Some(e) => Ok(Stmt::Const { pattern, init: e, pos }),
                None => self.err("`const` requires an initializer"),
            },
            Keyword::Let => Ok(Stmt::Let { pattern, init, pos }),
            _ => Ok(Stmt::Var { pattern, init, pos }),
        }
    }

    fn parse_if(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::If)?;
        self.expect(Punct::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(Punct::RParen)?;
        let then = Box::new(self.parse_sub_stmt()?);
        let els = if self.eat_kw(Keyword::Else) { Some(Box::new(self.parse_sub_stmt()?)) } else { None };
        Ok(Stmt::If { cond, then, els, pos })
    }

    fn parse_while(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::While)?;
        self.expect(Punct::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(Punct::RParen)?;
        let body = Box::new(self.parse_sub_stmt()?);
        Ok(Stmt::While { cond, body, pos })
    }

    fn parse_do_while(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Do)?;
        let body = Box::new(self.parse_sub_stmt()?);
        self.expect_kw(Keyword::While)?;
        self.expect(Punct::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(Punct::RParen)?;
        self.semicolon()?;
        Ok(Stmt::DoWhile { body, cond, pos })
    }

    /// §9.3's `parse_for`, covering all three header forms. The `of`/`in`
    /// distinction is decided by lookahead: `for (a in b)` begins with what is
    /// also a valid *expression* (`a in b`), so the first token cannot settle it.
    fn parse_for(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::For)?;
        self.expect(Punct::LParen)?;

        // `for (const x of ...)` / `for (let k in ...)`: a declaration keyword can
        // only start an iteration head or a classical initializer.
        if matches!(self.kind(), TokenKind::Keyword(Keyword::Const) | TokenKind::Keyword(Keyword::Let) | TokenKind::Keyword(Keyword::Var))
        {
            let dpos = self.pos();
            let kw = match self.kind() {
                TokenKind::Keyword(k) => *k,
                _ => unreachable!(),
            };
            self.bump();
            // The head is parsed *without* the top-level default, because `=` is
            // exactly what distinguishes `for (let i = 0; ...)` from
            // `for (const {a = 1} of list)`. Defaults nested inside a pattern are
            // still parsed, since they cannot be confused with the head.
            let head = self.parse_pattern_head()?;
            if self.cur().is_keyword(Keyword::Of) {
                let pattern = self.finish_pattern(head)?;
                self.bump();
                let iter = self.parse_fresh_expr()?;
                self.expect(Punct::RParen)?;
                let body = Box::new(self.parse_sub_stmt()?);
                return Ok(Stmt::ForOf { pattern, iter, body, is_async: false, pos });
            }
            if self.cur().is_keyword(Keyword::In) {
                let pattern = self.finish_pattern(head)?;
                self.bump();
                let obj = self.parse_fresh_expr()?;
                self.expect(Punct::RParen)?;
                let body = Box::new(self.parse_sub_stmt()?);
                return Ok(Stmt::ForIn { pattern, obj, body, pos });
            }
            // `for (let i = 0; i < n; i++)` — the form the analyzers see most.
            // The initializer is `[~In]`: if `in` could be consumed here, the
            // for-in head tested for above would never be reachable. A comma
            // separates declarators (`for (let i = 0, j = n; ;)`), so the list is
            // read one declarator at a time, as `parse_decl` reads one. Each
            // initializer stops at that comma by way of `parse_assign`, not
            // `parse_expr`, which would read the comma as the comma operator and
            // turn a declaration of `j` into an assignment to it.
            self.expect(Punct::Eq)?;
            let mut decls = vec![self.for_declarator(kw, dpos, head)?];
            while self.eat(Punct::Comma) {
                let next = self.parse_pattern_head()?;
                if self.eat(Punct::Eq) {
                    decls.push(self.for_declarator(kw, dpos, next)?);
                } else {
                    // `for (let i = 0, j; ;)` — a declarator with no initializer is
                    // legal but has no value, and a `const` one would have to
                    // invent it.
                    decls.push(match kw {
                        Keyword::Const => return self.err("`const` requires an initializer"),
                        Keyword::Let => Stmt::Let { pattern: next, init: None, pos: dpos },
                        _ => Stmt::Var { pattern: next, init: None, pos: dpos },
                    });
                }
            }
            self.expect(Punct::Semi)?;
            let decl = Box::new(if decls.len() == 1 {
                decls.pop().unwrap()
            } else {
                Stmt::Block(decls, dpos)
            });
            let cond = if self.cur().is_punct(Punct::Semi) { None } else { Some(self.parse_expr()?) };
            self.expect(Punct::Semi)?;
            let update = if self.cur().is_punct(Punct::RParen) { None } else { Some(self.parse_expr()?) };
            self.expect(Punct::RParen)?;
            let body = Box::new(self.parse_sub_stmt()?);
            return Ok(Stmt::For { head: ForHead::Classical { init: Some(decl), cond, update }, body, pos });
        }

        // `for (;;)` with no initializer.
        if self.cur().is_punct(Punct::Semi) {
            self.bump();
            let cond = if self.cur().is_punct(Punct::Semi) { None } else { Some(self.parse_expr()?) };
            self.expect(Punct::Semi)?;
            let update = if self.cur().is_punct(Punct::RParen) { None } else { Some(self.parse_expr()?) };
            self.expect(Punct::RParen)?;
            let body = Box::new(self.parse_sub_stmt()?);
            return Ok(Stmt::For { head: ForHead::Classical { init: None, cond, update }, body, pos });
        }

        // The initializer is parsed with `in` forbidden, which is what settles the
        // `for (a in b)` ambiguity without a guess: if `in` could not be consumed
        // here, the `in` still sitting in front of us means an iteration head.
        // The two later clauses allow `in`, as the spec's `[+In]` marks say.
        let first = self.parse_expr_no_in()?;
        if self.cur().is_keyword(Keyword::Of) {
            self.bump();
            let iter = self.parse_expr()?;
            self.expect(Punct::RParen)?;
            let body = Box::new(self.parse_sub_stmt()?);
            return Ok(Stmt::ForOf { pattern: binding_from_expr(&first)?, iter, body, is_async: false, pos });
        }
        if self.cur().is_keyword(Keyword::In) {
            self.bump();
            let obj = self.parse_expr()?;
            self.expect(Punct::RParen)?;
            let body = Box::new(self.parse_sub_stmt()?);
            return Ok(Stmt::ForIn { pattern: binding_from_expr(&first)?, obj, body, pos });
        }

        let init = Box::new(Stmt::Expr(first));
        self.expect(Punct::Semi)?;
        let cond = if self.cur().is_punct(Punct::Semi) { None } else { Some(self.parse_expr()?) };
        self.expect(Punct::Semi)?;
        let update = if self.cur().is_punct(Punct::RParen) { None } else { Some(self.parse_expr()?) };
        self.expect(Punct::RParen)?;
        let body = Box::new(self.parse_sub_stmt()?);
        Ok(Stmt::For { head: ForHead::Classical { init: Some(init), cond, update }, body, pos })
    }

    fn parse_function_decl(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Function)?;
        let name = self.ident()?;
        let params = self.parse_params()?;
        let body = self.parse_block()?;
        Ok(Stmt::FunctionDecl { name, params, body, is_async: false, pos })
    }

    /// §9.3 lists `parse_class`. Classes parse — they are in the language — but
    /// nothing lowers them, because `this` escapes to a runtime object model the
    /// shape analysis (§9.8) does not have. Parsing without lowering is what keeps
    /// the out-of-scope report precise ("`class`") instead of a generic syntax
    /// error that hides what the source actually said.
    fn parse_class(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Class)?;
        let name = if matches!(self.kind(), TokenKind::Ident) { Some(self.ident()?) } else { None };
        if self.eat_kw(Keyword::Extends) {
            self.parse_expr()?;
        }
        self.expect(Punct::LBrace)?;
        let mut members = Vec::new();
        while !self.eat(Punct::RBrace) {
            if matches!(self.kind(), TokenKind::Eof) {
                return self.err("unterminated class body");
            }
            self.check_bad()?;
            if self.eat(Punct::Semi) {
                continue;
            }
            members.push(self.parse_class_member()?);
        }
        Ok(Stmt::Class { name, members, pos })
    }

    /// One class body element. A method is recorded as a `FunctionDecl` — the
    /// shape that already carries a name, parameters and a body — and a field as
    /// an assignment to `this`, which is what a field declaration means.
    /// `static`, accessors and computed names are refused by name rather than
    /// folded into something they are not: they all hinge on `this` or on a
    /// property key only known at runtime, and §9.8 models neither.
    /// One `for`-declaration declarator, its `=` already consumed: read the
    /// `[~In]` initializer and assemble the declaration node `kw` asks for.
    fn for_declarator(&mut self, kw: Keyword, pos: Position, pattern: Pattern) -> P<Stmt> {
        let init = self.parse_assign_no_in()?;
        Ok(match kw {
            Keyword::Const => Stmt::Const { pattern, init, pos },
            Keyword::Let => Stmt::Let { pattern, init: Some(init), pos },
            _ => Stmt::Var { pattern, init: Some(init), pos },
        })
    }

    /// `true` when the current token is the identifier `word` and what follows it
    /// on the *same line* is a member name — which is what makes `static`, `get`,
    /// `set` and `async` modifiers rather than the member itself. `get = 1` is a
    /// field named `get` and `get() {}` is a method named `get`, so the following
    /// token decides, and a line break ends the modifier's scope.
    fn is_contextual_modifier(&self, word: &str) -> bool {
        if !matches!(self.kind(), TokenKind::Ident) || self.cur().text != word {
            return false;
        }
        let Some(next) = self.toks.get(self.i + 1) else { return false };
        if next.line != self.cur().line {
            return false;
        }
        // Anything that can begin a member name: an identifier, a keyword used as
        // a name, `*` for a generator, `[` for a computed key.
        !matches!(
            &next.kind,
            TokenKind::Punct(Punct::Eq | Punct::Semi | Punct::LParen | Punct::Comma | Punct::RBrace | Punct::Colon)
        )
    }

    fn parse_class_member(&mut self) -> P<Stmt> {
        let pos = self.pos();
        // `static`, `get` and `set` are contextual keywords: they lex as
        // identifiers, exactly like `from` in an import, so they are matched on
        // text rather than on a `Keyword` variant.
        for w in ["static", "get", "set"] {
            if self.is_contextual_modifier(w) {
                match w {
                    "static" => return self.err("`static` class members are outside the analyzable subset"),
                    _ => return self.err("accessor class members are outside the analyzable subset"),
                }
            }
        }
        // A generator method (`*m() {}`) and a computed key (`[k]() {}`) are both
        // refused before a name is read, so neither can be mistaken for a field.
        if self.cur().is_punct(Punct::Star) {
            return self.err("generator methods are outside the analyzable subset");
        }
        if self.cur().is_punct(Punct::LBracket) {
            return self.err("computed class member names are outside the analyzable subset");
        }
        let is_async = self.is_contextual_modifier("async");
        if is_async {
            self.bump();
        }
        let name = self.property_name()?;
        // A method: the name is immediately followed by its parameter list.
        if self.cur().is_punct(Punct::LParen) {
            let params = self.parse_params()?;
            let body = self.parse_block()?;
            // `constructor` needs no special case: it is a method by shape, and
            // the whole class is refused at lowering regardless of what it holds.
            return Ok(Stmt::FunctionDecl { name, params, body, is_async, pos });
        }
        // A field: `name`, optionally `= init`, terminated. `this.name = init` is
        // the assignment it desugars to, which keeps the member list a list of
        // statements without inventing a new variant for §9.4 to forget.
        let init = if self.eat(Punct::Eq) { self.parse_expr()? } else { Expr::Undefined(pos) };
        self.semicolon()?;
        Ok(Stmt::Expr(Expr::Assign {
            op: AssignOp::Assign,
            target: Box::new(Expr::Member { obj: Box::new(Expr::This(pos)), prop: name, computed: false, pos }),
            value: Box::new(init),
            pos,
        }))
    }

    fn parse_return(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Return)?;
        // `return;` carries no value; anything else must be an expression, so the
        // distinction is made on the next token rather than by guessing.
        let val = if self.cur().is_punct(Punct::Semi) || matches!(self.kind(), TokenKind::Eof) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.semicolon()?;
        Ok(Stmt::Return(val, pos))
    }

    fn parse_break_continue(&mut self, is_break: bool) -> P<Stmt> {
        let pos = self.pos();
        if is_break {
            self.expect_kw(Keyword::Break)?;
        } else {
            self.expect_kw(Keyword::Continue)?;
        }
        let label = if self.cur().is_punct(Punct::Semi) || matches!(self.kind(), TokenKind::Eof) {
            None
        } else {
            Some(self.ident()?)
        };
        self.semicolon()?;
        Ok(if is_break { Stmt::Break(label, pos) } else { Stmt::Continue(label, pos) })
    }

    fn parse_throw(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Throw)?;
        let e = self.parse_expr()?;
        self.semicolon()?;
        Ok(Stmt::Throw(e, pos))
    }

    /// §9.3's `parse_try`. Parsed, never lowered into an analyzable loop: §9.14
    /// puts `try`/`catch` inside a loop out of scope because the exception edge is
    /// a control edge the region analysis has no representation for.
    fn parse_try(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Try)?;
        let body = self.parse_block()?;
        let catch = if self.eat_kw(Keyword::Catch) {
            let pattern = if self.eat(Punct::LParen) {
                let p = self.parse_pattern()?;
                self.expect(Punct::RParen)?;
                Some(p)
            } else {
                None
            };
            Some(CatchClause { pattern, body: self.parse_block()? })
        } else {
            None
        };
        let finally = if self.eat_kw(Keyword::Finally) { Some(self.parse_block()?) } else { None };
        if catch.is_none() && finally.is_none() {
            return self.err("`try` requires `catch` or `finally`");
        }
        Ok(Stmt::Try { body, catch, finally, pos })
    }

    fn parse_switch(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Switch)?;
        self.expect(Punct::LParen)?;
        let subject = self.parse_expr()?;
        self.expect(Punct::RParen)?;
        self.expect(Punct::LBrace)?;
        let mut cases = Vec::new();
        while !self.eat(Punct::RBrace) {
            if matches!(self.kind(), TokenKind::Eof) {
                return self.err("unterminated switch body");
            }
            let test = if self.eat_kw(Keyword::Case) {
                Some(self.parse_expr()?)
            } else if self.eat_kw(Keyword::Default) {
                None
            } else {
                return self.err("expected `case` or `default` in switch");
            };
            self.expect(Punct::Colon)?;
            let mut body = Vec::new();
            while !(self.cur().is_keyword(Keyword::Case)
                || self.cur().is_keyword(Keyword::Default)
                || self.cur().is_punct(Punct::RBrace))
            {
                self.check_bad()?;
                body.push(self.parse_stmt()?);
            }
            cases.push(SwitchCase { test, body });
        }
        Ok(Stmt::Switch { subject, cases, pos })
    }

    fn parse_import(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Import)?;
        let mut specifiers = Vec::new();
        if self.eat(Punct::LBrace) {
            loop {
                if self.eat(Punct::RBrace) {
                    break;
                }
                specifiers.push(self.ident()?);
                if !self.eat(Punct::Comma) {
                    self.expect(Punct::RBrace)?;
                    break;
                }
            }
        } else if matches!(self.kind(), TokenKind::Ident) {
            specifiers.push(self.ident()?);
        }
        // `from` is a contextual keyword — it lexes as an identifier, so it is
        // matched on text rather than on a Keyword variant.
        let from = self.cur().clone();
        if from.text != "from" {
            return Err(ParseError {
                msg: format!("expected `from`, found {}", from.describe()),
                line: from.line,
                col: from.col,
            });
        }
        self.bump();
        let source = match self.kind() {
            TokenKind::Str => self.cur().text.clone(),
            _ => return self.err("expected a module specifier string"),
        };
        self.bump();
        self.semicolon()?;
        Ok(Stmt::Import { specifiers, source, pos })
    }

    fn parse_export(&mut self) -> P<Stmt> {
        let pos = self.pos();
        self.expect_kw(Keyword::Export)?;
        let default = self.eat_kw(Keyword::Default);
        let decl = Box::new(self.parse_stmt()?);
        Ok(Stmt::Export { decl, default, pos })
    }

    /// Require a statement terminator. See the module docs: ASI is not
    /// implemented, so a missing `;` is an error rather than an inference. A
    /// closing `}` legitimately ends a statement, and so does end of input.
    fn semicolon(&mut self) -> P<()> {
        if self.eat(Punct::Semi) {
            return Ok(());
        }
        if self.cur().is_punct(Punct::RBrace) || matches!(self.kind(), TokenKind::Eof) {
            return Ok(());
        }
        self.err(format!("expected `;`, found {}", self.cur().describe()))
    }

    /// §9.3's parameter list, which is a list of patterns (§9.5's
    /// `ClosureInfo::params`).
    fn parse_params(&mut self) -> P<Vec<Pattern>> {
        self.expect(Punct::LParen)?;
        let mut params = Vec::new();
        if self.eat(Punct::RParen) {
            return Ok(params);
        }
        loop {
            params.push(self.parse_pattern()?);
            if self.eat(Punct::Comma) {
                if self.cur().is_punct(Punct::RParen) {
                    break;
                }
                continue;
            }
            self.expect(Punct::RParen)?;
            break;
        }
        Ok(params)
    }

    /// §9.3's `parse_expr`: the comma operator, then assignment, then the ladder.
    fn parse_expr(&mut self) -> P<Expr> {
        let pos = self.pos();
        let mut items = vec![self.parse_assign()?];
        while self.eat(Punct::Comma) {
            items.push(self.parse_assign()?);
        }
        if items.len() == 1 {
            return Ok(items.pop().unwrap());
        }
        Ok(Expr::Seq(items, pos))
    }

    /// Assignment is right-associative and sits below the ladder, so `a = b = c`
    /// reads as `a = (b = c)`.
    ///
    /// `&&=`, `||=` and `??=` are deliberately not recognized: the lexer has no
    /// token for them, so they arrive as `&&` followed by `=` and are rejected.
    /// A logical assignment writes its target only when a condition holds, which
    /// is a control edge the scalar model has no place for, and §9.14's rule is
    /// that unanalyzable means refused rather than approximated.
    fn parse_assign(&mut self) -> P<Expr> {
        let left = self.parse_conditional()?;
        let pos = left.pos();
        // A bare `x => ...` (no parentheses) is an arrow whose parameter list is a
        // single identifier, and it is only recognizable after the parameter has
        // been read, since `x` is a complete expression on its own. The left side
        // is reinterpreted as a pattern; a non-pattern left (`a + b => c`) is
        // refused rather than being silently narrowed to its first operand.
        if self.cur().is_punct(Punct::Arrow) {
            let params = vec![binding_from_expr(&left)?];
            self.bump();
            let body = self.parse_arrow_body()?;
            return Ok(Expr::Arrow { params, body, is_async: false, pos });
        }
        let op = match self.kind() {
            TokenKind::Punct(Punct::Eq) => AssignOp::Assign,
            TokenKind::Punct(Punct::PlusEq) => AssignOp::Add,
            TokenKind::Punct(Punct::MinusEq) => AssignOp::Sub,
            TokenKind::Punct(Punct::StarEq) => AssignOp::Mul,
            TokenKind::Punct(Punct::SlashEq) => AssignOp::Div,
            TokenKind::Punct(Punct::PercentEq) => AssignOp::Mod,
            _ => return Ok(left),
        };
        self.bump();
        let value = Box::new(self.parse_assign()?);
        Ok(Expr::Assign { op, target: Box::new(left), value, pos })
    }

    fn parse_conditional(&mut self) -> P<Expr> {
        let cond = self.parse_binary(0)?;
        let pos = cond.pos();
        if !self.eat(Punct::Question) {
            return Ok(cond);
        }
        // Both branches are `[+In]`: `for (a ? b in c : d; ;)` is legal, so the
        // flag has to come off inside a ternary even when the ternary sits in a
        // `for` clause. A `?` below aborts the whole parse — there is no error
        // recovery — so only the success path needs to restore the flag.
        let saved = self.open_group();
        let then = self.parse_assign()?;
        self.expect(Punct::Colon)?;
        let els = self.parse_assign()?;
        self.close_group(saved);
        Ok(Expr::Conditional {
            cond: Box::new(cond),
            then: Box::new(then),
            els: Box::new(els),
            pos,
        })
    }

    /// Precedence climbing. `min` is the lowest level allowed here; an operator
    /// below it belongs to the caller.
    fn parse_binary(&mut self, min: u8) -> P<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let Some(prec) = prec_of(self.kind(), self.no_in) else { break };
            if prec.level < min {
                break;
            }
            let op = binop_of(self.kind());
            let pos = left.pos();
            self.bump();
            // A right-associative operator recurses at its own level; a
            // left-associative one climbs past it. That is the whole difference.
            let next_min = if prec.right { prec.level } else { prec.level + 1 };
            let right = self.parse_binary(next_min)?;
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right), pos };
        }
        Ok(left)
    }

    /// Parse an expression with `in` disabled, for the clauses of a classical
    /// `for` header. Restored on the way out so a nested iteration loop inside
    /// the body still sees `in`.
    fn parse_expr_no_in(&mut self) -> P<Expr> {
        let saved = self.no_in;
        self.no_in = true;
        let e = self.parse_expr();
        self.no_in = saved;
        e
    }

    /// The initializer of a `for`-declaration head: `[~In]`, and stopping at a
    /// comma, which in this position separates declarators (`for (let i = 0, j =
    /// n; ...)`) rather than being the comma operator.
    fn parse_assign_no_in(&mut self) -> P<Expr> {
        let saved = self.no_in;
        self.no_in = true;
        let e = self.parse_assign();
        self.no_in = saved;
        e
    }

    /// Parse an expression with `in` *enabled*, which is what every nested
    /// context requires: the spec marks `ParenthesizedExpression`, array and
    /// object literal elements, argument lists and function bodies `[+In]`, so
    /// only the direct clauses of a `for` header are `[~In]`. Without this,
    /// `for ((a in b); ;)` — a classical loop whose test is an `in` expression —
    /// would be a parse error.
    fn parse_fresh_expr(&mut self) -> P<Expr> {
        let saved = self.open_group();
        let e = self.parse_expr();
        self.close_group(saved);
        e
    }

    /// Enter a nested construct, where `in` is legal again. Returns the flag to
    /// restore, so the caller cannot forget it.
    fn open_group(&mut self) -> bool {
        let saved = self.no_in;
        self.no_in = false;
        saved
    }

    /// Leave a nested construct.
    fn close_group(&mut self, saved: bool) {
        self.no_in = saved;
    }

    /// Attach a `= default` to a pattern head already read. Used by the `for`
    /// declaration head, which has to see the token *after* the pattern before it
    /// knows whether an `=` belongs to a default (`for (const {a = 1} of t)`) or
    /// to the initializer (`for (let i = 0; ...)`). A failed default is an error,
    /// not a pattern with the default dropped: silently narrowing the pattern
    /// would change which names the binding declares.
    fn finish_pattern(&mut self, p: Pattern) -> P<Pattern> {
        if !self.eat(Punct::Eq) {
            return Ok(p);
        }
        let pos = p.pos();
        let saved = self.open_group();
        let right = self.parse_assign();
        self.close_group(saved);
        Ok(Pattern::Default { left: Box::new(p), right: Box::new(right?), pos })
    }






    /// Unary operators, including prefix `++`/`--` and the keyword operators.
    ///
    /// The depth guard lives here because every expression nesting level passes
    /// through this function — `parse_expr → parse_assign → parse_conditional →
    /// parse_binary → parse_unary` is the cycle, and a parenthesized expression,
    /// an array element, an object value and an argument each re-enter it. Guarding
    /// one point on the cycle bounds the whole recursion; guarding each re-entry
    /// would count the same nesting level several times and make the limit
    /// meaningless. The ladder's own recursion (`parse_binary` for a
    /// right-associative operator) re-enters through here too, so it is covered.
    fn parse_unary(&mut self) -> P<Expr> {
        self.enter()?;
        let r = self.parse_unary_inner();
        self.leave();
        r
    }

    fn parse_unary_inner(&mut self) -> P<Expr> {
        let pos = self.pos();
        let t = self.cur().clone();
        let op = match t.kind {
            TokenKind::Punct(Punct::Bang) => Some(UnOp::Not),
            TokenKind::Punct(Punct::Minus) => Some(UnOp::Neg),
            TokenKind::Punct(Punct::Plus) => Some(UnOp::Pos),
            TokenKind::Punct(Punct::Tilde) => Some(UnOp::BitNot),
            TokenKind::Keyword(Keyword::Typeof) => Some(UnOp::TypeOf),
            TokenKind::Keyword(Keyword::Void) => Some(UnOp::Void),
            TokenKind::Keyword(Keyword::Delete) => Some(UnOp::Delete),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let arg = self.parse_unary()?;
            return Ok(Expr::Unary { op, arg: Box::new(arg), pos });
        }
        if t.is_punct(Punct::PlusPlus) || t.is_punct(Punct::MinusMinus) {
            let op = if t.is_punct(Punct::PlusPlus) { UpdateOp::Inc } else { UpdateOp::Dec };
            self.bump();
            let operand = self.parse_unary()?;
            return Ok(Expr::Update { op, operand: Box::new(operand), prefix: true, pos });
        }
        if t.is_keyword(Keyword::Await) {
            self.bump();
            let arg = self.parse_unary()?;
            return Ok(Expr::Await(Box::new(arg), pos));
        }
        if t.is_keyword(Keyword::Yield) {
            self.bump();
            // `yield` alone is the generator form; `yield x` takes an operand.
            // The terminator check is what distinguishes them.
            let arg = if self.cur().is_punct(Punct::Semi)
                || self.cur().is_punct(Punct::RParen)
                || self.cur().is_punct(Punct::RBracket)
                || self.cur().is_punct(Punct::RBrace)
                || self.cur().is_punct(Punct::Comma)
                || self.cur().is_punct(Punct::Colon)
                || matches!(self.kind(), TokenKind::Eof)
            {
                None
            } else {
                Some(Box::new(self.parse_assign()?))
            };
            return Ok(Expr::Yield(arg, pos));
        }
        self.parse_postfix()
    }


    fn parse_call_member(&mut self) -> P<Expr> {
        let mut e = if self.cur().is_keyword(Keyword::New) {
            self.parse_new()?
        } else {
            self.parse_primary()?
        };
        loop {
            let pos = e.pos();
            if self.eat(Punct::Dot) {
                let prop = self.property_name()?;
                e = Expr::Member { obj: Box::new(e), prop, computed: false, pos };
            } else if self.eat(Punct::QuestionDot) {
                if self.eat(Punct::LParen) {
                    let args = self.parse_args_after_lparen()?;
                    e = Expr::Call { callee: Box::new(e), args, pos };
                } else if self.eat(Punct::LBracket) {
                    let index = self.parse_expr()?;
                    self.expect(Punct::RBracket)?;
                    e = Expr::Index { obj: Box::new(e), index: Box::new(index), pos };
                } else {
                    let prop = self.property_name()?;
                    e = Expr::Member { obj: Box::new(e), prop, computed: false, pos };
                }
            } else if self.cur().is_punct(Punct::LBracket) {
                self.bump();
                let saved = self.open_group();
                let index = self.parse_expr()?;
                self.expect(Punct::RBracket)?;
                self.close_group(saved);
                e = Expr::Index { obj: Box::new(e), index: Box::new(index), pos };
            } else if self.cur().is_punct(Punct::LParen) {
                self.bump();
                let args = self.parse_args_after_lparen()?;
                e = Expr::Call { callee: Box::new(e), args, pos };
            } else if matches!(self.kind(), TokenKind::Template) {
                // Tagged template: `` f`x` ``. A tagged template *is* a call of
                // the tag, so it is built as one — replacing `e` with a bare
                // template would drop the callee and make the tree lie about
                // what the source said. §9.4 refuses it either way.
                let raw = self.cur().text.clone();
                let tpos = self.pos();
                self.bump();
                let tmpl = Expr::Template {
                    parts: template_parts(&raw, tpos, &mut self.depth)?,
                    pos: tpos,
                };
                e = Expr::Call { callee: Box::new(e), args: vec![tmpl], pos };
            } else {
                break;
            }
        }
        Ok(e)
    }

    /// `new a.b()` attaches the call to `b`, not to `a`, so the callee is parsed
    /// as a member chain before the argument list is looked for. `new` without
    /// parentheses (`new Date`) is legal and gives an empty argument list, which
    /// §9.4 reads as a zero-argument allocation.
    fn parse_new(&mut self) -> P<Expr> {
        let pos = self.pos();
        self.expect_kw(Keyword::New)?;
        if self.cur().is_keyword(Keyword::New) {
            return self.err("`new new` is not a construct expression");
        }
        let callee = self.parse_call_member()?;
        let args = if self.cur().is_punct(Punct::LParen) {
            self.bump();
            self.parse_args_after_lparen()?
        } else {
            Vec::new()
        };
        Ok(Expr::New { callee: Box::new(callee), args, pos })
    }

    /// Arguments, called with the `(` already consumed. Spread can only appear in
    /// this position or in an array literal, which is why it is handled here.
    fn parse_args_after_lparen(&mut self) -> P<Vec<Expr>> {
        // An argument list is `[+In]`: `f(a in b)` passes one argument, the result
        // of the test, not two.
        let saved = self.open_group();
        let mut args = Vec::new();
        if self.eat(Punct::RParen) {
            self.close_group(saved);
            return Ok(args);
        }
        loop {
            if self.cur().is_punct(Punct::Ellipsis) {
                let pos = self.pos();
                self.bump();
                let e = self.parse_assign()?;
                args.push(Expr::Spread(Box::new(e), pos));
            } else {
                args.push(self.parse_assign()?);
            }
            if self.eat(Punct::Comma) {
                if self.cur().is_punct(Punct::RParen) {
                    break;
                }
                continue;
            }
            self.expect(Punct::RParen)?;
            break;
        }
        self.close_group(saved);
        Ok(args)
    }

    /// A property name. Keywords are legal here (`obj.in`, `obj.default`), so the
    /// name is taken from the token text whatever its class.
    fn property_name(&mut self) -> P<String> {
        let t = self.cur().clone();
        match &t.kind {
            TokenKind::Ident | TokenKind::Num => {
                self.bump();
                Ok(t.text)
            }
            TokenKind::Keyword(k) => {
                self.bump();
                Ok(k.as_str().to_string())
            }
            _ => Err(ParseError {
                msg: format!("expected a property name, found {}", t.describe()),
                line: t.line,
                col: t.col,
            }),
        }
    }

    /// §9.3's `parse_primary`: the tokens that start an expression and have no
    /// further internal structure the ladder cares about.
    fn parse_primary(&mut self) -> P<Expr> {
        let pos = self.pos();
        let t = self.cur().clone();
        match &t.kind {
            TokenKind::Num => {
                self.bump();
                Ok(Expr::Num { int: t.num, text: t.text, pos })
            }
            TokenKind::Str => {
                self.bump();
                Ok(Expr::Str(t.text, pos))
            }
            TokenKind::Template => {
                self.bump();
                Ok(Expr::Template { parts: template_parts(&t.text, pos, &mut self.depth)?, pos })
            }
            TokenKind::Regex => {
                self.bump();
                Ok(Expr::Str(t.text, pos))
            }
            TokenKind::Ident => {
                self.bump();
                Ok(Expr::Ident(t.text, pos))
            }
            TokenKind::Keyword(k) => match k {
                Keyword::True => {
                    self.bump();
                    Ok(Expr::Bool(true, pos))
                }
                Keyword::False => {
                    self.bump();
                    Ok(Expr::Bool(false, pos))
                }
                Keyword::Null => {
                    self.bump();
                    Ok(Expr::Null(pos))
                }
                Keyword::Undefined => {
                    self.bump();
                    Ok(Expr::Undefined(pos))
                }
                Keyword::This => {
                    self.bump();
                    Ok(Expr::This(pos))
                }
                Keyword::Super => {
                    self.bump();
                    Ok(Expr::Super(pos))
                }
                Keyword::Function => self.parse_function_expr(),
                Keyword::Async => {
                    // `async` is only a modifier when what follows is `function`
                    // or a parameter list on the same line; otherwise it is a
                    // plain identifier (`async()` is a call).
                    let next = match self.toks.get(self.i + 1) {
                        Some(n) => n.clone(),
                        None => self.toks.last().unwrap().clone(),
                    };
                    // `async x => y` is also an arrow, with a bare identifier
                    // parameter, so the token two ahead has to be considered.
                    let bare_arrow = matches!(self.toks.get(self.i + 2), Some(n) if n.is_punct(Punct::Arrow));
                    let modifies = next.line == t.line
                        && (next.is_keyword(Keyword::Function)
                            || next.is_punct(Punct::LParen)
                            || (matches!(next.kind, TokenKind::Ident) && bare_arrow));
                    if modifies {
                        self.parse_async(&next, pos)
                    } else {
                        self.bump();
                        Ok(Expr::Ident(t.text, pos))
                    }
                }
                _ => self.err(format!("{} cannot start an expression", t.describe())),
            },
            TokenKind::Punct(p) => match p {
                Punct::LParen => {
                    // Either a parenthesized expression or an arrow parameter
                    // list; `(` alone cannot decide which, so the contents are
                    // parsed once and reinterpreted afterwards.
                    self.parse_paren_head()
                }
                Punct::LBracket => self.parse_array_literal(),
                Punct::LBrace => self.parse_object_literal(),
                _ => self.err(format!("{} cannot start an expression", t.describe())),
            },
            TokenKind::Eof => self.err("unexpected end of input"),
            TokenKind::Bad(_) => self.err(t.describe()),
        }
    }

    /// `i++` at the end of a line is `i`, not `i++` joined to a statement that
    /// begins with `++`. The one ASI rule the parser keeps, because dropping it
    /// mis-parses the expression rather than the statement boundary.
    fn parse_postfix(&mut self) -> P<Expr> {
        let mut e = self.parse_call_member()?;
        if !self.starts_line()
            && (self.cur().is_punct(Punct::PlusPlus) || self.cur().is_punct(Punct::MinusMinus))
        {
            let op = if self.cur().is_punct(Punct::PlusPlus) { UpdateOp::Inc } else { UpdateOp::Dec };
            let pos = e.pos();
            self.bump();
            e = Expr::Update { op, operand: Box::new(e), prefix: false, pos };
        }
        Ok(e)
    }

    /// `true` when the current token begins a line after the token just read.
    fn starts_line(&self) -> bool {
        match (self.toks.get(self.i.saturating_sub(1)), self.toks.get(self.i)) {
            (Some(prev), Some(cur)) => prev.line != cur.line,
            _ => false,
        }
    }

    /// `function` expressions, named or anonymous (§9.3's `parse_function`).
    fn parse_function_expr(&mut self) -> P<Expr> {
        let pos = self.pos();
        self.expect_kw(Keyword::Function)?;
        let name = if matches!(self.kind(), TokenKind::Ident) { Some(self.ident()?) } else { None };
        let params = self.parse_params()?;
        let body = self.parse_block()?;
        Ok(Expr::Function { name, params, body, is_async: false, pos })
    }

    /// `async function f() {}` and `async (x) => {}`. Both are parsed so that the
    /// lowering can refuse by name: §9.14 puts `async`/`await` out of scope, and a
    /// refusal is only useful if it can say *which* construct it refused.
    /// Generators (`function*`) are refused at the parser, because `*` after
    /// `function` is not an expression operator and no subset of it is analyzable.
    fn parse_async(&mut self, next: &Token, pos: Position) -> P<Expr> {
        self.expect_kw(Keyword::Async)?;
        if next.is_keyword(Keyword::Function) {
            let name_pos = self.pos();
            self.expect_kw(Keyword::Function)?;
            if self.cur().is_punct(Punct::Star) {
                let _ = name_pos;
                return self.err("generators (`function*`) are not supported");
            }
            let name = if matches!(self.kind(), TokenKind::Ident) { Some(self.ident()?) } else { None };
            let params = self.parse_params()?;
            let body = self.parse_block()?;
            return Ok(Expr::Function { name, params, body, is_async: true, pos });
        }
        // `async (a, b) => ...` — the parameter list follows. `async x => ...` has
        // a bare identifier parameter, which cannot go through `parse_params`.
        let params = if matches!(self.kind(), TokenKind::Ident) {
            let ppos = self.pos();
            let name = self.ident()?;
            self.expect(Punct::Arrow)?;
            vec![Pattern::Binding { name, pos: ppos }]
        } else {
            let params = self.parse_params()?;
            self.expect(Punct::Arrow)?;
            params
        };
        let body = self.parse_arrow_body()?;
        Ok(Expr::Arrow { params, body, is_async: true, pos })
    }

    /// Decide between `(expr)` and `(a, b) => ...`. The two are only separable
    /// after the closing paren, so the matching `)` is *located* by a bounded
    /// scan and the contents are parsed once, from a reset cursor. Locating is
    /// not error recovery: if the scan finds no `)`, the input really is
    /// malformed and the error names it.
    fn parse_paren_head(&mut self) -> P<Expr> {
        let pos = self.pos();
        self.expect(Punct::LParen)?;
        let close = self.matching_close(self.i)?;
        let is_arrow = matches!(self.toks.get(close + 1), Some(t) if t.is_punct(Punct::Arrow));
        // A parenthesized expression is `[+In]`: `for ((a in b); ;)` is a
        // classical loop whose test is an `in` expression.
        let saved = self.open_group();
        if is_arrow {
            let params = self.parse_params_before(close)?;
            self.expect(Punct::Arrow)?;
            let body = self.parse_arrow_body()?;
            self.close_group(saved);
            return Ok(Expr::Arrow { params, body, is_async: false, pos });
        }
        if self.cur().is_punct(Punct::RParen) {
            return self.err("empty parenthesized expression");
        }
        let e = self.parse_expr()?;
        self.expect(Punct::RParen)?;
        self.close_group(saved);
        Ok(e)
    }

    /// The index of the bracket closing the one that opened just before `from`.
    /// Read-only — the caller's cursor is untouched — and purely structural, so
    /// this is lookahead, not error recovery: it never repairs anything, it only
    /// decides which rule to run, and a missing close is reported by the caller
    /// when its own `expect` fails.
    fn matching_close(&self, from: usize) -> P<usize> {
        let mut depth = 1usize;
        let mut j = from;
        while j < self.toks.len() {
            if let TokenKind::Punct(p) = &self.toks[j].kind {
                match p {
                    Punct::LParen | Punct::LBracket | Punct::LBrace => depth += 1,
                    Punct::RParen | Punct::RBracket | Punct::RBrace => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j);
                        }
                    }
                    _ => {}
                }
            }
            j += 1;
        }
        self.err("unterminated `(`")
    }

    /// Parameters for an arrow, bounded by the `)` at `close`. The bound matters
    /// because `(a, b) => c` cannot use `parse_params`: nothing in a pattern's own
    /// grammar says where the list ends, and only the pre-scanned `)` does.
    fn parse_params_before(&mut self, close: usize) -> P<Vec<Pattern>> {
        let mut params = Vec::new();
        if self.i == close {
            self.bump();
            return Ok(params);
        }
        loop {
            params.push(self.parse_pattern()?);
            if self.i >= close {
                break;
            }
            if !self.eat(Punct::Comma) {
                break;
            }
            // A trailing comma before the close is legal: `(a,) => a`.
            if self.i == close {
                break;
            }
        }
        self.expect(Punct::RParen)?;
        Ok(params)
    }

    /// `=> expr` or `=> { stmts }`. The brace form is a body, not an object
    /// literal — the one place where `{` means something other than what
    /// `parse_primary` would read it as.
    fn parse_arrow_body(&mut self) -> P<ArrowBody> {
        if self.cur().is_punct(Punct::LBrace) {
            // `parse_block` already resets the flag for its own body.
            return Ok(ArrowBody::Block(self.parse_block()?));
        }
        // A concise arrow body is `[+In]`: `x => x in y` is a valid arrow whose
        // body is an `in` test.
        let saved = self.open_group();
        let e = self.parse_assign();
        self.close_group(saved);
        Ok(ArrowBody::Expr(Box::new(e?)))
    }



    /// `[1, 2, 3]`, `[a, , b]` and `[x, ...rest]`. A hole is `None`; the lowering
    /// must know a slot was skipped rather than reading a value that is not there.
    fn parse_array_literal(&mut self) -> P<Expr> {
        let pos = self.pos();
        self.expect(Punct::LBracket)?;
        // An array literal is `[+In]`: `[a in b]` is a one-element array holding
        // the result of the `in` test.
        let saved = self.open_group();
        let mut elements: Vec<Option<Expr>> = Vec::new();
        if self.eat(Punct::RBracket) {
            self.close_group(saved);
            return Ok(Expr::ArrayLiteral { elements, pos });
        }
        loop {
            if self.cur().is_punct(Punct::Comma) {
                // A hole: `[, x]` and `[x, , y]` both skip a slot.
                elements.push(None);
            } else if self.cur().is_punct(Punct::Ellipsis) {
                let spos = self.pos();
                self.bump();
                let e = self.parse_assign()?;
                elements.push(Some(Expr::Spread(Box::new(e), spos)));
            } else {
                elements.push(Some(self.parse_assign()?));
            }
            if !self.eat(Punct::Comma) {
                break;
            }
            if self.cur().is_punct(Punct::RBracket) {
                break;
            }
        }
        self.expect(Punct::RBracket)?;
        self.close_group(saved);
        Ok(Expr::ArrayLiteral { elements, pos })
    }

    /// `{ a, b: c, "d": e, x: [1, 2] }`. Methods, accessors and spread are refused
    /// here, at the token that makes the property one of those: they need a `this`
    /// model §9.8 does not have, and lowering them into a plain property would
    /// silently analyze a program the source does not contain.
    fn parse_object_literal(&mut self) -> P<Expr> {
        let pos = self.pos();
        self.expect(Punct::LBrace)?;
        let saved = self.open_group();
        let mut props: Vec<(String, Expr)> = Vec::new();
        while !self.eat(Punct::RBrace) {
            self.check_bad()?;
            if self.cur().is_punct(Punct::Ellipsis) {
                return self.err("object spread (`{...o}`) is outside the analyzable subset");
            }
            if self.cur().is_punct(Punct::Star) || self.cur().is_punct(Punct::LBracket) {
                return self.err("computed or generator property names are outside the analyzable subset");
            }
            let key = self.property_name()?;
            // What follows the name decides the form. `(` is a method, `:` is an
            // explicit value, `get`/`set` were already consumed as a name, and
            // anything else (`}` or `,`) is the shorthand `{ a }`.
            if self.cur().is_punct(Punct::LParen) {
                return self.err(format!("object method `{key}()` is outside the analyzable subset"));
            }
            let value = if self.eat(Punct::Colon) {
                self.parse_assign()?
            } else {
                Expr::Ident(key.clone(), self.pos())
            };
            props.push((key, value));
            if self.eat(Punct::Comma) {
                continue;
            }
            self.expect(Punct::RBrace)?;
            break;
        }
        self.close_group(saved);
        Ok(Expr::ObjectLiteral { props, pos })
    }

    /// §9.3's `parse_binding_element`: a pattern, optionally `= default`.
    fn parse_pattern(&mut self) -> P<Pattern> {
        let p = self.parse_pattern_head()?;
        if !self.eat(Punct::Eq) {
            return Ok(p);
        }
        let pos = p.pos();
        let right = Box::new(self.parse_assign()?);
        Ok(Pattern::Default { left: Box::new(p), right, pos })
    }

    /// The pattern itself, without its default.
    fn parse_pattern_head(&mut self) -> P<Pattern> {
        let pos = self.pos();
        if matches!(self.kind(), TokenKind::Ident) {
            let name = self.ident()?;
            return Ok(Pattern::Binding { name, pos });
        }
        if self.cur().is_punct(Punct::LBracket) {
            return self.parse_array_pattern();
        }
        if self.cur().is_punct(Punct::LBrace) {
            return self.parse_object_pattern();
        }
        self.err("expected a binding name, `[` or `{`")
    }

    /// `[a, b, ...rest]`, with holes. `...rest` is stored separately rather than
    /// as the last element because §9.6 emits it as one length-dependent load:
    /// the rest binding is not at an index, it *is* the remaining slice.
    fn parse_array_pattern(&mut self) -> P<Pattern> {
        let pos = self.pos();
        self.expect(Punct::LBracket)?;
        let mut elements: Vec<Option<Pattern>> = Vec::new();
        let mut rest: Option<Box<Pattern>> = None;
        if self.eat(Punct::RBracket) {
            return Ok(Pattern::Array { elements, rest, pos });
        }
        loop {
            if self.cur().is_punct(Punct::Ellipsis) {
                self.bump();
                let inner = self.parse_pattern()?;
                rest = Some(Box::new(inner));
                // `[...r, x]` has no reading: the rest takes the tail.
                if self.eat(Punct::Comma) && !self.cur().is_punct(Punct::RBracket) {
                    return self.err("rest element must come last in an array pattern");
                }
                break;
            }
            if self.cur().is_punct(Punct::Comma) {
                elements.push(None);
            } else {
                elements.push(Some(self.parse_pattern()?));
            }
            if !self.eat(Punct::Comma) {
                break;
            }
            if self.cur().is_punct(Punct::RBracket) {
                break;
            }
        }
        self.expect(Punct::RBracket)?;
        Ok(Pattern::Array { elements, rest, pos })
    }

    /// `{a, b: c, "d": e, ...rest}`. `key` records the *property read* and
    /// `value` the pattern bound to it, which is what lets `{a: [x, y]}` lower
    /// without a second pass.
    fn parse_object_pattern(&mut self) -> P<Pattern> {
        let pos = self.pos();
        self.expect(Punct::LBrace)?;
        let mut props: Vec<ObjectProp> = Vec::new();
        let mut rest: Option<Box<Pattern>> = None;
        while !self.eat(Punct::RBrace) {
            self.check_bad()?;
            if self.cur().is_punct(Punct::Ellipsis) {
                self.bump();
                rest = Some(Box::new(self.parse_pattern_head()?));
                self.eat(Punct::Comma);
                break;
            }
            let key = self.property_name()?;
            if self.eat(Punct::Colon) {
                let value = self.parse_pattern()?;
                props.push(ObjectProp { key, value, shorthand: false });
            } else {
                // Shorthand: the property binds to a local of the same name, and
                // a default may follow (`{a = 1}`).
                let mut value = Pattern::Binding { name: key.clone(), pos: self.pos() };
                if self.eat(Punct::Eq) {
                    let vp = value.pos();
                    value = Pattern::Default {
                        left: Box::new(value),
                        right: Box::new(self.parse_assign()?),
                        pos: vp,
                    };
                }
                props.push(ObjectProp { key, value, shorthand: true });
            }
            if !self.eat(Punct::Comma) {
                break;
            }
        }
        self.expect(Punct::RBrace)?;
        Ok(Pattern::Object { props, rest, pos })
    }
}

/// The binary operator a token names. Split from [`prec_of`] because the two
/// questions — "does this bind, and how tightly" and "which node do I build" —
/// have different domains: `prec_of` also answers for `in` and `instanceof`,
/// which are keywords, while this must name every operator the ladder admits.
/// A token with precedence but no entry here would be mis-parsed, so the two
/// tables are kept listing the same set.
fn binop_of(kind: &TokenKind) -> BinOp {
    match kind {
        TokenKind::Keyword(Keyword::In) => BinOp::In,
        TokenKind::Keyword(Keyword::Instanceof) => BinOp::Instanceof,
        TokenKind::Punct(p) => match p {
            Punct::Star => BinOp::Mul,
            Punct::Slash => BinOp::Div,
            Punct::Percent => BinOp::Mod,
            Punct::StarStar => BinOp::Pow,
            Punct::Plus => BinOp::Add,
            Punct::Minus => BinOp::Sub,
            Punct::ShiftLeft => BinOp::Shl,
            Punct::ShiftRight => BinOp::Shr,
            Punct::UShiftRight => BinOp::UShr,
            Punct::Lt => BinOp::Lt,
            Punct::Le => BinOp::Le,
            Punct::Gt => BinOp::Gt,
            Punct::Ge => BinOp::Ge,
            Punct::EqEq => BinOp::Eq,
            Punct::Ne => BinOp::Ne,
            Punct::EqEqEq => BinOp::StrictEq,
            Punct::NeNe => BinOp::StrictNe,
            Punct::Amp => BinOp::BitAnd,
            Punct::Caret => BinOp::BitXor,
            Punct::Pipe => BinOp::BitOr,
            Punct::AmpAmp => BinOp::LogicalAnd,
            Punct::PipePipe => BinOp::LogicalOr,
            Punct::Nullish => BinOp::NullishCoalesce,
            // Unreachable: `prec_of` gives no precedence to any other punctuator.
            // Reaching it would be table drift, and a wrong tree in a test is a
            // better symptom than a panic in a fuzz run.
            _ => BinOp::Mul,
        },
        _ => BinOp::Mul,
    }
}

/// Convert the left-hand side of `for (x of y)` / `for (k in o)` — read as an
/// expression, because nothing distinguishes it from one until `of`/`in`
/// appears — into the pattern the statement needs. Purely structural: it builds
/// the same [`Pattern`] the declaration forms build, and refuses a shape that is
/// not a pattern (`for (a + b of c)`).
fn binding_from_expr(e: &Expr) -> P<Pattern> {
    let pos = e.pos();
    Ok(match e {
        Expr::Ident(name, p) => Pattern::Binding { name: name.clone(), pos: *p },
        Expr::ArrayLiteral { elements, pos } => {
            let mut out: Vec<Option<Pattern>> = Vec::new();
            let mut rest: Option<Box<Pattern>> = None;
            for (k, el) in elements.iter().enumerate() {
                match el {
                    None => out.push(None),
                    Some(Expr::Spread(inner, _)) => {
                        if k + 1 != elements.len() {
                            return Err(ParseError {
                                msg: "rest element must come last in an array pattern".into(),
                                line: inner.pos().line,
                                col: inner.pos().col,
                            });
                        }
                        rest = Some(Box::new(binding_from_expr(inner)?));
                    }
                    Some(other) => out.push(Some(binding_from_expr(other)?)),
                }
            }
            Pattern::Array { elements: out, rest, pos: *pos }
        }
        Expr::ObjectLiteral { props, pos } => {
            let mut out = Vec::new();
            for (key, value) in props {
                let shorthand = matches!(value, Expr::Ident(n, _) if n == key);
                out.push(ObjectProp { key: key.clone(), value: binding_from_expr(value)?, shorthand });
            }
            Pattern::Object { props: out, rest: None, pos: *pos }
        }
        Expr::Assign { op: AssignOp::Assign, target, value, pos } => Pattern::Default {
            left: Box::new(binding_from_expr(target)?),
            right: value.clone(),
            pos: *pos,
        },
        other => {
            return Err(ParseError {
                msg: format!("{} is not a binding pattern", describe_shape(other)),
                line: pos.line,
                col: pos.col,
            })
        }
    })
}

/// A short, stable name for an expression shape, for messages that must say what
/// was found without dumping a subtree.
fn describe_shape(e: &Expr) -> &'static str {
    match e {
        Expr::Num { .. } => "a number literal",
        Expr::Str(..) => "a string literal",
        Expr::Bool(..) | Expr::Null(_) | Expr::Undefined(_) => "a constant",
        Expr::Ident(..) => "an identifier",
        Expr::ArrayLiteral { .. } => "an array literal",
        Expr::ObjectLiteral { .. } => "an object literal",
        Expr::Function { .. } | Expr::Arrow { .. } => "a function expression",
        Expr::Call { .. } => "a call",
        Expr::New { .. } => "a `new` expression",
        Expr::Member { .. } | Expr::Index { .. } => "a property access",
        Expr::Binary { .. } => "a binary expression",
        Expr::Unary { .. } => "a unary expression",
        Expr::Update { .. } => "an update expression",
        Expr::Assign { .. } => "an assignment",
        Expr::Conditional { .. } => "a conditional expression",
        Expr::Template { .. } => "a template literal",
        Expr::Spread(..) => "a spread element",
        Expr::Yield(..) => "`yield`",
        Expr::Await(..) => "`await`",
        Expr::This(_) => "`this`",
        Expr::Super(_) => "`super`",
        Expr::Seq(..) => "a comma expression",
    }
}

/// Split the raw text of a template literal into parts.
///
/// The lexer keeps the span between the backticks verbatim, so the escapes and
/// the `${...}` boundaries are recovered here. An interpolation is parsed from
/// the substring, which is the only way to give §9.4's refusal the location of
/// the expression rather than of the whole literal — but the position reported
/// is the literal's start, because column offsets inside a raw span cannot be
/// reconstructed without the original line.
///
/// `outer_depth` is the outer parser's recursion counter, carried in and out —
/// named apart from the function's own `depth`, which counts braces inside one
/// interpolation. A nested template (`${`${`${x}`}`}`) re-enters the parser here,
/// and a re-entry that started a fresh counter would reset [`MAX_DEPTH`] at every
/// level: the guard would then bound the nesting *inside* one interpolation
/// rather than the nesting of interpolations, which is the one that overflows the
/// stack. Sharing the counter makes the ceiling total, so the process cannot abort
/// on a deeply nested template any more than it can on a deeply nested
/// parenthesization.
fn template_parts(raw: &str, pos: Position, outer_depth: &mut usize) -> P<Vec<TemplatePart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                // Escape sequences other than `\n` are passed through as literal
                // text: the parser must not invent decoded values it cannot model.
                if i + 1 < chars.len() {
                    match chars[i + 1] {
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        '`' => text.push('`'),
                        '$' => text.push('$'),
                        '\\' => text.push('\\'),
                        other => {
                            text.push('\\');
                            text.push(other);
                        }
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '$' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                // Find the matching brace, counting nesting so `${a ? b : {c}}`
                // does not end at the inner `}`.
                let mut depth = 1;
                let mut j = i + 2;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if depth != 0 {
                    return Err(ParseError {
                        msg: "unterminated `${` interpolation".into(),
                        line: pos.line,
                        col: pos.col,
                    });
                }
                let inner: String = chars[i + 2..j].iter().collect();
                // The re-entry is one nesting level deeper than the template that
                // contains it, so the outer parser's depth is handed in rather than
                // restarted at zero (§9.3's stack-boundedness argument above). On the
                // way out the count is restored: two sibling interpolations in one
                // literal (`${a}${b}`) are one level deep, not two.
                *outer_depth += 1;
                let r = if *outer_depth >= MAX_DEPTH {
                    Err(ParseError {
                        msg: "template interpolation nesting exceeds the parser's depth limit"
                            .into(),
                        line: pos.line,
                        col: pos.col,
                    })
                } else {
                    parse_at(&inner, *outer_depth)
                };
                *outer_depth -= 1;
                let e = r?.body.into_iter().next().and_then(|s| match s {
                    Stmt::Expr(e) => Some(e),
                    _ => None,
                });
                match e {
                    Some(e) => parts.push(TemplatePart::Expr(e)),
                    None => {
                        return Err(ParseError {
                            msg: "`${}` interpolation must contain a single expression".into(),
                            line: pos.line,
                            col: pos.col,
                        })
                    }
                }
                i = j + 1;
            }
            c => {
                text.push(c);
                i += 1;
            }
        }
    }
    if !text.is_empty() {
        parts.push(TemplatePart::Text(text));
    }
    Ok(parts)
}

/// Parse a source string into a [`Program`].
///
/// This is the frontend's whole §9.3 stage: no lowering, no refusal of
/// out-of-subset constructs, no recovery. A `Program` here means only that the
/// text is a syntactically valid JS file of the subset the grammar covers;
/// whether any of it may reach an analyzer is §9.4's question, asked in
/// [`crate::frontend::lower`].
///
/// The parse runs on a thread with [`PARSE_STACK`] bytes of stack rather than on
/// the caller's. That is not a performance decision and it is not cosmetic:
/// [`MAX_DEPTH`] bounds the *number of frames*, and whether that many frames fit
/// is decided by the stack the caller happens to have. A 2 MiB worker thread —
/// which is what libtest gives every `#[test]` — overflows at roughly 80 nested
/// statements, below the 64-level ceiling only if the ceiling is generous, and
/// either way the caller cannot tell "refused" from "aborted" because the abort
/// is a SIGSEGV the process does not return from. Providing the stack here makes
/// the ceiling and the stack a single decision owned by the parser, so no caller
/// and no test has to know the frame cost of a debug build to be safe.
pub fn parse(src: &str) -> P<Program> {
    let built = std::thread::Builder::new().stack_size(PARSE_STACK);
    // A scoped spawn so `src` needs no copy: the parse does not outlive this call.
    // A spawn failure is a resource failure, not a syntax error, and a panic
    // inside the parser is neither; both become an error rather than unwinding
    // through a caller that expects `is_err()` for bad input.
    let r = std::thread::scope(|s| built.spawn_scoped(s, || parse_at(src, 0)).map(|h| h.join()));
    match r {
        // Three layers: spawn, join, then the parser's own `P<Program>` — which is
        // what this arm returns unchanged, so a syntax error keeps its real
        // position instead of being re-wrapped at 1:1.
        Ok(Ok(prog)) => prog,
        // A panic inside the parser stays an error rather than an unwind through
        // a caller that reads `is_err()` as "bad input".
        Ok(Err(_panicked)) => internal_error("parser panicked"),
        // Could not spawn: a resource failure, not a syntax error.
        Err(_io) => internal_error("parser could not start: insufficient system resources"),
    }
}

/// The only errors this parser reports that are not about the input.
fn internal_error(msg: &str) -> P<Program> {
    Err(ParseError { msg: format!("internal error: {msg}"), line: 1, col: 1 })
}

/// [`parse`] without the stack arrangement, starting at recursion depth `depth`.
///
/// Re-entrant calls — a `${...}` interpolation, §9.3 — come here with the outer
/// parser's depth instead of zero, which is what makes the ceiling total across
/// re-entries. They run on the *caller's* stack, which is the stack `parse`
/// already sized: the interpolation's frames sit on top of the ones that paused
/// to parse it, so a fresh thread would not enlarge the room available, it would
/// only hide the nesting from [`MAX_DEPTH`].
fn parse_at(src: &str, depth: usize) -> P<Program> {
    let mut p = Parser::new(src);
    p.depth = depth;
    let prog = p.parse_program()?;
    // `parse_program` stops at Eof; anything left means the stream ran out early
    // mid-construct, which is only reachable if a rule forgot to consume its
    // terminator. Checked here so the mistake surfaces as a parse error.
    if !matches!(p.kind(), TokenKind::Eof) {
        return p.err("unexpected trailing input");
    }
    p.check_bad()?;
    Ok(prog)
}




