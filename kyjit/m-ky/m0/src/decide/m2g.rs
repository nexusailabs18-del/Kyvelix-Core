//! M2g — affine-in-disguise.
//!
//! M2g is a *normalizer*, not a decision procedure. It scans a loop for
//! accesses that carry a surface `expr` (filled by the JS frontend),
//! recognizes affine patterns that look non-affine, rewrites them into
//! standard `index` + `offset` form, then delegates the decision to M0.
//!
//! Recognized patterns (with soundness conditions):
//!
//!   (i << c)         ->  2^c * i
//!   i * c            ->  c * i
//!   i + c            ->  i + c
//!   (i << a) | b     ->  2^a * i + b       requires 0 <= b < 2^a
//!   (i + c) * a      ->  a * i + a*c
//!
//! The `| b` guard is strict. If b >= 2^a, the constant reaches into
//! the shifted bits of `2^a * i` and the OR is NOT affine in i.
//! Example: (i << 1) | 3 is not 2*i + 3; it equals 2*i + 3 only when
//! i is even, and the IV bounds do not guarantee that. Such cases
//! return Unknown. See g004b below.
//!   (i / c) * c      ->  i                 only when i is provably 0 mod c
//!
//! If any `expr` cannot be normalized, M2g returns Unknown. It never
//! panics and never guesses. Every Exact it emits was produced by M0 on
//! the normalized loop and is verifiable by the existing checker.

use super::{m0, Decision};
use crate::certificate::{Certificate, Witness, RewriteRule};
use crate::ir::*;

// ============================================================================
// Tokenizer
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Num(i64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Shl,
    Or,
    LParen,
    RParen,
    Eof,
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => { toks.push(Tok::Plus); i += 1; }
            '-' => { toks.push(Tok::Minus); i += 1; }
            '*' => { toks.push(Tok::Star); i += 1; }
            '/' => { toks.push(Tok::Slash); i += 1; }
            '|' => { toks.push(Tok::Or); i += 1; }
            '(' => { toks.push(Tok::LParen); i += 1; }
            ')' => { toks.push(Tok::RParen); i += 1; }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '<' {
                    toks.push(Tok::Shl);
                    i += 2;
                } else {
                    return None;
                }
            }
            _ if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let n: i64 = chars[start..i].iter().collect::<String>().parse().ok()?;
                toks.push(Tok::Num(n));
            }
            _ if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                toks.push(Tok::Ident(name));
            }
            _ => return None,
        }
    }
    toks.push(Tok::Eof);
    Some(toks)
}

// ============================================================================
// Parser (public for M4 constant folding)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Expr {
    Num(i64),
    Var(String),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Shl(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Self { Parser { toks, pos: 0 } }
    fn peek(&self) -> &Tok { self.toks.get(self.pos).unwrap_or(&Tok::Eof) }
    fn advance(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() { self.pos += 1; }
        t
    }
    fn parse(&mut self) -> Option<Expr> {
        let e = self.parse_or()?;
        if *self.peek() != Tok::Eof { return None; }
        Some(e)
    }
    fn parse_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_add()?;
        while *self.peek() == Tok::Or {
            self.advance();
            let right = self.parse_add()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Some(left)
    }
    fn parse_add(&mut self) -> Option<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            match self.peek() {
                Tok::Plus => {
                    self.advance();
                    let r = self.parse_mul()?;
                    left = Expr::Add(Box::new(left), Box::new(r));
                }
                Tok::Minus => {
                    self.advance();
                    let r = self.parse_mul()?;
                    left = Expr::Sub(Box::new(left), Box::new(r));
                }
                _ => break,
            }
        }
        Some(left)
    }
    fn parse_mul(&mut self) -> Option<Expr> {
        let mut left = self.parse_atom()?;
        loop {
            match self.peek() {
                Tok::Star => {
                    self.advance();
                    let r = self.parse_atom()?;
                    left = Expr::Mul(Box::new(left), Box::new(r));
                }
                Tok::Slash => {
                    self.advance();
                    let r = self.parse_atom()?;
                    left = Expr::Div(Box::new(left), Box::new(r));
                }
                Tok::Shl => {
                    self.advance();
                    let r = self.parse_atom()?;
                    left = Expr::Shl(Box::new(left), Box::new(r));
                }
                _ => break,
            }
        }
        Some(left)
    }
    fn parse_atom(&mut self) -> Option<Expr> {
        match self.advance() {
            Tok::Num(n) => Some(Expr::Num(n)),
            Tok::Ident(s) => Some(Expr::Var(s)),
            Tok::LParen => {
                let e = self.parse_or()?;
                if self.advance() != Tok::RParen { return None; }
                Some(e)
            }
            Tok::Minus => {
                let e = self.parse_atom()?;
                Some(Expr::Sub(Box::new(Expr::Num(0)), Box::new(e)))
            }
            _ => None,
        }
    }
}

