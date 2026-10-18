//! Phase 9 §9.4 — AST → IR lowering.
//!
//! The module that makes the analyzers reachable from source: a `.js` file in,
//! [`crate::ir::Loop`]s out, in the form `m0-decide` already accepts.
//!
//! ## The rule this module is built on
//!
//! [`crate::frontend`]'s contract is that the frontend lowers only what the IR can
//! express *without lying about it*, and refuses the rest by name (§9.14). Every
//! decision below comes from comparing a source construct against what [`Loop`]
//! can actually hold, not against what §9.4's table hopes for eventually:
//!
//! | IR capacity (read in `src/ir/mod.rs`) | Consequence here |
//! |---|---|
//! | `IV.lower/upper/step` are `i64` | a bound naming a program parameter (`j < n`) must resolve through [`Env`] or the nest is refused |
//! | `Access.index` is `Vec<i64>` | a stride naming a parameter (`A[k * ld + j]`) must become an integer coefficient or the nest is refused |
//! | `Guard` holds IV-vs-**constant** comparisons | `if (i < 32)` narrows the accesses below it; a condition the IR cannot name is *dropped*, and one it cannot analyze refuses the nest |
//! | `IV` has no bound *expression* | a triangular nest (`j <= i`) cannot be stated, and is refused — §9.14's boundary, not a parser gap |
//! | `Loop::validate` requires `step > 0` | a descending loop (`i--`) is refused rather than silently reversed |
//! | `Access.expr` sends a loop to M2g, which **discards** `index`/`offset` and re-derives them from the string | always `None`: this lowering has already produced affine coefficients, and a surface form would get them thrown away |
//! | `IV` is a counter | a `for-of` has none, so §9.7's rewriting synthesizes one (`v#of`) and takes the trip count from [`Env`] (`A.length`); an array whose length is not a parameter is refused |
//!
//! ## Two entry points, one rule
//!
//! [`lower_strict`] refuses at the first construct outside the subset — the shape
//! [`crate::frontend`] states ("refusal is a `Result`, never a degraded `Loop`"),
//! for a caller that must be sure nothing was skipped. [`lower_program`] lowers
//! every nest it can and reports the rest as [`Skipped`] with a position and a
//! reason, because §9.14's rule ("compiled correctly but not analyzed") is
//! *per construct*: `tests/js_kernels/mix.js` has an analyzable `for` and an
//! unanalyzable `for-in`, and both facts have to survive.
//!
//! ## Loops become nests, not blocks
//!
//! A chain of enclosing `for`s becomes one `Loop` whose `depth` is the chain
//! length, with the accesses of every level in that one `body`, exactly as the
//! hand-written corpora do — compare `tests/real_loops/r643.json`, whose depth-2
//! `A[i][j]` is written `index: [80, 1]`. A statement beside a nested loop
//! (syr2k's `C[i * ld + j] += t1 + t2;`, which *follows* the `k` loop) is charged
//! to the same nest: the locations it touches are unchanged, and in the nest's
//! lexicographic order it still follows the inner statements it follows in the
//! source, so the dependence relation is preserved rather than invented. Its
//! coefficient vector is padded with a zero for every deeper IV, which says the
//! truth — the access does not move as `k` moves — and can only *add* address
//! collisions, never remove one. Two *sibling* loops in one body cannot both be
//! charged this way: one `Loop` states one iteration space, so the second is
//! refused — enforced, not just intended, by the check in `for_stmt`/`for_of_stmt`
//! that compares a reopened level's IV name against the space already recorded.
//!
//! ## `for-of` is a counted loop in disguise
//!
//! §9.7: `for (const v of A) body` walks `A` by index, and the only thing the IR
//! can name about `v` is its *location*. So the binding is mapped to `A[i#of]`,
//! where `i#of` is a synthesized IV (`#` is not an identifier character, so the
//! name cannot collide with the source), and every read of `v` in the body is
//! recorded as a read of `A` at that index. The shape is not invented: it is what
//! the source executes, and it is the reason `tests/js_kernels/jaccard.js`'s
//! `for (const item of setA)` has a location for `item` at all.
//!
//! ## Why `scalars` and `stmts` stay empty
//!
//! [`crate::ir::stmt::ScalarExpr`] has no indexed-load variant (`stubs.md`, "Stub
//! 2: M5 rewire"), so the RHS of the statement form these kernels are made of —
//! `sum += A[i][j]` — is not representable. An `Assign` for it would either drop
//! the array read (an under-approximation, unsound) or invent a scalar the source
//! does not have. So the scalar layer is left to
//! [`crate::fud::cfg::build_cfg_from_loop`], which synthesizes IV initializers,
//! header guards, latch steps and one array statement per access from `body`. The
//! array dependences M0–M12 decide on are complete; the scalar FUD chains report
//! nothing they did not see.
//!
//! ## Storage association for a chained subscript
//!
//! `u[i][j]` loads a row and then an element, so its flat address needs the row
//! length. This lowering takes it to be the *innermost* IV's `upper` bound — the
//! square-matrix convention the corpora encode by hand (`r643.json`: `j` runs
//! `0..80`, and `A[i][j]` is `index: [80, 1]`) and the one
//! [`crate::ir::layout::Layout::RowMajor`] documents for JavaScript. When the
//! subscripts stay within a row the address is exact; when one leaves it, the
//! flattened address lands in the neighbouring row, so two references naming
//! distinct objects appear to share a location. That direction only ever *adds*
//! dependences, which is the conservative one. Dope vectors, which get the width
//! right without depending on the nest, are §9.8's job.

use crate::frontend::ast::{AssignOp, BinOp, Expr, ForHead, Pattern, Position, Program, Stmt, UnOp, UpdateOp};
use crate::frontend::parser::ParseError;
use crate::frontend::{parse, LowerError};
use crate::ir::{Access, AccessKind, Cmp, CmpOp, Guard, IV, IndexKind, Loop, OffsetValue};

use std::collections::HashMap;

/// The byte width of one JavaScript number: a double, which is the width every
/// corpus file uses for `elem_size` (`tests/m5_loops/*.json`,
/// `tests/real_loops/*.json`) and the positive value `Loop::validate` requires.
pub const ELEM_SIZE: i64 = 8;

/// `Loop::validate` rejects a depth above this, so a deeper nest is refused
/// rather than emitted into a loop the deciders will not accept.
const MAX_DEPTH: usize = 4;

/// The `cur` value meaning "outside every loop", where a program starts. An
/// access there has no IV to be affine in, so it cannot be lowered at all.
const NO_NEST: usize = usize::MAX;

/// The integer values of the program parameters a kernel's bounds and strides
/// name.
///
/// `Loop`'s bounds and coefficients are `i64`, so `j < n` and `A[k * ld + j]`
/// reach the IR only once `n` and `ld` are integers. This maps a name to one and
/// nothing more: a name that is absent turns the construct using it into a refusal
/// that says so, which is the difference between "this kernel needs its size" and
/// "this kernel cannot be analyzed".
///
/// The shape is deliberately the one `m1-decide` takes on the command line
/// (`KEY=VAL`) and the M1 decider resolves against, so a lowered nest can be
/// instantiated with parameters a caller already has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env(HashMap<String, i64>);

impl Env {
    /// An environment that resolves nothing: every named bound or stride becomes
    /// a refusal.
    pub fn empty() -> Env {
        Env(HashMap::new())
    }

    /// Build from name/value pairs, e.g. `Env::of(&[("n", 8), ("ld", 8)])`.
    pub fn of(pairs: &[(&str, i64)]) -> Env {
        Env(pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect())
    }

    /// The value bound to `name`, if this environment has one.
    pub fn resolve(&self, name: &str) -> Option<i64> {
        self.0.get(name).copied()
    }

    /// The names bound here, sorted, for diagnostics.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.0.keys().cloned().collect();
        v.sort();
        v
    }
}

/// A construct that parsed correctly and was then not lowered (§9.14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Where in the source. A refusal has to be locatable: the construct's name
    /// alone does not tell a reader which loop to fix.
    pub pos: Position,
    /// Why. The same type the strict entry point returns, so a caller can match
    /// on [`LowerError::OutOfScope`] in either mode.
    pub reason: LowerError,
}

