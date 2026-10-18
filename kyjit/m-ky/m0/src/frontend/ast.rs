//! Phase 9 §9.3 — the syntax tree.
//!
//! The shapes are the ones the spec lists, so a reader can check a variant
//! against §9.3 directly. Two departures, both deliberate:
//!
//! 1. **`Position` on every node.** The spec's tree has no spans. Lowering needs
//!    them: §9.4's refusal messages must name the source location of the
//!    construct that is out of scope, and a fuzz failure against Node (§9.12) is
//!    unreadable without one.
//! 2. **Patterns are a separate type.** §9.3's table gives `parse_object_pattern`
//!    and `parse_array_pattern` their own functions and §9.5's `ClosureInfo`
//!    stores `params: Vec<Pattern>`, so the pattern vocabulary has to be a type
//!    of its own rather than a shape of `Expr`.
//!
//! ## What the tree does *not* do
//!
//! It does not mark nodes "analyzable". The tree is the whole parsed subset; the
//! decision about what the analyzers may consume is made in [`crate::frontend::lower`],
//! once, against the §9.14 contract. Keeping the tree honest about what the
//! source *says* is what lets a later phase widen the subset without rewriting
//! the parser.

use serde::{Deserialize, Serialize};

/// A source span. 1-based line, 0-based column, matching [`crate::frontend::lexer::Token`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl Position {
    pub fn new(line: usize, col: usize) -> Position {
        Position { line, col }
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Binary operators. `StrictEq`/`StrictNe` are `===`/`!==`; the spec's §9.4
/// lowering table names `CmpEq` for `===`, so the loose forms are kept distinct
/// rather than conflated with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    /// `&&`, which §9.4 lowers to a short-circuit conditional.
    LogicalAnd,
    /// `||`, likewise.
    LogicalOr,
    /// `??`, lowered as `If(x != null, x, y)`.
    NullishCoalesce,
    Instanceof,
    In,
}

impl BinOp {
    /// `true` when this operator is guaranteed to produce a number from numbers
    /// with no coercion surprise — the set the lowering can hand to the scalar
    /// arithmetic model. Everything else falls back to conservative treatment.
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::Pow
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::UShr
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
    BitNot,
    TypeOf,
    Void,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    Pow,
    Nullish,
    And,
    Or,
}

impl AssignOp {
    /// The binary operator a compound assignment desugars to, or `None` for the
    /// logical and nullish forms, whose short-circuit behavior §9.4 models as a
    /// conditional rather than as an arithmetic operation.
    pub fn to_binop(self) -> Option<BinOp> {
        Some(match self {
            AssignOp::Assign => return None,
            AssignOp::Add => BinOp::Add,
            AssignOp::Sub => BinOp::Sub,
            AssignOp::Mul => BinOp::Mul,
            AssignOp::Div => BinOp::Div,
            AssignOp::Mod => BinOp::Mod,
            AssignOp::BitAnd => BinOp::BitAnd,
            AssignOp::BitOr => BinOp::BitOr,
            AssignOp::BitXor => BinOp::BitXor,
            AssignOp::Shl => BinOp::Shl,
            AssignOp::Shr => BinOp::Shr,
            AssignOp::UShr => BinOp::UShr,
            AssignOp::Pow => BinOp::Pow,
            AssignOp::Nullish | AssignOp::And | AssignOp::Or => return None,
        })
    }
}

/// §9.3's binding forms: `{ a, b: c, ...rest }` and `[x, y, ...rest]`.
///
/// A pattern is not an expression: it appears only on the left of an
/// initialization or in a parameter list, and §9.6 lowers it to a *sequence* of
/// loads. Making it its own type keeps that multiplicity out of `Expr`, where
/// every other node denotes exactly one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pattern {
    /// `x`
    Binding { name: String, pos: Position },
    /// `[a, b, ...rest]`
    Array { elements: Vec<Option<Pattern>>, rest: Option<Box<Pattern>>, pos: Position },
    /// `{a, b: c, ...rest}` — `props` holds `(key, pattern)` pairs in source
    /// order.
    Object { props: Vec<ObjectProp>, rest: Option<Box<Pattern>>, pos: Position },
    /// `x = expr`
    Default { left: Box<Pattern>, right: Box<Expr>, pos: Position },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectProp {
    /// The property name being read off the source object.
    pub key: String,
    /// The pattern the value is bound to — a `Binding` for `{a}`, nested for
    /// `{a: [x, y]}`.
    pub value: Pattern,
    /// `true` when the source wrote `{a}` rather than `{a: a}`.
    pub shorthand: bool,
}