pub fn parse_expr(s: &str) -> Option<Expr> {
    let toks = tokenize(s)?;
    let mut p = Parser::new(toks);
    p.parse()
}

// ============================================================================
// Affine conversion (public for M4 constant folding)
// ============================================================================

pub fn to_affine_strict(e: &Expr, ivs: &[IV]) -> Option<(Vec<i64>, i64)> {
    let d = ivs.len();
    match e {
        Expr::Num(n) => Some((vec![0; d], *n)),
        Expr::Var(name) => {
            let pos = ivs.iter().position(|iv| &iv.name == name)?;
            let mut c = vec![0i64; d];
            c[pos] = 1;
            Some((c, 0))
        }
        Expr::Add(a, b) => {
            let (ca, ka) = to_affine_strict(a, ivs)?;
            let (cb, kb) = to_affine_strict(b, ivs)?;
            let mut c = ca;
            for i in 0..d { c[i] = c[i].checked_add(cb[i])?; }
            Some((c, ka.checked_add(kb)?))
        }
        Expr::Sub(a, b) => {
            let (ca, ka) = to_affine_strict(a, ivs)?;
            let (cb, kb) = to_affine_strict(b, ivs)?;
            let mut c = ca;
            for i in 0..d { c[i] = c[i].checked_sub(cb[i])?; }
            Some((c, ka.checked_sub(kb)?))
        }
        Expr::Mul(a, b) => {
            let (ca, ka) = to_affine_strict(a, ivs)?;
            let (cb, kb) = to_affine_strict(b, ivs)?;
            let a_const = ca.iter().all(|&x| x == 0);
            let b_const = cb.iter().all(|&x| x == 0);
            if a_const {
                let mut c = vec![0i64; d];
                for i in 0..d { c[i] = cb[i].checked_mul(ka)?; }
                Some((c, kb.checked_mul(ka)?))
            } else if b_const {
                let mut c = vec![0i64; d];
                for i in 0..d { c[i] = ca[i].checked_mul(kb)?; }
                Some((c, ka.checked_mul(kb)?))
            } else {
                None
            }
        }
        Expr::Shl(a, b) => {
            let (ca, ka) = to_affine_strict(a, ivs)?;
            let (cb, kb) = to_affine_strict(b, ivs)?;
            if !cb.iter().all(|&x| x == 0) { return None; }
            if !(0..=62).contains(&kb) { return None; }
            let mult: i64 = 1i64.checked_shl(kb as u32)?;
            let mut c = vec![0i64; d];
            for i in 0..d { c[i] = ca[i].checked_mul(mult)?; }
            Some((c, ka.checked_mul(mult)?))
        }
        Expr::Div(_, _) => None,
        Expr::Or(_, _) => None,
    }
}

fn affine_always_multiple_of(coeffs: &[i64], constant: i64, ivs: &[IV], c: i64) -> bool {
    if c == 0 || c == 1 || c == -1 { return true; }
    let c = c.abs();
    for (k, iv) in ivs.iter().enumerate() {
        if coeffs[k] % c != 0 {
            if iv.lower % c != 0 || iv.step % c != 0 {
                return false;
            }
        }
    }
    constant % c == 0
}