/// What lowering a whole program produced: the IR, and what did not become IR.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kernels {
    /// One `Loop` per lowered nest, in source order.
    pub loops: Vec<Loop>,
    /// One entry per refused construct, in source order. Non-empty means the file
    /// is only partly analyzed — which §9.14 allows, and which a caller must not
    /// read as "the loops above are the whole program".
    pub skipped: Vec<Skipped>,
}

/// Parse and lower a source string, strictly.
pub fn lower(src: &str, env: &Env) -> Result<Vec<Loop>, LowerError> {
    let prog = parse(src).map_err(not_analyzable)?;
    lower_strict(&prog, env)
}

/// Lower an already-parsed program, refusing at the first construct outside the
/// analyzable subset.
pub fn lower_strict(prog: &Program, env: &Env) -> Result<Vec<Loop>, LowerError> {
    let k = run(prog, env, true);
    match k.skipped.first() {
        // `run` in strict mode records at most one refusal, then stops; the
        // reason is what the caller asked for.
        Some(s) => Err(s.reason.clone()),
        None => Ok(k.loops),
    }
}

/// Lower an already-parsed program leniently: every nest the IR can express is
/// returned, everything else is named in `skipped`.
pub fn lower_program(prog: &Program, env: &Env) -> Kernels {
    run(prog, env, false)
}

/// Parse and lower leniently. A parse error is still fatal: the source not being
/// the language is a different fact from being outside the subset.
pub fn lower_source(src: &str, env: &Env) -> Result<Kernels, LowerError> {
    let prog = parse(src).map_err(not_analyzable)?;
    Ok(lower_program(&prog, env))
}

fn run(prog: &Program, env: &Env, strict: bool) -> Kernels {
    let mut out = Kernels::default();
    let mut lowerer = Lowerer {
        env: env.clone(),
        out: &mut out,
        strict,
        halted: false,
        nests: Vec::new(),
        elements: Vec::new(),
    };
    let _ = lowerer.program(prog);
    out
}

/// A syntax error becomes `NotAnalyzable` rather than a new variant: `LowerError`
/// speaks about the analyzable subset, and the caller already holds the precise
/// diagnosis in [`ParseError`].
fn not_analyzable(e: ParseError) -> LowerError {
    LowerError::NotAnalyzable(format!("source did not parse: {e}"))
}

/// A nest under construction: the IV chain, and the accesses charged to it.
///
/// `ivs` is the chain *as currently open* — an inner loop appends its IV and
/// truncates it again when it closes, so a statement records the depth it
/// actually sits at. `space` is the deepest chain the nest ever reached, and that
/// is what the emitted `Loop` describes; a shallower statement is padded with
/// zeros for the IVs it does not mention. See the module docs for why charging a
/// statement to its enclosing chain rather than refusing it is the sound
/// direction.
struct Nest {
    ivs: Vec<IV>,
    space: Vec<IV>,
    body: Vec<Access>,
    /// Set once something inside the chain is refused. The chain is then not
    /// emitted at all: a nest with a statement missing from it is not the nest
    /// the source wrote, and shipping the remainder would report "no dependence"
    /// for whatever the dropped statement touches.
    poisoned: bool,
}

/// A refusal with its source position. [`LowerError`] has nowhere to put a
/// position, so refusals carry one internally and surface it through [`Skipped`].
#[derive(Debug, Clone)]
struct Refusal {
    pos: Position,
    reason: LowerError,
}

/// The lowering's result type.
type R<T> = Result<T, Refusal>;

impl Refusal {
    /// §9.14's out-of-scope list, naming the construct.
    fn scope(pos: Position, what: &'static str) -> Refusal {
        Refusal { pos, reason: LowerError::OutOfScope(what) }
    }

    /// In scope, but this shape is not expressible in [`Loop`].
    fn na(pos: Position, msg: impl Into<String>) -> Refusal {
        Refusal { pos, reason: LowerError::NotAnalyzable(msg.into()) }
    }
}

struct Lowerer<'a> {
    env: Env,
    out: &'a mut Kernels,
    /// `true` → the first refusal stops the walk; `false` → refusals are recorded
    /// and the walk continues (§9.14's rule is per construct, not per file).
    strict: bool,
    /// Set by [`Lowerer::record`] once a refusal has been fatal, so every level
    /// of the walk unwinds instead of lowering a program the caller has already
    /// been told is unanalyzable.
    halted: bool,
    nests: Vec<Nest>,
    /// The `for-of` element bindings currently in scope (§9.7). A read of the
    /// binding *is* a read of the array at the synthesized index — the element
    /// is not a value this lowering can name, so naming it is exactly what the
    /// `Access` says.
    elements: Vec<Element>,
}

/// One `for (const v of A)` binding: `v` stands for `A[i#of]`.
///
/// The index IV's name is `v#of`. `#` cannot appear in a JavaScript identifier,
/// so the synthesized name cannot collide with a name the source declares — and
/// because the element binding maps the *user's* name to this IV, `v` inside the
/// body never has to be distinguished from an IV that happens to be called `v`.
struct Element {
    /// The variable the source binds, e.g. `item`.
    var: String,
    /// The array it walks, e.g. `setA`.
    base: String,
    /// The synthesized index IV's name, e.g. `item#of`.
    iv: String,
}

impl<'a> Lowerer<'a> {
    fn program(&mut self, prog: &Program) -> R<()> {
        // Module code means something only its resolver can give it.
        if prog.is_module {
            let pos = prog
                .body
                .iter()
                .find(|s| matches!(s, Stmt::Import { .. } | Stmt::Export { .. }))
                .map(Stmt::pos)
                .unwrap_or(Position::new(1, 0));
            return Err(Refusal::scope(pos, "module syntax"));
        }
        self.block(&prog.body, NO_NEST, None);
        Ok(())
    }

    /// Walk a statement list living in nest `cur`.
    ///
    /// `guard` is the IV-vs-constant condition the list runs under, if an
    /// enclosing `if` supplied one; it is attached to every access found below.
    ///
    /// This is the boundary where a [`Refusal`] becomes a [`Skipped`]: the
    /// statement that raised it is abandoned, the open nest is poisoned, and the
    /// rest of the list still lowers (§9.14's rule is per construct). It returns
    /// nothing because there is nothing left to propagate; strict mode stops via
    /// the `halted` flag instead, which is why the walk is not a `Result`.
    fn block(&mut self, stmts: &[Stmt], cur: usize, guard: Option<&Guard>) {
        for s in stmts {
            if self.halted {
                return;
            }
            if let Err(r) = self.stmt(s, cur, guard) {
                self.record(r);
            }
        }
    }

    /// Walk the single statement or block that forms a loop's or `if`'s body.
    /// Absorbs refusals exactly as [`Lowerer::block`] does, which is what lets
    /// [`Lowerer::for_stmt`] truncate its IV unconditionally afterwards: a chain
    /// with a refused statement inside it is poisoned, never half-emitted.
    fn body(&mut self, s: &Stmt, cur: usize, guard: Option<&Guard>) {
        match s.as_block() {
            Some(v) => self.block(v, cur, guard),
            None => {
                if let Err(r) = self.stmt(s, cur, guard) {
                    self.record(r);
                }
            }
        }
    }

    /// Deliver a refusal: poison every open chain (a nest missing a statement is
    /// not the nest the source wrote), record it, and report whether the walk must
    /// stop — which happens only in strict mode, where [`lower_strict`] turns the
    /// single recorded refusal back into an `Err`.
    fn record(&mut self, r: Refusal) -> bool {
        for n in &mut self.nests {
            n.poisoned = true;
        }
        self.out.skipped.push(Skipped { pos: r.pos, reason: r.reason });
        if self.strict {
            self.halted = true;
            return true;
        }
        false
    }
}