impl Pattern {
    pub fn pos(&self) -> Position {
        match self {
            Pattern::Binding { pos, .. }
            | Pattern::Array { pos, .. }
            | Pattern::Object { pos, .. }
            | Pattern::Default { pos, .. } => *pos,
        }
    }

    /// Every name this pattern binds, in source order. §9.6 needs this to emit
    /// one declaration per binding, and §9.5's free-variable scan uses it to
    /// decide what a closure captures.
    pub fn bindings(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_bindings(&mut out);
        out
    }

    fn collect_bindings(&self, out: &mut Vec<String>) {
        match self {
            Pattern::Binding { name, .. } => out.push(name.clone()),
            Pattern::Array { elements, rest, .. } => {
                for e in elements.iter().flatten() {
                    e.collect_bindings(out);
                }
                if let Some(r) = rest {
                    r.collect_bindings(out);
                }
            }
            Pattern::Object { props, rest, .. } => {
                for p in props {
                    p.value.collect_bindings(out);
                }
                if let Some(r) = rest {
                    r.collect_bindings(out);
                }
            }
            Pattern::Default { left, .. } => left.collect_bindings(out),
        }
    }
}

/// §9.3's expression table, variant for variant.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expr {
    /// A numeric literal. `int` is `Some(_)` only when the literal names an exact
    /// `i64`; the original text is kept for diagnostics. A fractional literal has
    /// `int: None`, which is what tells §9.4 it is not an analytic index.
    Num { int: Option<i64>, text: String, pos: Position },
    Str(String, Position),
    Bool(bool, Position),
    Null(Position),
    Undefined(Position),
    Ident(String, Position),
    ArrayLiteral { elements: Vec<Option<Expr>>, pos: Position },
    ObjectLiteral { props: Vec<(String, Expr)>, pos: Position },
    Function { name: Option<String>, params: Vec<Pattern>, body: Vec<Stmt>, is_async: bool, pos: Position },
    Arrow { params: Vec<Pattern>, body: ArrowBody, is_async: bool, pos: Position },
    Call { callee: Box<Expr>, args: Vec<Expr>, pos: Position },
    New { callee: Box<Expr>, args: Vec<Expr>, pos: Position },
    Member { obj: Box<Expr>, prop: String, computed: bool, pos: Position },
    Index { obj: Box<Expr>, index: Box<Expr>, pos: Position },
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr>, pos: Position },
    Unary { op: UnOp, arg: Box<Expr>, pos: Position },
    Update { op: UpdateOp, operand: Box<Expr>, prefix: bool, pos: Position },
    Assign { op: AssignOp, target: Box<Expr>, value: Box<Expr>, pos: Position },
    Conditional { cond: Box<Expr>, then: Box<Expr>, els: Box<Expr>, pos: Position },
    Template { parts: Vec<TemplatePart>, pos: Position },
    Spread(Box<Expr>, Position),
    Yield(Option<Box<Expr>>, Position),
    Await(Box<Expr>, Position),
    This(Position),
    Super(Position),
    /// `a, b` — the comma operator. Only its last operand has a value, which is
    /// why §9.4 keeps it separate from `Binary`.
    Seq(Vec<Expr>, Position),
}

/// `++`/`--`. Carried as its own node rather than desugared at parse time so the
/// IV recognition in §9.4 can see the source shape `i++`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateOp {
    Inc,
    Dec,
}

/// An arrow body is either an expression (`x => x * 2`) or a block
/// (`x => { return x; }`), which is what §9.3 calls `ArrowBody`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplatePart {
    /// A literal chunk between `$`-escapes.
    Text(String),
    /// An interpolation. Outside the analyzable subset (§9.14 lists only
    /// `String.length`); carried so the lowering can refuse at the right
    /// location instead of failing in the lexer.
    Expr(Expr),
}

/// §9.3's statement table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stmt {
    Expr(Expr),
    Let { pattern: Pattern, init: Option<Expr>, pos: Position },
    Const { pattern: Pattern, init: Expr, pos: Position },
    Var { pattern: Pattern, init: Option<Expr>, pos: Position },
    If { cond: Expr, then: Box<Stmt>, els: Option<Box<Stmt>>, pos: Position },
    While { cond: Expr, body: Box<Stmt>, pos: Position },
    DoWhile { body: Box<Stmt>, cond: Expr, pos: Position },
    For { head: ForHead, body: Box<Stmt>, pos: Position },
    ForOf { pattern: Pattern, iter: Expr, body: Box<Stmt>, is_async: bool, pos: Position },
    ForIn { pattern: Pattern, obj: Expr, body: Box<Stmt>, pos: Position },
    FunctionDecl { name: String, params: Vec<Pattern>, body: Vec<Stmt>, is_async: bool, pos: Position },
    Class { name: Option<String>, members: Vec<Stmt>, pos: Position },
    Return(Option<Expr>, Position),
    Break(Option<String>, Position),
    Continue(Option<String>, Position),
    Throw(Expr, Position),
    Try { body: Vec<Stmt>, catch: Option<CatchClause>, finally: Option<Vec<Stmt>>, pos: Position },
    Switch { subject: Expr, cases: Vec<SwitchCase>, pos: Position },
    Block(Vec<Stmt>, Position),
    Empty(Position),
    Import { specifiers: Vec<String>, source: String, pos: Position },
    Export { decl: Box<Stmt>, default: bool, pos: Position },
}