fn to_affine(e: &Expr, ivs: &[IV]) -> Option<(Vec<i64>, i64)> {
    if let Expr::Mul(a, b) = e {
        let try_cancel = |x: &Expr, y: &Expr| -> Option<(Vec<i64>, i64)> {
            if let Expr::Div(inner, div) = x {
                if let (Expr::Num(c1), Expr::Num(c2)) = (div.as_ref(), y) {
                    if c1 == c2 {
                        let (coeffs, konst) = to_affine(inner, ivs)?;
                        if affine_always_multiple_of(&coeffs, konst, ivs, *c1) {
                            return Some((coeffs, konst));
                        }
                    }
                }
            }
            None
        };
        if let Some(r) = try_cancel(a, b) { return Some(r); }
        if let Some(r) = try_cancel(b, a) { return Some(r); }
    }
    if let Expr::Or(a, b) = e {
        if let Expr::Shl(inner, shift) = a.as_ref() {
            let (cs, ks) = to_affine(shift, ivs)?;
            if !cs.iter().all(|&x| x == 0) { return None; }
            if !(0..=62).contains(&ks) { return None; }
            let (cb, kb) = to_affine(b, ivs)?;
            if !cb.iter().all(|&x| x == 0) { return None; }
            let mult: i64 = 1i64.checked_shl(ks as u32)?;
            if kb < 0 || kb >= mult { return None; }
            let (ci, ki) = to_affine(inner, ivs)?;
            let mut c = vec![0i64; ivs.len()];
            for i in 0..c.len() { c[i] = ci[i].checked_mul(mult)?; }
            return Some((c, ki.checked_mul(mult)?.checked_add(kb)?));
        }
        return None;
    }
    match e {
        Expr::Add(a, b) => {
            let (ca, ka) = to_affine(a, ivs)?;
            let (cb, kb) = to_affine(b, ivs)?;
            let mut c = ca;
            for i in 0..c.len() { c[i] = c[i].checked_add(cb[i])?; }
            Some((c, ka.checked_add(kb)?))
        }
        Expr::Sub(a, b) => {
            let (ca, ka) = to_affine(a, ivs)?;
            let (cb, kb) = to_affine(b, ivs)?;
            let mut c = ca;
            for i in 0..c.len() { c[i] = c[i].checked_sub(cb[i])?; }
            Some((c, ka.checked_sub(kb)?))
        }
        Expr::Mul(a, b) => {
            let (ca, ka) = to_affine(a, ivs)?;
            let (cb, kb) = to_affine(b, ivs)?;
            let a_const = ca.iter().all(|&x| x == 0);
            let b_const = cb.iter().all(|&x| x == 0);
            if a_const {
                let mut c = vec![0i64; ivs.len()];
                for i in 0..c.len() { c[i] = cb[i].checked_mul(ka)?; }
                Some((c, kb.checked_mul(ka)?))
            } else if b_const {
                let mut c = vec![0i64; ivs.len()];
                for i in 0..c.len() { c[i] = ca[i].checked_mul(kb)?; }
                Some((c, ka.checked_mul(kb)?))
            } else {
                None
            }
        }
        Expr::Shl(a, b) => {
            let (ca, ka) = to_affine(a, ivs)?;
            let (cb, kb) = to_affine(b, ivs)?;
            if !cb.iter().all(|&x| x == 0) { return None; }
            if !(0..=62).contains(&kb) { return None; }
            let mult: i64 = 1i64.checked_shl(kb as u32)?;
            let mut c = vec![0i64; ivs.len()];
            for i in 0..c.len() { c[i] = ca[i].checked_mul(mult)?; }
            Some((c, ka.checked_mul(mult)?))
        }
        Expr::Div(_, _) => None,
        Expr::Or(_, _) => None,
        Expr::Num(_) | Expr::Var(_) => to_affine_strict(e, ivs),
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Rewrite every `expr`-bearing access into standard affine form.
/// Returns `Some(loop)` only if every `expr` normalizes; `None` otherwise.
pub fn normalize(loop_: &Loop) -> Option<Loop> {
    let mut changed = false;
    let mut body = Vec::with_capacity(loop_.body.len());
    for a in &loop_.body {
        match &a.expr {
            None => body.push(a.clone()),
            Some(s) => {
                let e = parse_expr(s)?;
                let (coeffs, konst) = to_affine(&e, &loop_.ivs)?;
                if coeffs.len() != loop_.depth { return None; }
                let mut a2 = a.clone();
                a2.index = coeffs;
                a2.offset = OffsetValue::Const(konst);
                a2.expr = None;
                body.push(a2);
                changed = true;
            }
        }
    }
    if !changed { return None; }
    Some(Loop {
        depth: loop_.depth,
        ivs: loop_.ivs.clone(),
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,})
}

/// M2g entry point — same signature as `m0::decide`.
/// Certificates are emitted against the *normalized* loop; `level` becomes "M2g".
pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    match normalize(loop_) {
        Some(normalized) => {
            let (d, c) = m0::decide(&normalized, p, q);
            (d, c.map(|mut cert| {
                cert.level = "M2g".to_string();
                cert.loop_hash = loop_.hash();
                cert.witness = Witness::Rewrite {
                    rule: RewriteRule::M2gNormalize,
                    original: Box::new(loop_.clone()),
                    rewritten: Box::new(normalized),
                    inner: Box::new(cert.witness),
                };
                cert
            }))
        }
        None => (
            Decision::Unknown("M2g: no affine-in-disguise pattern".into()),
            None,
        ),
    }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(name: &str) -> IV {
        IV::new(name, 0, 100, 1)
    }
    fn iv_step(name: &str, lo: i64, step: i64) -> IV {
        IV::new(name, lo, 100, step)
    }
    fn affine(expr: &str, ivs: &[IV]) -> Option<(Vec<i64>, i64)> {
        let e = parse_expr(expr)?;
        to_affine(&e, ivs)
    }

    #[test]
    fn g001_shift_add() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("(i << 1) + 1", &ivs), Some((vec![2], 1)));
    }

    #[test]
    fn g002_mul() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("i * 4", &ivs), Some((vec![4], 0)));
    }

    #[test]
    fn g003_add_then_mul() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("(i + 1) * 2", &ivs), Some((vec![2], 2)));
    }

    #[test]
    fn g004a_or_in_range() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("(i << 1) | 1", &ivs), Some((vec![2], 1)));
    }

    /// `(i << 1) | 3` is NOT affine. Bitwise OR with a constant that
    /// reaches into the shifted bits is not expressible as `2^a * i + b`
    /// with `0 <= b < 2^a`; here `b = 3 >= 2^1 = 2`, so the pattern is
    /// rejected and M2g returns Unknown. This is deliberate.
    #[test]
    fn g004b_or_out_of_range_is_not_affine() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("(i << 1) | 3", &ivs), None);
    }

    #[test]
    fn g005_div_cancel_when_provable() {
        let ivs = vec![iv_step("i", 0, 2)];
        assert_eq!(affine("i / 2 * 2", &ivs), Some((vec![1], 0)));
    }

    #[test]
    fn g005_div_cancel_unprovable() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("i / 2 * 2", &ivs), None);
    }

    #[test]
    fn g006_quadratic_is_unknown() {
        let ivs = vec![iv("i")];
        assert_eq!(affine("i * i", &ivs), None);
    }

    #[test]
    fn g007_modulo_rejected_by_tokenizer() {
        assert!(parse_expr("i % 4").is_none());
    }

    #[test]
    fn multiple_ivs() {
        let ivs = vec![iv("i"), iv("j")];
        assert_eq!(affine("i + j + 3", &ivs), Some((vec![1, 1], 3)));
        assert_eq!(affine("(i << 1) + j", &ivs), Some((vec![2, 1], 0)));
    }

    #[test]
    fn normalize_rewrites_and_clears_expr() {
        let l = Loop {
            depth: 1,
            ivs: vec![iv("i")],
            body: vec![
                Access {
                    kind: AccessKind::Write,
                    base: "A".into(),
                    index: vec![0],
                    elem_size: 8,
                    offset: OffsetValue::Const(0),
                    expr: Some("i".into()),
                    indirect: None,
                    guard: None,
                    range: None,
                    index_kind: IndexKind::Affine,
                },
                Access {
                    kind: AccessKind::Read,
                    base: "A".into(),
                    index: vec![0],
                    elem_size: 8,
                    offset: OffsetValue::Const(0),
                    expr: Some("(i << 1) + 1".into()),
                    indirect: None,
                    guard: None,
                    range: None,
                    index_kind: IndexKind::Affine,
                },
            ],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        let n = normalize(&l).expect("should normalize");
        assert_eq!(n.body[0].index, vec![1]);
        assert_eq!(n.body[1].index, vec![2]);
        assert_eq!(n.body[1].offset, OffsetValue::Const(1));
        assert!(n.body[0].expr.is_none());
        assert!(n.body[1].expr.is_none());
    }

    #[test]
    fn normalize_returns_none_on_quadratic() {
        let l = Loop {
            depth: 1,
            ivs: vec![iv("i")],
            body: vec![Access {
                kind: AccessKind::Read,
                base: "A".into(),
                index: vec![0],
                elem_size: 8,
                offset: OffsetValue::Const(0),
                expr: Some("i * i".into()),
                indirect: None,
                guard: None,
                range: None,
                index_kind: IndexKind::Affine,
            }],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        assert!(normalize(&l).is_none());
    }

    #[test]
    fn normalize_returns_none_when_no_exprs() {
        let l = Loop {
            depth: 1,
            ivs: vec![iv("i")],
            body: vec![Access {
                kind: AccessKind::Read,
                base: "A".into(),
                index: vec![1],
                elem_size: 8,
                offset: OffsetValue::Const(0),
                expr: None,
                indirect: None,
                guard: None,
                range: None,
                index_kind: IndexKind::Affine,
            }],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        assert!(normalize(&l).is_none());
    }
    // ================================================================
    // Property fuzz: random surface expressions. The oracle evaluates
    // the expression over the IV range directly; the parser must
    // produce an affine form that agrees at every IV, or return None.
    //
    // For expressions whose surface form is genuinely non-affine
    // (e.g. (i << a) | b with b >= 2^a), the parser must return None,
    // not a wrong affine form.
    // ================================================================

    fn xorshift2g(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    /// Evaluate a surface expression over a concrete IV value, using
    /// Rust semantics (arithmetic shift for <<, bitwise for |).
    fn eval_surface(expr: &str, iv: i64) -> Option<i64> {
        // We'll parse the same surface syntax manually with our own
        // mini-interpreter. Only the expressions this fuzzer emits are
        // supported.
        // Format: "<op> <args>" lines, one per node, in RPN.
        // Simpler: we'll just re-derive from the shape.
        // This function is a no-op; the oracle is computed inline below.
        let _ = (expr, iv);
        None
    }

    #[derive(Debug, Clone)]
    enum Surf {
        MulConst(i64),           // c * i
        AddConst(i64),           // i + c
        ShlConst(u32),           // i << a
        ShlOrConst(u32, i64),    // (i << a) | b
        AddThenMul(i64, i64),    // (i + c) * a
    }

    fn surf_str(s: &Surf) -> String {
        match s {
            Surf::MulConst(c) => format!("i * {}", c),
            Surf::AddConst(c) => if *c >= 0 { format!("i + {}", c) } else { format!("i - {}", -c) },
            Surf::ShlConst(a) => format!("(i << {})", a),
            Surf::ShlOrConst(a, b) => format!("(i << {}) | {}", a, b),
            Surf::AddThenMul(c, a) => if *c >= 0 { format!("(i + {}) * {}", c, a) } else { format!("(i - {}) * {}", -c, a) },
        }
    }

    fn eval_surf(s: &Surf, i: i64) -> i64 {
        match s {
            Surf::MulConst(c) => c * i,
            Surf::AddConst(c) => i + c,
            Surf::ShlConst(a) => i << a,
            Surf::ShlOrConst(a, b) => (i << a) | b,
            Surf::AddThenMul(c, a) => (i + c) * a,
        }
    }

    /// The affine oracle: if the surface form is affine, returns (c, d)
    /// such that eval_surf(s, i) == c * i + d for all i in the domain.
    /// If not affine on the domain, returns None.
    fn oracle_affine(s: &Surf, lo: i64, hi: i64) -> Option<(i64, i64)> {
        // Sample every IV in [lo, hi) and check linearity.
        if hi <= lo { return None; }
        // Compute first two points.
        let i0 = lo;
        let i1 = lo + 1;
        if i1 >= hi {
            // Single-point domain: any affine fits. Return the value at i0.
            return Some((0, eval_surf(s, i0)));
        }
        let v0 = eval_surf(s, i0);
        let v1 = eval_surf(s, i1);
        let c = v1 - v0;
        let d = v0 - c * i0;
        for i in lo..hi {
            if c * i + d != eval_surf(s, i) {
                return None;
            }
        }
        Some((c, d))
    }

    #[test]
    fn fuzz_m2g_parser_property() {
        let mut state: u64 = 0x2a_0badc0de;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut examples: Vec<String> = Vec::new();

        let ivs = vec![IV::new("i", 0, 16, 1)];

        for _ in 0..3000 {
            let s = match xorshift2g(&mut state) % 5 {
                0 => Surf::MulConst((xorshift2g(&mut state) % 8) as i64 + 1),
                1 => Surf::AddConst((xorshift2g(&mut state) % 9) as i64 - 4),
                2 => Surf::ShlConst((xorshift2g(&mut state) % 4) as u32),
                3 => {
                    let a = (xorshift2g(&mut state) % 4) as u32;
                    let b = (xorshift2g(&mut state) % 16) as i64;
                    Surf::ShlOrConst(a, b)
                }
                _ => {
                    let c = (xorshift2g(&mut state) % 7) as i64 - 3;
                    let a = (xorshift2g(&mut state) % 5) as i64 + 1;
                    Surf::AddThenMul(c, a)
                }
            };
            let e = surf_str(&s);
            let expected = oracle_affine(&s, 0, 16);

            // to_affine returns (coeffs: Vec<i64>, constant: i64).
            // M2g is depth-1, so the coeffs vector has length 1.
            let parsed_v = parse_expr(&e).and_then(|ast| to_affine(&ast, &ivs));
            let parsed: Option<(i64, i64)> = parsed_v.and_then(|(v, d)| {
                if v.len() == 1 { Some((v[0], d)) } else { None }
            });
            tested += 1;

            match (expected, parsed) {
                (None, None) => {}
                (None, Some((c, d))) => {
                    // Verify: is (c, d) actually affine for the expression?
                    let mut agrees = true;
                    for i in 0..16 {
                        if c * i + d != eval_surf(&s, i) { agrees = false; break; }
                    }
                    if !agrees {
                        unsound += 1;
                        if examples.len() < 10 {
                            examples.push(format!("NONAFFINE-ACCEPTED expr={} parser=({}, {})", e, c, d));
                        }
                    }
                }
                (Some((c, d)), Some((c2, d2))) => {
                    if c != c2 || d != d2 {
                        unsound += 1;
                        if examples.len() < 10 {
                            examples.push(format!(
                                "MISMATCH expr={} oracle=({}, {}) parser=({}, {})",
                                e, c, d, c2, d2
                            ));
                        }
                    }
                }
                (Some(_), None) => {
                    // Conservative — parser refused something it could have accepted.
                }
            }
        }

        println!("M2g fuzz: tested={} unsound={}", tested, unsound);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M2g parser found {} unsound", unsound);
    }

}