impl<'a> Lowerer<'a> {
    /// One statement: the containers that hold more, and the leaves that hold
    /// accesses.
    fn stmt(&mut self, s: &Stmt, cur: usize, guard: Option<&Guard>) -> R<()> {
        match s {
            // The one form that opens or extends a nest.
            Stmt::For { head, body, pos } => self.for_stmt(*pos, head, body, cur, guard),

            // Containers: recurse with the nest unchanged. `block` absorbs its own
            // refusals, so there is nothing to propagate.
            Stmt::Block(v, _) => {
                self.block(v, cur, guard);
                Ok(())
            }

            // The unit these kernels are written in. Its loops are their own
            // iteration spaces, so the walk restarts at the top level; its
            // parameters are *not* refused, because a kernel's bounds reach the
            // IR through `Env` — that is what `n` in `j < n` is.
            Stmt::FunctionDecl { body, is_async, pos, .. } => {
                if *is_async {
                    return Err(Refusal::scope(*pos, "async function"));
                }
                self.block(body, NO_NEST, None);
                Ok(())
            }

            // Assignments and calls: where the accesses live.
            Stmt::Expr(e) => self.expr_stmt(e, cur, guard),

            // Declarations. `let t1 = 0` names a scalar, and the scalar layer is
            // the CFG builder's (module docs); a destructuring head is §9.6, and
            // an array read on the RHS is an access that is recorded.
            Stmt::Let { pattern, init, pos } | Stmt::Var { pattern, init, pos } => {
                self.decl(pattern, init.as_ref(), *pos, cur, guard)
            }
            Stmt::Const { pattern, init, pos } => {
                self.decl(pattern, Some(init), *pos, cur, guard)
            }

            Stmt::If { cond, then, els, pos } => {
                let mut own = self.if_guard(cond, cur, *pos)?;
                if let (Some(a), Some(b)) = (&mut own, guard) {
                    a.constraints.extend(b.constraints.iter().cloned());
                }
                let eff = own.as_ref().or(guard);
                // A condition the IR *can* name but does not constrain (a
                // variable on the right) yields `None` and is dropped: accesses
                // below are recorded as if unconditional. Assuming a guard might
                // always hold only ever *adds* dependences, so it cannot report
                // "no dependence" for one that exists — §9.14's sound direction.
                self.body(then, cur, eff);
                if let Some(e) = els {
                    // The else branch does not run under this branch's condition.
                    self.body(e, cur, guard);
                }
                Ok(())
            }

            Stmt::Return(_, _) | Stmt::Empty(_) => Ok(()),

            // Outside the analyzable subset (§9.14), each named as §9.14 names it.
            Stmt::ForOf { pattern, iter, body, is_async, pos } => {
                // §9.7's shape: an iterator is not an `IV`, so the walk is
                // rewritten as the indexed loop it denotes.
                self.for_of_stmt(*pos, pattern, iter, body, *is_async, cur, guard)
            }
            Stmt::ForIn { pos, .. } => Err(Refusal::scope(*pos, "for-in")),
            Stmt::While { pos, .. } => Err(Refusal::scope(*pos, "while")),
            Stmt::DoWhile { pos, .. } => Err(Refusal::scope(*pos, "do-while")),
            Stmt::Break(_, pos) | Stmt::Continue(_, pos) => {
                // An IV's trip count comes from its bound; a body that can leave
                // early has no bound the IV could state.
                Err(Refusal::scope(*pos, "break/continue"))
            }
            Stmt::Throw(_, pos) => Err(Refusal::scope(*pos, "throw")),
            Stmt::Try { pos, .. } => Err(Refusal::scope(*pos, "try/catch/finally")),
            Stmt::Switch { pos, .. } => Err(Refusal::scope(*pos, "switch")),
            Stmt::Class { pos, .. } => Err(Refusal::scope(*pos, "class")),
            Stmt::Import { pos, .. } => Err(Refusal::scope(*pos, "import")),
            Stmt::Export { pos, .. } => Err(Refusal::scope(*pos, "export")),
        }
    }

    /// The condition of an `if`, when it is an IV-vs-constant comparison the IR
    /// can state as an [`Guard`]. Conjunctions of them are collected, which is
    /// what `Access.guard` means ("the access executes only when ALL hold").
    ///
    /// Returns `Ok(None)` for a test the IR can name but not constrain with — a
    /// comparison against a variable — and `Err` for one it cannot name at all,
    /// because an unmodelled predicate inside a loop is where a silently-dropped
    /// guard would turn into a wrong `NoDependence`.
    fn if_guard(&mut self, cond: &Expr, cur: usize, pos: Position) -> R<Option<Guard>> {
        if cur == NO_NEST {
            // No nest to attach to; the condition cannot constrain anything.
            return Ok(None);
        }
        let mut constraints = Vec::new();
        self.collect_guard(cond, cur, pos, &mut constraints)
            .map(|named| if named { Some(Guard { constraints }) } else { None })
    }

    /// Fold `&&`-joined comparisons into `out`. `Ok(true)` means every leaf was
    /// an IV-vs-constant comparison; `Ok(false)` means some part of the test is
    /// nameable but not a constraint the guard can carry.
    fn collect_guard(
        &mut self,
        cond: &Expr,
        cur: usize,
        pos: Position,
        out: &mut Vec<Cmp>,
    ) -> R<bool> {
        match cond {
            Expr::Binary { op: BinOp::LogicalAnd, left, right, .. } => {
                let a = self.collect_guard(left, cur, pos, out)?;
                let b = self.collect_guard(right, cur, pos, out)?;
                return Ok(a && b);
            }
            Expr::Binary { op, left, right, .. } => {
                if let Some(cmp) = self.iv_cmp(*op, left, right, cur)
                    .or_else(|| self.iv_cmp(flip(*op), right, left, cur))
                {
                    out.push(cmp);
                    return Ok(true);
                }
                // Not a constraint the guard can carry: a comparison of an IV
                // against another IV or against a variable, or a numeric
                // expression used as a truthiness test (a coercion the IR has no
                // notion of). The *test* is dropped, but its array reads have to
                // be recorded: the condition executes every iteration whether or
                // not it holds, so an access inside it is a real access, and
                // leaving it out of the nest is the one mistake this module must
                // never make — a missing access becomes a wrong `NoDependence`.
                // Recorded unconditionally, which assumes it may always run.
                self.reads(left, cur, None)?;
                self.reads(right, cur, None)?;
                Ok(false)
            }
            // A bare identifier or literal as a predicate: nameable, unmodelled.
            Expr::Ident(..) | Expr::Bool(..) | Expr::Num { .. } => Ok(false),
            other => Err(self.unnamedable(other, pos)),
        }
    }

    /// `iv <const`, in either direction. `None` when either side is not one.
    fn iv_cmp(&self, op: BinOp, lhs: &Expr, rhs: &Expr, cur: usize) -> Option<Cmp> {
        let op = cmp_op(op)?;
        let name = lhs.as_ident()?;
        let iv = self.iv_position(cur, name)?;
        let bound = self.const_value(rhs)?;
        Some(Cmp { iv, op, bound })
    }

    /// A predicate the IR cannot even name — a call, a property read, a coercion.
    /// Inside a nest this refuses the nest rather than dropping the test: an
    /// unmodelled predicate is where a silently-dropped guard turns into a wrong
    /// `NoDependence`.
    fn unnamedable(&self, e: &Expr, pos: Position) -> Refusal {
        let what = match e {
            Expr::Call { .. } => "a call in a condition",
            Expr::Member { .. } => "a property access in a condition",
            Expr::New { .. } => "a constructor call in a condition",
            Expr::Unary { op: UnOp::TypeOf, .. } => "`typeof`",
            Expr::Binary { op: BinOp::In | BinOp::Instanceof, .. } => {
                "`in`/`instanceof`"
            }
            Expr::Template { .. } => "a template literal",
            Expr::Arrow { .. } | Expr::Function { .. } => "a function expression",
            Expr::Await(..) => "`await`",
            Expr::Yield(..) => "`yield`",
            _ => "a condition shape the IR cannot name",
        };
        Refusal::na(
            if e.pos().line == 0 { pos } else { e.pos() },
            format!("{what} is not a comparison the guard IR can state (§9.14)"),
        )
    }

    /// A literal, or a name this lowering can resolve through [`Env`].
    ///
    /// The `Env` arm is what lets `for (i = 0; i < n; i++)` state `upper: n` as an
    /// `i64`: the IR's bounds are integers, and a kernel's are parameters.
    fn const_value(&self, e: &Expr) -> Option<i64> {
        match e {
            Expr::Num { int, .. } => *int,
            Expr::Ident(name, _) => self.env.resolve(name),
            Expr::Unary { op: UnOp::Neg, arg, .. } => self.const_value(arg)?.checked_neg(),
            Expr::Unary { op: UnOp::Pos, arg, .. } => self.const_value(arg),
            _ => None,
        }
    }