/// A `for` header, kept as one type because §9.3 gives it one parser function
/// (`parse_for`). The iteration forms are their own statements (`ForOf`,
/// `ForIn`) because they carry a pattern and an iterable, not three clauses —
/// and because §9.6 lowers them to a shape that has nothing in common with a
/// counted loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForHead {
    /// `for (init; cond; update)` — any clause may be absent.
    Classical { init: Option<Box<Stmt>>, cond: Option<Expr>, update: Option<Expr> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchCase {
    /// `None` is the `default:` arm.
    pub test: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchClause {
    pub pattern: Option<Pattern>,
    pub body: Vec<Stmt>,
}

/// A parsed file. The top level is a program of statements, which is what a
/// `.js` file is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub body: Vec<Stmt>,
    /// `true` when the source used `import`/`export`: module code is parsed but
    /// never lowered, because module resolution is outside the analyzable subset.
    pub is_module: bool,
}

impl Program {
    pub fn new(body: Vec<Stmt>) -> Program {
        let is_module = body.iter().any(|s| matches!(s, Stmt::Import { .. } | Stmt::Export { .. }));
        Program { body, is_module }
    }
}

impl Stmt {
    pub fn pos(&self) -> Position {
        match self {
            Stmt::Expr(e) => e.pos(),
            Stmt::Let { pos, .. }
            | Stmt::Const { pos, .. }
            | Stmt::Var { pos, .. }
            | Stmt::If { pos, .. }
            | Stmt::While { pos, .. }
            | Stmt::DoWhile { pos, .. }
            | Stmt::For { pos, .. }
            | Stmt::ForOf { pos, .. }
            | Stmt::ForIn { pos, .. }
            | Stmt::FunctionDecl { pos, .. }
            | Stmt::Class { pos, .. }
            | Stmt::Return(_, pos)
            | Stmt::Break(_, pos)
            | Stmt::Continue(_, pos)
            | Stmt::Throw(_, pos)
            | Stmt::Try { pos, .. }
            | Stmt::Switch { pos, .. }
            | Stmt::Block(_, pos)
            | Stmt::Empty(pos)
            | Stmt::Import { pos, .. }
            | Stmt::Export { pos, .. } => *pos,
        }
    }

    /// The statements of a block body, when this is one. Used by the lowering to
    /// walk a loop body without matching on every other variant.
    pub fn as_block(&self) -> Option<&[Stmt]> {
        match self {
            Stmt::Block(v, _) => Some(v),
            _ => None,
        }
    }
}

impl Expr {
    pub fn pos(&self) -> Position {
        match self {
            Expr::Num { pos, .. }
            | Expr::Str(_, pos)
            | Expr::Bool(_, pos)
            | Expr::Null(pos)
            | Expr::Undefined(pos)
            | Expr::Ident(_, pos)
            | Expr::ArrayLiteral { pos, .. }
            | Expr::ObjectLiteral { pos, .. }
            | Expr::Function { pos, .. }
            | Expr::Arrow { pos, .. }
            | Expr::Call { pos, .. }
            | Expr::New { pos, .. }
            | Expr::Member { pos, .. }
            | Expr::Index { pos, .. }
            | Expr::Binary { pos, .. }
            | Expr::Unary { pos, .. }
            | Expr::Update { pos, .. }
            | Expr::Assign { pos, .. }
            | Expr::Conditional { pos, .. }
            | Expr::Template { pos, .. }
            | Expr::Spread(_, pos)
            | Expr::Yield(_, pos)
            | Expr::Await(_, pos)
            | Expr::This(pos)
            | Expr::Super(pos)
            | Expr::Seq(_, pos) => *pos,
        }
    }

    /// The integer a numeric literal names, when it names one exactly. `Some(_)`
    /// is what makes an index analytic; a fractional or out-of-range literal
    /// returns `None`, and the lowering treats it as a runtime value.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Expr::Num { int, .. } => *int,
            _ => None,
        }
    }

    pub fn as_ident(&self) -> Option<&str> {
        match self {
            Expr::Ident(s, _) => Some(s),
            _ => None,
        }
    }
}