    /// The position of `name` among the IVs of nest `cur`'s *open* chain.
    fn iv_position(&self, cur: usize, name: &str) -> Option<usize> {
        self.nests.get(cur)?.ivs.iter().position(|iv| iv.name == name)
    }

    /// Is `name` already an IV of the chain being extended? A redeclaration would
    /// put two `ivs` under one name, which the checker corpus treats as a bug
    /// shape (`tests/bug_tests/b017_dup_iv.json`).
    fn iv_redeclared(&self, cur: usize, name: &str) -> bool {
        self.iv_position(cur, name).is_some()
    }
}

/// `Lt`/`Le`/`Gt`/`Ge` read from the other side, so `n > i` states the bound of `i`.
fn flip(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

/// The comparison operators a `Cmp` can carry: `Guard`'s doc limits it to
/// constant-bounded `<`/`<=`/`>`/`>=`, so `==` and `!=` are not constraints.
fn cmp_op(op: BinOp) -> Option<CmpOp> {
    match op {
        BinOp::Lt => Some(CmpOp::Lt),
        BinOp::Le => Some(CmpOp::Le),
        BinOp::Gt => Some(CmpOp::Gt),
        BinOp::Ge => Some(CmpOp::Ge),
        _ => None,
    }
}

/// An affine index over the nest: coefficients by IV position, plus a constant.
///
/// Positions rather than names, because `Access::index` is positional
/// (`Vec<i64>` over `Loop::ivs`). Held sparse so an access recorded while the
/// chain was still short can be padded when the nest closes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Affine {
    coeffs: Vec<(usize, i64)>,
    k: i64,
}

impl Affine {
    fn iv(pos: usize) -> Affine {
        Affine { coeffs: vec![(pos, 1)], k: 0 }
    }

    fn konst(k: i64) -> Affine {
        Affine { coeffs: Vec::new(), k }
    }

    fn add(&self, o: &Affine) -> Option<Affine> {
        let mut coeffs = self.coeffs.clone();
        for (p, c) in &o.coeffs {
            match coeffs.iter_mut().find(|(q, _)| q == p) {
                Some(slot) => slot.1 = slot.1.checked_add(*c)?,
                None => coeffs.push((*p, *c)),
            }
        }
        coeffs.retain(|(_, c)| *c != 0);
        Some(Affine { coeffs, k: self.k.checked_add(o.k)? })
    }

    fn sub(&self, o: &Affine) -> Option<Affine> {
        let mut neg = Affine { coeffs: Vec::with_capacity(o.coeffs.len()), k: o.k.checked_neg()? };
        for (p, c) in &o.coeffs {
            neg.coeffs.push((*p, c.checked_neg()?));
        }
        self.add(&neg)
    }

    fn scale(&self, f: i64) -> Option<Affine> {
        let mut coeffs = Vec::with_capacity(self.coeffs.len());
        for (p, c) in &self.coeffs {
            coeffs.push((*p, c.checked_mul(f)?));
        }
        Some(Affine { coeffs, k: self.k.checked_mul(f)? })
    }

    /// Materialize against a nest of `depth` IVs. The coefficients go in
    /// `Access::index` and the constant term in `Access::offset`, which is where
    /// the corpora put it: `tests/loops/05_2d.json` writes `A[i][j + 99]` as
    /// `index: [100, 1]` with `offset: 99`.
    fn split(&self, depth: usize) -> (Vec<i64>, i64) {
        let mut v = vec![0i64; depth];
        for (p, c) in &self.coeffs {
            if *p < depth {
                v[*p] = *c;
            }
        }
        (v, self.k)
    }
}

impl<'a> Lowerer<'a> {
    /// `for (init; cond; update) body` — §9.4's counted-loop row.
    ///
    /// The head is read before anything is opened, so a loop the IR cannot
    /// describe never leaves a half-built chain behind.
    fn for_stmt(
        &mut self,
        pos: Position,
        head: &ForHead,
        body: &Stmt,
        cur: usize,
        guard: Option<&Guard>,
    ) -> R<()> {
        let ForHead::Classical { init, cond, update } = head;
        let iv = self.counted_iv(cur, init.as_deref(), cond.as_ref(), update.as_ref(), pos)?;

        if cur == NO_NEST {
            self.nests.push(Nest {
                ivs: vec![iv.clone()],
                space: vec![iv],
                body: Vec::new(),
                poisoned: false,
            });
            let idx = self.nests.len() - 1;
            self.body(body, idx, guard);
            let nest = self.nests.pop().expect("pushed just above");
            self.close_nest(pos, nest);
            return Ok(());
        }

        // Extending the chain this statement's nest owns. The IV is appended for
        // the duration of the body and truncated after, so a statement records
        // the depth it actually sits at.
        let depth_before = self.nests[cur].ivs.len();
        // The sibling rule the module docs state: once a chain has reached
        // `space`, a later loop opening at a level this chain has *already*
        // closed is a sibling, not a nesting. Charging it here would merge two
        // iteration spaces into one `Loop` — the shape `for (i) { for (j) A[..]
        // for (k) B[..] }` would be emitted as depth 2 with `k` playing `j`'s
        // role, which states a dependence between iterates that do not coexist.
        // Refused, not merged. A sibling that reopens the *same* IV name is the
        // same iteration space and stays lowerable.
        if depth_before < self.nests[cur].space.len()
            && self.nests[cur].space[depth_before].name != iv.name
        {
            return Err(Refusal::na(
                pos,
                format!(
                    "a sibling loop: this nest already states an iteration space over `{}` at this level, and one `Loop` cannot describe two (§9.14); the second loop is refused rather than merged",
                    self.nests[cur].space[depth_before].name
                ),
            ));
        }
        if depth_before + 1 > MAX_DEPTH {
            return Err(Refusal::na(
                pos,
                format!("a nest of depth {} is beyond the IR's maximum of {MAX_DEPTH}", depth_before + 1),
            ));
        }
        self.nests[cur].ivs.push(iv.clone());
        if self.nests[cur].ivs.len() > self.nests[cur].space.len() {
            self.nests[cur].space = self.nests[cur].ivs.clone();
        }
        self.body(body, cur, guard);
        self.nests[cur].ivs.truncate(depth_before);
        Ok(())
    }

    /// `for (const v of A) body` — §9.7's indexed-loop shape.
    ///
    /// The IR has no iterator, so the source form is lowered to the counted loop
    /// it denotes: the array is walked by an index, and a read of the binding `v`
    /// becomes a read of `A` at that index. Two facts have to be *available* for
    /// that to be stated rather than guessed, and each is a refusal when it is
    /// not:
    ///
    /// * the iterable is a plain name — a `for-of` over a call result has no
    ///   base this lowering can write in `Access::base`, and which array a name
    ///   holds is §9.7's points-to set, not a syntactic fact; and
    /// * `A.length` resolves through [`Env`] — `IV.upper` is an `i64`, so a
    ///   kernel's length is a parameter in exactly the way `n` in
    ///   `for (i = 0; i < n; i++)` is.
    ///
    /// A zero-length array is deliberately *not* refused: nothing in the body
    /// executes, so no `Access` exists to record and the walk is skipped.
    /// Diagnosing code that cannot execute would be a false refusal.
    fn for_of_stmt(
        &mut self,
        pos: Position,
        pattern: &Pattern,
        iter: &Expr,
        body: &Stmt,
        is_async: bool,
        cur: usize,
        guard: Option<&Guard>,
    ) -> R<()> {
        if is_async {
            return Err(Refusal::scope(pos, "for await"));
        }
        let Pattern::Binding { name, .. } = pattern else {
            // §9.6: `for (const [a, b] of A)` destructures the element, which is
            // a sequence of loads, and the module that emits it does not exist.
            return Err(Refusal::scope(pattern.pos(), "destructuring"));
        };
        let Expr::Ident(arr, _) = iter else {
            return Err(Refusal::na(
                iter.pos(),
                "a for-of whose iterable is not a plain array name: the element's location is not statically known (§9.7's points-to set decides which array a name holds)",
            ));
        };
        let Some(len) = self.env.resolve(&format!("{arr}.length")) else {
            return Err(Refusal::na(
                pos,
                format!(
                    "`{arr}.length` is not in the environment, so this for-of's trip count is not a fact the IR can state (§9.7): `IV.upper` is an `i64`"
                ),
            ));
        };
        if len <= 0 {
            return Ok(());
        }
        // `#` is not a JavaScript identifier character, so this name cannot
        // collide with anything the source declares.
        let iv = IV::new(format!("{name}#of"), 0, len, 1);
        if self.iv_redeclared(cur, &iv.name) {
            return Err(Refusal::na(
                pos,
                format!("IV `{}` redeclared inside a nest that already has it", iv.name),
            ));
        }
        let binding = Element { var: name.clone(), base: arr.clone(), iv: iv.name.clone() };

        if cur == NO_NEST {
            self.nests.push(Nest {
                ivs: vec![iv.clone()],
                space: vec![iv],
                body: Vec::new(),
                poisoned: false,
            });
            let idx = self.nests.len() - 1;
            self.elements.push(binding);
            self.body(body, idx, guard);
            self.elements.pop();
            let nest = self.nests.pop().expect("pushed just above");
            self.close_nest(pos, nest);
            return Ok(());
        }

        let depth_before = self.nests[cur].ivs.len();
        // The sibling rule [`Lowerer::for_stmt`] applies, for the same reason.
        if depth_before < self.nests[cur].space.len()
            && self.nests[cur].space[depth_before].name != iv.name
        {
            let existing = self.nests[cur].space[depth_before].name.clone();
            return Err(Refusal::na(
                pos,
                format!(
                    "a sibling loop: this nest already states an iteration space over `{existing}` at this level, and one `Loop` cannot describe two (§9.14)"
                ),
            ));
        }
        if depth_before + 1 > MAX_DEPTH {
            return Err(Refusal::na(
                pos,
                format!("a nest of depth {} is beyond the IR's maximum of {MAX_DEPTH}", depth_before + 1),
            ));
        }
        self.nests[cur].ivs.push(iv.clone());
        if self.nests[cur].ivs.len() > self.nests[cur].space.len() {
            self.nests[cur].space = self.nests[cur].ivs.clone();
        }
        self.elements.push(binding);
        self.body(body, cur, guard);
        self.elements.pop();
        self.nests[cur].ivs.truncate(depth_before);
        Ok(())
    }
    fn close_nest(&mut self, pos: Position, mut nest: Nest) {
        if nest.poisoned {
            return; // the refusal naming it is already recorded
        }
        if nest.body.is_empty() {
            // Nothing for M0–M12 to decide on. Not a refusal: the nest was
            // analyzable, it just does not touch an array.
            return;
        }
        let depth = nest.space.len();
        for a in &mut nest.body {
            // Pad to the space's depth: a statement from a shallower level does
            // not move with the deeper IVs, which is exactly what a zero says.
            a.index.resize(depth, 0);
        }
        // One numbering per file: the corpora list writes before reads, and a
        // certificate names accesses by position.
        nest.body.sort_by(|a, b| {
            a.base
                .cmp(&b.base)
                .then_with(|| (a.kind as i32).cmp(&(b.kind as i32)))
                .then_with(|| a.index.cmp(&b.index))
                .then_with(|| a.offset.to_string().cmp(&b.offset.to_string()))
        });
        let loop_ = Loop {
            depth,
            ivs: std::mem::take(&mut nest.space),
            body: std::mem::take(&mut nest.body),
            recursion: None,
            chase: None,
            assertions: Vec::new(),
            scalars: Vec::new(),
            stmts: None,
        };
        // The IR's invariant, re-checked rather than trusted: `validate`'s rules
        // are the deciders' assumptions, so a loop that fails one must not leave
        // the frontend.
        if let Err(e) = loop_.validate() {
            self.record(Refusal::na(pos, format!("lowered loop rejected by `Loop::validate`: {e}")));
            return;
        }
        self.out.loops.push(loop_);
    }


    /// The one loop head this frontend can state as an [`IV`]: a name, a lower
    /// bound, an upper bound and a positive constant step - each either a literal
    /// or a name [`Env`] resolves.
    fn counted_iv(
        &mut self,
        cur: usize,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        update: Option<&Expr>,
        pos: Position,
    ) -> R<IV> {
        let Some(init) = init else {
            return Err(Refusal::na(pos, "loop has no initializer, so its IV has no lower bound"));
        };
        let (name, lower) = self.initializer(init, pos)?;
        if self.iv_redeclared(cur, &name) {
            return Err(Refusal::na(
                pos,
                format!("IV `{name}` redeclared inside a nest that already has it"),
            ));
        }
        let Some(cond) = cond else {
            return Err(Refusal::na(
                pos,
                format!("IV `{name}` has no condition, so its upper bound is not a fact of the loop"),
            ));
        };
        let upper = self.upper_bound(&name, cond, cur, pos)?;
        let Some(update) = update else {
            return Err(Refusal::na(
                pos,
                format!("IV `{name}` is never stepped, so the loop has no trip count"),
            ));
        };
        let step = self.step(cur, &name, update, pos)?;
        Ok(IV::new(name, lower, upper, step))
    }

    /// `let i = 0` (also `const`/`var`) - the IV's name and its lower bound.
    fn initializer(&mut self, init: &Stmt, pos: Position) -> R<(String, i64)> {
        let (pattern, value) = match init {
            Stmt::Let { pattern, init: Some(e), .. }
            | Stmt::Const { pattern, init: e, .. }
            | Stmt::Var { pattern, init: Some(e), .. } => (pattern, e),
            Stmt::Block(stmts, bpos) => {
                // `for (let i = 0, j = 0; ...)`: one bound per IV exists in the IR
                // only once both IVs are bounded, which is 9.10's
                // canonicalization, not this row of 9.4's table.
                return match stmts.as_slice() {
                    [only] => self.initializer(only, pos),
                    _ => Err(Refusal::na(*bpos, "a loop initializer declaring more than one variable")),
                };
            }
            Stmt::Expr(e) => {
                // `for (i = 0; ...)`: the value is there, the declaration is not.
                // Whether `i` is this loop's IV or an outer variable is a scope
                // question (9.5's capture analysis), and guessing it is how a
                // frontend invents an IV.
                return Err(Refusal::na(e.pos(), "a bare assignment initializer declares no IV"));
            }
            _ => {
                return Err(Refusal::na(
                    pos,
                    "loop initializer is not a let/const/var of one initialized variable",
                ))
            }
        };
        let Pattern::Binding { name, .. } = pattern else {
            // 9.6: a destructuring head becomes a sequence of loads, and the
            // module that does that does not exist yet.
            return Err(Refusal::scope(pattern.pos(), "destructuring"));
        };
        let lower = self.const_value(value).ok_or_else(|| {
            Refusal::na(
                value.pos(),
                format!(
                    "IV `{name}` starts at a value that is neither a literal nor a name in the parameter environment (which holds {:?})",
                    self.env.names()
                ),
            )
        })?;
        Ok((name.clone(), lower))
    }

    /// The upper bound, from an `i < n` / `i <= n - 1` shaped condition.
    ///
    /// `IV.upper` is exclusive (`validate` rejects `lower >= upper`, and the
    /// corpora write `for (i=0; i<N; i++)` as `upper: N`), so an inclusive
    /// comparison becomes `bound + 1`.
    fn upper_bound(&mut self, name: &str, cond: &Expr, cur: usize, pos: Position) -> R<i64> {
        let Expr::Binary { op, left, right, .. } = cond else {
            return Err(Refusal::na(
                cond.pos(),
                "a loop condition that is not a comparison gives the IV no upper bound",
            ));
        };
        let (iv_side, other_side, strict) = match op {
            BinOp::Lt | BinOp::Le => (&**left, &**right, matches!(op, BinOp::Lt)),
            BinOp::Gt | BinOp::Ge => (&**right, &**left, matches!(op, BinOp::Gt)),
            other => {
                return Err(Refusal::na(
                    cond.pos(),
                    format!(
                        "a loop condition using `{}` is not a bound the IV can state",
                        binop_name(*other)
                    ),
                ))
            }
        };
        let _ = pos;
        let Some(iv_name) = iv_side.as_ident() else {
            return Err(Refusal::na(
                iv_side.pos(),
                "the side of the bound that should hold the IV is not a single name",
            ));
        };
        if iv_name != name {
            // `for (let i = 0; j < n; i++)`: the trip count is not a function of
            // this loop's IV.
            return Err(Refusal::na(
                cond.pos(),
                format!("loop on `{name}` is bounded by `{iv_name}`"),
            ));
        }
        let term = self.affine(other_side, cur, cond.pos())?;
        if !term.coeffs.is_empty() {
            // `j <= i`. This is the nonrectangular region whose extrema
            // `solver::extreme::extreme_nonrectangular` computes exactly - but
            // `IV.upper` is an `i64` and `Guard`'s `Cmp` compares an IV against a
            // constant, so there is nowhere in the IR to state it. Reporting the
            // triangle as its bounding rectangle would claim locations the source
            // never touches, which is the one direction 9.14 forbids: a dependence
            // "found" there is about an iteration that does not exist. So: refuse.
            let parts: Vec<String> = term
                .coeffs
                .iter()
                .map(|(p, c)| format!("{c} * iv[{p}]"))
                .collect();
            return Err(Refusal::na(
                cond.pos(),
                format!(
                    "the bound of IV `{iv_name}` depends on another IV ({}): `IV.upper` is an `i64` and `Guard` holds only IV-vs-constant comparisons, so a nonrectangular region cannot be stated in the IR (§9.14)",
                    parts.join(" + ")
                ),
            ));
        }
        if strict {
            Ok(term.k)
        } else {
            term.k.checked_add(1).ok_or_else(|| {
                Refusal::na(cond.pos(), "an inclusive bound at i64::MAX has no exclusive form")
            })
        }
    }

    /// `i++`, `++i`, `i += 2`, `i = i + 2` - the positive constant step `IV.step`
    /// needs. A decrement is refused rather than reversed: `Loop::validate`
    /// requires `step > 0`, and flipping the direction would change which
    /// iterations the source's bounds select.
    fn step(&mut self, cur: usize, name: &str, update: &Expr, pos: Position) -> R<i64> {
        let _ = pos;
        let delta = match update {
            Expr::Update { op: UpdateOp::Inc, operand, .. } => {
                self.step_target(name, operand)?;
                1
            }
            Expr::Update { op: UpdateOp::Dec, operand, .. } => {
                self.step_target(name, operand)?;
                return Err(Refusal::na(
                    update.pos(),
                    format!("IV `{name}` steps down (`--`), which `Loop::validate` cannot state: it requires `step > 0`"),
                ));
            }
            Expr::Assign { op: AssignOp::Add, target, value, .. } => {
                self.step_target(name, target)?;
                let d = self.affine(value, cur, update.pos())?;
                if !d.coeffs.is_empty() || d.k <= 0 {
                    return Err(Refusal::na(
                        value.pos(),
                        format!("IV `{name}`'s step is not a positive constant"),
                    ));
                }
                d.k
            }
            Expr::Assign { op: AssignOp::Assign, target, value, .. } => {
                self.step_target(name, target)?;
                // `i = i + 2`, which states the same step.
                let t = self.affine(value, cur, update.pos())?;
                let iv_pos = self.iv_position(cur, name).ok_or_else(|| {
                    Refusal::na(update.pos(), format!("IV `{name}` is not open in the nest being stepped"))
                })?;
                let self_coeff = t.coeffs.iter().find(|(p, _)| *p == iv_pos).map(|(_, c)| *c);
                let others: Vec<(usize, i64)> =
                    t.coeffs.iter().cloned().filter(|(p, _)| *p != iv_pos).collect();
                match self_coeff {
                    Some(1) if others.is_empty() => t.k,
                    _ => {
                        return Err(Refusal::na(
                            value.pos(),
                            format!("IV `{name}`'s update is not `{name} + <constant>`"),
                        ))
                    }
                }
            }
            other => {
                return Err(Refusal::na(
                    other.pos(),
                    "the loop's third clause is neither an increment nor an additive assignment",
                ))
            }
        };
        if delta <= 0 {
            return Err(Refusal::na(
                update.pos(),
                format!("IV `{name}` steps by {delta}, and `Loop::validate` requires a positive step"),
            ));
        }
        Ok(delta)
    }

    fn step_target(&self, name: &str, target: &Expr) -> R<()> {
        match target.as_ident() {
            Some(n) if n == name => Ok(()),
            Some(n) => Err(Refusal::na(
                target.pos(),
                format!("the loop steps `{n}`, not its IV `{name}`"),
            )),
            None => Err(Refusal::na(
                target.pos(),
                format!("the loop's step target is not a plain name, so it cannot be IV `{name}`'s"),
            )),
        }
    }

    /// Classify an expression as an [`Affine`] term over the open nest.
    ///
    /// This is the whole of §9.4's index column: an access is analyzable only if
    /// its subscript is affine in the IVs, because that is what `Access::index`
    /// (a `Vec<i64>`) and `OffsetValue::Const` can hold.
    ///
    /// Refusals are precise about which arithmetic JS performs and the IR cannot
    /// follow: `/` on floats (`Math.floor` is what a program means by it), a
    /// product of two IVs (`IndexKind::Product` exists for it, but the M6 corpora
    /// were written with it, not lowered to it), and any value that comes out of
    /// an object, a call or an array element rather than out of an IV.
    fn affine(&self, e: &Expr, cur: usize, why: Position) -> R<Affine> {
        match e {
            Expr::Num { int, .. } => match int {
                Some(v) => Ok(Affine::konst(*v)),
                None => Err(Refusal::na(
                    e.pos(),
                    "an index is a fractional literal, which cannot be an element subscript",
                )),
            },
            Expr::Ident(name, _) => {
                if let Some(p) = self.iv_position(cur, name) {
                    return Ok(Affine::iv(p));
                }
                if let Some(v) = self.env.resolve(name) {
                    return Ok(Affine::konst(v));
                }
                Err(Refusal::na(
                    e.pos(),
                    format!(
                        "an index reads `{name}`, which is neither an IV of the enclosing nest nor a parameter in the environment ({}) — §9.14's array indexing is affine in the IVs",
                        self.site(why)
                    ),
                ))
            }
            Expr::Unary { op: UnOp::Neg, arg, .. } => {
                Ok(self.affine(arg, cur, why)?.scale(-1).ok_or_else(|| self.overflow(e.pos()))?)
            }
            Expr::Unary { op: UnOp::Pos, arg, .. } => self.affine(arg, cur, why),
            Expr::Binary { op, left, right, .. } => {
                let a = self.affine(left, cur, why)?;
                let b = self.affine(right, cur, why)?;
                match op {
                    BinOp::Add => a.add(&b).ok_or_else(|| self.overflow(e.pos())),
                    BinOp::Sub => a.sub(&b).ok_or_else(|| self.overflow(e.pos())),
                    BinOp::Mul => {
                        // Linear times a constant stays affine. A product of two
                        // IV terms is M6's `IndexKind::Product`, which no corpus
                        // here lowers to, and `Access::index` cannot hold.
                        if a.coeffs.is_empty() {
                            b.scale(a.k).ok_or_else(|| self.overflow(e.pos()))
                        } else if b.coeffs.is_empty() {
                            a.scale(b.k).ok_or_else(|| self.overflow(e.pos()))
                        } else {
                            Err(Refusal::na(
                                e.pos(),
                                "an index is a product of two IV-bearing terms (nonlinear); `Access::index` holds one coefficient per IV, so this nest is not expressible without `IndexKind::Product` (M6), which lowering to it is not (§9.14)",
                            ))
                        }
                    }
                    BinOp::Div => {
                        // JS `/` is float division. `Access::offset` is an integer,
                        // and `param_expr::ParamExpr::Div` refuses to evaluate when
                        // the dividend is not exactly divisible, which is the right
                        // answer for a subscript and not this one.
                        Err(Refusal::na(
                            e.pos(),
                            "an index uses `/`, which in JavaScript is float division; the IR's subscript is an integer, and `Math.floor` (which the source's address actually needs) is not modelled (§9.14)",
                        ))
                    }
                    other => Err(Refusal::na(
                        e.pos(),
                        format!(
                            "an index uses `{}`, which the affine index form does not hold ({})",
                            binop_name(*other),
                            self.site(why)
                        ),
                    )),
                }
            }
            other => Err(Refusal::na(
                other.pos(),
                format!(
                    "an index is not an arithmetic expression over IVs ({}) — {}",
                    shape_name(other),
                    self.site(why)
                ),
            )),
        }
    }

    /// Where a refusal happened, in words that name the construct rather than the
    /// frame, because the position is the caller's only handle on the source.
    fn site(&self, pos: Position) -> String {
        format!("at {pos}")
    }

    fn overflow(&self, pos: Position) -> Refusal {
        Refusal::na(pos, "an index expression overflows i64, so its coefficient is not computable")
    }

}

/// The operator as it appears in source, for a refusal that names it.
fn binop_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::UShr => ">>>",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::StrictEq => "===",
        BinOp::StrictNe => "!==",
        BinOp::LogicalAnd => "&&",
        BinOp::LogicalOr => "||",
        BinOp::NullishCoalesce => "??",
        BinOp::Instanceof => "instanceof",
        BinOp::In => "in",
    }
}

/// The node's surface kind, for a refusal that has to say what it found.
fn shape_name(e: &Expr) -> &'static str {
    match e {
        Expr::Member { .. } => "a property access (§9.8's shapes)",
        Expr::Index { .. } => "a chained subscript",
        Expr::Call { .. } => "a call (§9.5's effects)",
        Expr::New { .. } => "a constructor call",
        Expr::Conditional { .. } => "a conditional",
        Expr::Assign { .. } => "an assignment",
        Expr::Update { .. } => "an increment",
        Expr::ArrayLiteral { .. } => "an array literal",
        Expr::ObjectLiteral { .. } => "an object literal",
        Expr::Arrow { .. } | Expr::Function { .. } => "a function expression",
        Expr::Template { .. } => "a template literal",
        Expr::Await(..) => "`await`",
        Expr::Yield(..) => "`yield`",
        Expr::Spread(..) => "a spread",
        Expr::Seq(..) => "the comma operator",
        Expr::Unary { .. } => "a unary operation",
        Expr::Str(..) => "a string",
        Expr::Bool(..) => "a boolean",
        Expr::Null(..) | Expr::Undefined(..) => "a nullish value",
        Expr::This(..) => "`this`",
        Expr::Super(..) => "`super`",
        Expr::Binary { .. } | Expr::Num { .. } | Expr::Ident(..) => "an expression",
    }
}

impl<'a> Lowerer<'a> {
    /// A statement whose value is an expression: where the loads and stores are.
    ///
    /// §9.4's table gives the two rows that matter here: `A[i] = x` is a store
    /// with `x`'s value in it, and `A[i]` alone is a load. The value half is not
    /// representable (module docs: no indexed-load `ScalarExpr`), so a statement
    /// is lowered for its accesses and its RHS is walked for more of them.
    fn expr_stmt(&mut self, e: &Expr, cur: usize, guard: Option<&Guard>) -> R<()> {
        match e {
            Expr::Assign { op, target, value, .. } => {
                // `a[i] = x` / `a[i] += x`: the target is a write, the value may
                // read more arrays, and a compound assignment reads the target
                // too. A target that is not an array reference (`t1 += ...`) names
                // a scalar, and the scalar layer belongs to `build_cfg_from_loop`
                // (module docs) — recording it as a store would invent an array
                // called `t1`. The RHS is still walked, because that is where the
                // reads are.
                if self.is_array_ref(target) {
                    self.store(target, cur, guard, op.to_binop().is_some())?;
                }
                self.reads(value, cur, guard)
            }
            Expr::Update { op: _, operand, .. } => {
                // `i++` on an array element is rare; on a scalar it is IR this
                // module leaves to the CFG builder.
                if self.is_array_ref(operand) {
                    self.store(operand, cur, guard, true)?;
                }
                Ok(())
            }
            Expr::Call { callee, args, .. } => {
                // A call is §9.4's `Call(f, [x, y])` row. The IR here has no call
                // statement, but the arguments can read arrays, and those reads
                // are real dependences. The call itself is refused: `push` on an
                // array in a loop grows it, which no `Access` of this nest's
                // length can describe.
                self.reads_all(args, cur, guard)?;
                let what = match &**callee {
                    Expr::Member { .. } => {
                        "a method call in a loop body (§9.14: `Array.prototype.push` grows the array, which no access in a fixed nest can state)"
                    }
                    _ => "a call in a loop body (§9.4's `Call(f, [x, y])` row has no IR node yet)",
                };
                Err(Refusal::scope(e.pos(), what))
            }
            other => {
                // A bare expression statement that is not an assignment: evaluate
                // for reads, which is the only IR it can produce.
                if self.is_array_ref(other) {
                    self.load(other, AccessKind::Read, cur, guard)?;
                }
                Ok(())
            }
        }
    }

    /// A declaration. `let t1 = 0` is a scalar, and the scalar layer belongs to
    /// `build_cfg_from_loop` (module docs); `const x = A[i][j]` reads an array and
    /// that read is an access the nest must carry.
    fn decl(
        &mut self,
        pattern: &Pattern,
        init: Option<&Expr>,
        pos: Position,
        cur: usize,
        guard: Option<&Guard>,
    ) -> R<()> {
        if !matches!(pattern, Pattern::Binding { .. }) {
            // §9.6: destructuring lowers to a *sequence* of loads, and the module
            // that does it does not exist yet.
            return Err(Refusal::scope(pattern.pos(), "destructuring"));
        }
        let _ = pos;
        if let Some(e) = init {
            self.reads(e, cur, guard)?;
        }
        Ok(())
    }

    /// Walk an expression for the array reads inside it, without lowering the
    /// expression itself. A read of `A[k * ld + j]` buried in `alpha * A[..] * B[..]`
    /// is a dependence the nest has to report, so it is recorded as its own
    /// access; the arithmetic around it is not IR and is not pretended to be.
    fn reads(&mut self, e: &Expr, cur: usize, guard: Option<&Guard>) -> R<()> {
        match e {
            Expr::Index { .. } | Expr::Member { .. } if self.is_array_ref(e) => {
                self.load(e, AccessKind::Read, cur, guard)
            }
            Expr::Binary { left, right, .. } => {
                self.reads(left, cur, guard)?;
                self.reads(right, cur, guard)
            }
            Expr::Unary { arg, .. } | Expr::Spread(arg, _) | Expr::Await(arg, _) => {
                self.reads(arg, cur, guard)
            }
            Expr::Assign { target, value, .. } => {
                self.reads(target, cur, guard)?;
                self.reads(value, cur, guard)
            }
            Expr::Update { operand, .. } => self.reads(operand, cur, guard),
            Expr::Conditional { cond, then, els, .. } => {
                self.reads(cond, cur, guard)?;
                self.reads(then, cur, guard)?;
                self.reads(els, cur, guard)
            }
            Expr::Call { callee, args, .. } => {
                self.reads(callee, cur, guard)?;
                self.reads_all(args, cur, guard)?;
                Err(Refusal::scope(e.pos(), "a call in a loop body"))
            }
            // A `for-of` binding read: the element *is* the array at the index
            // the synthesized IV walks (§9.7), so reading the binding is reading
            // that element. Recorded rather than dropped, because a missing
            // access is the one mistake this module must never make.
            Expr::Ident(name, _) => {
                self.element_read(name, cur, guard);
                Ok(())
            }
            // Literals and anything else that reads no array. A function
            // expression or a template interpolation would hide statements, and
            // those are §9.14's, so they are named rather than walked past.
            Expr::Arrow { .. } | Expr::Function { .. } => {
                Err(Refusal::scope(e.pos(), "a function expression in a loop body"))
            }
            Expr::Template { .. } => Err(Refusal::scope(e.pos(), "a template literal")),
            Expr::ArrayLiteral { elements, .. } => {
                for el in elements.iter().flatten() {
                    self.reads(el, cur, guard)?;
                }
                Ok(())
            }
            Expr::ObjectLiteral { props, .. } => {
                for (_, v) in props {
                    self.reads(v, cur, guard)?;
                }
                Ok(())
            }
            Expr::Seq(v, _) => {
                for x in v {
                    self.reads(x, cur, guard)?;
                }
                Ok(())
            }
            Expr::New { args, .. } => {
                self.reads_all(args, cur, guard)?;
                Err(Refusal::scope(e.pos(), "new"))
            }
            _ => Ok(()),
        }
    }

    fn reads_all(&mut self, args: &[Expr], cur: usize, guard: Option<&Guard>) -> R<()> {
        for a in args {
            self.reads(a, cur, guard)?;
        }
        Ok(())
    }

    /// Record a read of a `for-of` element binding, if `name` is one in scope.
    ///
    /// Contributes nothing when it is not: a name that is not an element (a
    /// scalar, an array, a parameter) reads no element, and an element read
    /// outside any nest has no IV to be indexed by — its loop was refused, and
    /// `Access` addresses an element, not an array.
    fn element_read(&mut self, name: &str, cur: usize, guard: Option<&Guard>) {
        if cur == NO_NEST {
            return;
        }
        // Innermost binding wins, mirroring the scope a nested `for-of` creates.
        let Some(binding) = self.elements.iter().rev().find(|b| b.var == name) else {
            return;
        };
        let base = binding.base.clone();
        let iv_name = binding.iv.clone();
        let Some(p) = self.nests[cur].ivs.iter().position(|iv| iv.name == iv_name) else {
            return;
        };
        let mut index = vec![0i64; self.nests[cur].ivs.len()];
        index[p] = 1;
        self.nests[cur].body.push(Access {
            kind: AccessKind::Read,
            base,
            index,
            elem_size: ELEM_SIZE,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: guard.cloned(),
            range: None,
            index_kind: IndexKind::Affine,
        });
    }

    /// Is this expression an array *element* — `A[expr]` or `A[i][j]`, rooted at a
    /// plain identifier?
    ///
    /// Two rejections matter here. Rooted at `this` or at a call, it is an object
    /// access (§9.8), which the IR's `Access` cannot name. And a *bare* identifier
    /// is not an element at all: `t1 += ...` assigns to a scalar, and treating its
    /// target as an access would invent an array named `t1` — and then refuse the
    /// nest for a subscript it does not have. Whether a bare name holds an array is
    /// a points-to question (§9.7), not a syntactic one.
    fn is_array_ref(&self, e: &Expr) -> bool {
        match e {
            Expr::Index { obj, .. } => self.is_array_root(obj),
            _ => false,
        }
    }

    /// The base of a subscript chain: an identifier, or another subscripted access.
    fn is_array_root(&self, e: &Expr) -> bool {
        match e {
            Expr::Index { obj, .. } => self.is_array_root(obj),
            Expr::Ident(..) => true,
            // `A.length`/`obj.field` are properties, whose offset §9.8's shape
            // inference decides. Until then they are not elements of a named array.
            Expr::Member { .. } => false,
            _ => false,
        }
    }
}

impl<'a> Lowerer<'a> {
    /// Record a load of `e` as an [`Access`].
    fn load(&mut self, e: &Expr, kind: AccessKind, cur: usize, guard: Option<&Guard>) -> R<()> {
        self.emit_access(e, kind, cur, guard)
    }

    /// Record a store. §9.4's `A[i] = x` row: the target of the assignment is the
    /// write, and it is the only part of the statement the IR can hold.
    fn store(&mut self, target: &Expr, cur: usize, guard: Option<&Guard>, also_read: bool) -> R<()> {
        self.emit_access(target, AccessKind::Write, cur, guard)?;
        if also_read {
            // A compound assignment reads the location it writes. M0 keys on that
            // pair, so it has to be there.
            self.emit_access(target, AccessKind::Read, cur, guard)?;
        }
        Ok(())
    }

    /// Lower one array reference into an `Access` and append it to the nest.
    ///
    /// The subscript is what §9.4's table calls "affine in the IVs". Anything
    /// else — a value read out of another array, a property, a call — is refused
    /// by name, because `Access::index` cannot hold it and `Access::expr` would
    /// hand the access to M2g, which discards the coefficients computed here.
    fn emit_access(&mut self, e: &Expr, kind: AccessKind, cur: usize, guard: Option<&Guard>) -> R<()> {
        if cur == NO_NEST {
            return Err(Refusal::na(
                e.pos(),
                "an array access outside any loop has no IV to be indexed by, so it is not a `Loop` body element",
            ));
        }
        let Some((base, subs)) = self.reference(e) else {
            return Err(self.not_an_array(e));
        };
        let depth = self.nests[cur].ivs.len();
        let (index, constant) = match subs.as_slice() {
            [sub] => self.affine(sub, cur, e.pos())?.split(depth),
            [row, col] => self.flatten(row, col, cur, e.pos(), depth)?,
            [] => {
                return Err(Refusal::na(
                    e.pos(),
                    format!("`{base}` is named without a subscript; the IR's `Access` addresses an element, not an array object"),
                ))
            }
            _ => {
                return Err(Refusal::na(
                    e.pos(),
                    format!(
                        "`{base}` has {} subscripts: only a flat subscript and one row/column pair are lowered (a third dimension is 9.9's array-of-structs recognition)",
                        subs.len()
                    ),
                ))
            }
        };
        let access = Access {
            kind,
            base: base.to_string(),
            index,
            elem_size: ELEM_SIZE,
            offset: OffsetValue::Const(constant),
            expr: None,
            indirect: None,
            guard: guard.cloned(),
            range: None,
            index_kind: IndexKind::Affine,
        };
        self.nests[cur].body.push(access);
        Ok(())
    }

    /// `u[i - 1][j]` as the flat index of a row-major array: `row * width + col`,
    /// with `width` taken to be the innermost IV's exclusive upper bound — the
    /// square-matrix convention the corpora encode by hand (see module docs).
    fn flatten(
        &self,
        row: &Expr,
        col: &Expr,
        cur: usize,
        pos: Position,
        depth: usize,
    ) -> R<(Vec<i64>, i64)> {
        let r = self.affine(row, cur, pos)?;
        let c = self.affine(col, cur, pos)?;
        let width = self.nests[cur].ivs.last().map(|iv| iv.upper).ok_or_else(|| {
            Refusal::na(pos, "a two-subscript access has no innermost IV to take the row length from")
        })?;
        let flat = r.scale(width).ok_or_else(|| self.overflow(pos))?;
        let sum = flat.add(&c).ok_or_else(|| self.overflow(pos))?;
        Ok(sum.split(depth))
    }

    /// The base name and the subscript expressions of an array reference, outermost
    /// subscript first. `None` when the chain is not rooted at a plain identifier,
    /// which makes it an object access (§9.8) rather than an array.
    fn reference<'s>(&self, e: &'s Expr) -> Option<(String, Vec<&'s Expr>)> {
        let mut subs: Vec<&Expr> = Vec::new();
        let mut cur = e;
        loop {
            match cur {
                Expr::Index { obj, index, .. } => {
                    subs.push(index);
                    cur = obj;
                }
                Expr::Member { obj, prop, computed, .. } => {
                    if *computed {
                        // `A[expr]` written as a member expression: the property is
                        // the subscript.
                        subs.push(obj);
                        let _ = prop;
                        cur = obj;
                    } else {
                        // A named property: §9.8's shapes, not an element.
                        return None;
                    }
                }
                Expr::Ident(name, _) => {
                    subs.reverse();
                    return Some((name.clone(), subs));
                }
                _ => return None,
            }
        }
    }

    /// An access this lowering cannot name a base for.
    fn not_an_array(&self, e: &Expr) -> Refusal {
        let pos = e.pos();
        match e {
            Expr::Member { .. } => Refusal::scope(
                pos,
                "a named property access (§9.8's shape inference is what turns `obj.field` into a static offset)",
            ),
            _ => Refusal::na(
                pos,
                "the base of an access is not a plain name, so no `Access::base` names it (§9.7's points-to)",
            ),
        }
    }
}
