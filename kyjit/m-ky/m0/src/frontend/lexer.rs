//! Phase 9 §9.2 — tokenizer for the analyzable JavaScript subset.
//!
//! The spec says "extend the existing lexer in `kyjit_frontend.c`". There is no
//! such file in this repository: `/home/kumarr/kyjit/src/lexer/` is a README
//! describing a future SIMD tokenizer, and nothing under that tree is Rust or
//! wired into the build. So the frontend lexer is written here instead, as a
//! single-pass character scanner, which is what §9.2 asks for.
//!
//! It covers all of §9.2's list: template literals, regex literals, numeric
//! separators, exponent notation, the BigInt suffix, and the full keyword set.
//!
//! ## Lexing is permissive, parsing is strict
//!
//! The scanner accepts lexemes it has no intention of analyzing — regex bodies,
//! template spans, BigInt literals — and the *parser* decides what is inside the
//! analyzable subset. Keeping that decision in one place matters for soundness:
//! a scanner that itself refuses input puts an accept/reject boundary in two
//! components that then have to agree, and only one of them is checked against
//! the subset contract (§9.14).
//!
//! Malformed input becomes [`TokenKind::Bad`] carrying a message, and scanning
//! continues, so a fuzz run gets a diagnostic instead of a truncated token
//! stream. `Bad` is always a parse error, never `Unknown`.

use serde::{Deserialize, Serialize};

/// Every lexeme class the frontend recognizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenKind {
    /// Identifier; keywords are recognized here too, as [`TokenKind::Keyword`].
    Ident,
    /// Numeric literal. `1_000_000`, `1e10`, `0x1f` and `42n` are all lexed;
    /// [`Token::num`] says whether the value is an exact `i64`.
    Num,
    /// `'...'` or `"..."`, escapes decoded.
    Str,
    /// `` `...` `` — raw source between the backticks.
    Template,
    /// `/pattern/flags`, recognized only where an expression may begin.
    Regex,
    Keyword(Keyword),
    Punct(Punct),
    /// End of input.
    Eof,
    /// An unterminated or otherwise invalid lexeme.
    Bad(String),
}

/// §9.2's keyword coverage list, in the order the spec names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Keyword {
    Let,
    Const,
    Var,
    Class,
    Extends,
    Super,
    Import,
    Export,
    Yield,
    Await,
    Async,
    Of,
    In,
    Instanceof,
    Typeof,
    Void,
    Delete,
    Function,
    Return,
    If,
    Else,
    While,
    Do,
    For,
    Break,
    Continue,
    New,
    This,
    Throw,
    Try,
    Catch,
    Finally,
    Switch,
    Case,
    Default,
    True,
    False,
    Null,
    Undefined,
}

impl Keyword {
    pub fn from_str(s: &str) -> Option<Keyword> {
        Some(match s {
            "let" => Keyword::Let,
            "const" => Keyword::Const,
            "var" => Keyword::Var,
            "class" => Keyword::Class,
            "extends" => Keyword::Extends,
            "super" => Keyword::Super,
            "import" => Keyword::Import,
            "export" => Keyword::Export,
            "yield" => Keyword::Yield,
            "await" => Keyword::Await,
            "async" => Keyword::Async,
            "of" => Keyword::Of,
            "in" => Keyword::In,
            "instanceof" => Keyword::Instanceof,
            "typeof" => Keyword::Typeof,
            "void" => Keyword::Void,
            "delete" => Keyword::Delete,
            "function" => Keyword::Function,
            "return" => Keyword::Return,
            "if" => Keyword::If,
            "else" => Keyword::Else,
            "while" => Keyword::While,
            "do" => Keyword::Do,
            "for" => Keyword::For,
            "break" => Keyword::Break,
            "continue" => Keyword::Continue,
            "new" => Keyword::New,
            "this" => Keyword::This,
            "throw" => Keyword::Throw,
            "try" => Keyword::Try,
            "catch" => Keyword::Catch,
            "finally" => Keyword::Finally,
            "switch" => Keyword::Switch,
            "case" => Keyword::Case,
            "default" => Keyword::Default,
            "true" => Keyword::True,
            "false" => Keyword::False,
            "null" => Keyword::Null,
            "undefined" => Keyword::Undefined,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        use Keyword::*;
        match self {
            Let => "let", Const => "const", Var => "var", Class => "class",
            Extends => "extends", Super => "super", Import => "import",
            Export => "export", Yield => "yield", Await => "await",
            Async => "async", Of => "of", In => "in", Instanceof => "instanceof",
            Typeof => "typeof", Void => "void", Delete => "delete",
            Function => "function", Return => "return", If => "if",
            Else => "else", While => "while", Do => "do", For => "for",
            Break => "break", Continue => "continue", New => "new",
            This => "this", Throw => "throw", Try => "try", Catch => "catch",
            Finally => "finally", Switch => "switch", Case => "case",
            Default => "default", True => "true", False => "false",
            Null => "null", Undefined => "undefined",
        }
    }
}

/// Punctuation. `Punct::Slash` and `Punct::Regex` never collide: the scanner
/// only produces `Slash` where an operand cannot begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Punct {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Colon,
    Question,
    Dot,
    Ellipsis,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Bang,
    Lt,
    Gt,
    ShiftLeft,
    ShiftRight,
    UShiftRight,
    Eq,
    EqEq,
    EqEqEq,
    Ne,
    NeNe,
    Le,
    Ge,
    PlusPlus,
    MinusMinus,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpAmp,
    PipePipe,
    Nullish,
    QuestionDot,
}

impl Punct {
    pub fn as_str(self) -> &'static str {
        use Punct::*;
        match self {
            LParen => "(", RParen => ")", LBrace => "{", RBrace => "}",
            LBracket => "[", RBracket => "]", Semi => ";", Comma => ",",
            Colon => ":", Question => "?", Dot => ".", Ellipsis => "...",
            Arrow => "=>", Plus => "+", Minus => "-", Star => "*", Slash => "/",
            Percent => "%", StarStar => "**", Amp => "&", Pipe => "|",
            Caret => "^", Tilde => "~", Bang => "!", Lt => "<", Gt => ">",
            ShiftLeft => "<<", ShiftRight => ">>", UShiftRight => ">>>",
            Eq => "=", EqEq => "==", EqEqEq => "===", Ne => "!=", NeNe => "!==",
            Le => "<=", Ge => ">=", PlusPlus => "++", MinusMinus => "--",
            PlusEq => "+=", MinusEq => "-=", StarEq => "*=", SlashEq => "/=",
            PercentEq => "%=", AmpAmp => "&&", PipePipe => "||",
            Nullish => "??", QuestionDot => "?.",
        }
    }
}

/// A lexeme, its decoded text, and its position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub kind: TokenKind,
    /// Raw source text for identifiers and punctuation; decoded contents for
    /// string literals.
    pub text: String,
    /// The integer value, when this literal names an exact `i64`. `None` for a
    /// fractional float, a BigInt, or an out-of-range value — the parser uses
    /// this, not the token kind, to decide whether a literal is an analytic
    /// integer index.
    pub num: Option<i64>,
    /// 1-based line / 0-based column, for diagnostics only.
    pub line: usize,
    pub col: usize,
}

impl Token {
    pub fn is_punct(&self, p: Punct) -> bool {
        self.kind == TokenKind::Punct(p)
    }

    pub fn is_keyword(&self, k: Keyword) -> bool {
        self.kind == TokenKind::Keyword(k)
    }

    /// `true` for the tokens after which a `/` is division rather than a regex:
    /// operand-ish tokens. This is the whole of the §9.2 regex disambiguation.
    pub fn is_operand_end(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Ident
                | TokenKind::Num
                | TokenKind::Str
                | TokenKind::Template
                | TokenKind::Regex
                | TokenKind::Keyword(Keyword::This)
                | TokenKind::Keyword(Keyword::Super)
                | TokenKind::Keyword(Keyword::True)
                | TokenKind::Keyword(Keyword::False)
                | TokenKind::Keyword(Keyword::Null)
                | TokenKind::Keyword(Keyword::Undefined)
        ) || matches!(
            self.kind,
            TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::PlusPlus | Punct::MinusMinus)
        )
    }

    pub fn describe(&self) -> String {
        match &self.kind {
            TokenKind::Ident => format!("identifier `{}`", self.text),
            TokenKind::Num => format!("number `{}`", self.text),
            TokenKind::Str => format!("string `{}`", self.text),
            TokenKind::Template => format!("template `{}`", self.text),
            TokenKind::Regex => format!("regex `/{}`", self.text),
            TokenKind::Keyword(k) => format!("keyword `{}`", k.as_str()),
            TokenKind::Punct(p) => format!("`{}`", p.as_str()),
            TokenKind::Eof => "end of input".into(),
            TokenKind::Bad(m) => format!("invalid token ({})", m),
        }
    }
}

/// Single-pass scanner over a whole source file.
pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    line_start: usize,
    /// Last token, consulted to disambiguate `/`.
    prev: Option<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer { src: src.as_bytes(), pos: 0, line: 1, line_start: 0, prev: None }
    }

    /// Tokenize a whole file. The result always ends with a [`TokenKind::Eof`]
    /// token, so callers can scan without a bounds check.
    pub fn tokenize(src: &str) -> Vec<Token> {
        let mut lx = Lexer::new(src);
        let mut out = Vec::with_capacity(src.len() / 4 + 8);
        loop {
            let t = lx.next_token();
            let done = t.kind == TokenKind::Eof;
            out.push(t);
            if done {
                return out;
            }
        }
    }

    fn peek(&self, k: usize) -> u8 {
        // A trailing NUL sentinel is never a JS source byte, so out-of-bounds
        // reads land in the same arm as "input finished".
        *self.src.get(self.pos + k).unwrap_or(&0)
    }

    fn bump(&mut self) -> u8 {
        let c = self.peek(0);
        if c == 0 && self.pos >= self.src.len() {
            return c;
        }
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        c
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn skip_trivia(&mut self) -> Option<String> {
        let mut bad: Option<String> = None;
        loop {
            match self.peek(0) {
                b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => {
                    self.bump();
                }
                b'/' if self.peek(1) == b'/' => {
                    while !self.at_end() && self.peek(0) != b'\n' {
                        self.bump();
                    }
                }
                b'/' if self.peek(1) == b'*' => {
                    self.bump();
                    self.bump();
                    let mut closed = false;
                    while !self.at_end() {
                        if self.peek(0) == b'*' && self.peek(1) == b'/' {
                            self.bump();
                            self.bump();
                            closed = true;
                            break;
                        }
                        self.bump();
                    }
                    if !closed {
                        bad = Some("unterminated block comment".into());
                        return bad;
                    }
                }
                _ => return bad,
            }
        }
    }

    /// A `/` opens a regex literal only where an expression may begin. After an
    /// operand it is division — the usual JS disambiguation, and the sole reason
    /// the scanner remembers `prev`.
    pub fn regex_allowed(&self) -> bool {
        match &self.prev {
            None => true,
            Some(t) => !t.is_operand_end(),
        }
    }

    /// Scan the next token.
    pub fn next_token(&mut self) -> Token {
        let trivia_err = self.skip_trivia();
        let line = self.line;
        let col = self.pos - self.line_start;
        if self.at_end() {
            let kind = match trivia_err {
                Some(m) => TokenKind::Bad(m),
                None => TokenKind::Eof,
            };
            let t = Token { kind, text: String::new(), num: None, line, col };
            self.prev = Some(t.clone());
            return t;
        }
        if let Some(m) = trivia_err {
            let t = Token { kind: TokenKind::Bad(m), text: String::new(), num: None, line, col };
            self.prev = Some(t.clone());
            return t;
        }
        let t = self.scan(line, col);
        self.prev = Some(t.clone());
        t
    }

    fn scan(&mut self, line: usize, col: usize) -> Token {
        let c = self.peek(0);
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            return self.ident(line, col);
        }
        if c.is_ascii_digit() || (c == b'.' && self.peek(1).is_ascii_digit()) {
            return self.number(line, col);
        }
        if c == b'\'' || c == b'"' {
            return self.string(c, line, col);
        }
        if c == b'`' {
            return self.template(line, col);
        }
        if c == b'/' && self.regex_allowed() {
            return self.regex(line, col);
        }
        self.punct(line, col)
    }

    fn str_from(&self, start: usize) -> String {
        // The source was a `&str`, so every UTF-8 boundary we can land on while
        // scanning ASCII-delimited lexemes is a valid prefix of it.
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn ident(&mut self, line: usize, col: usize) -> Token {
        let start = self.pos;
        while matches!(self.peek(0), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$') {
            self.bump();
        }
        let text = self.str_from(start);
        let kind = match Keyword::from_str(&text) {
            Some(k) => TokenKind::Keyword(k),
            None => TokenKind::Ident,
        };
        Token { kind, text, num: None, line, col }
    }

    /// `0x`/`0b`/`0o` prefixes, `_` separators, decimal points, exponents and
    /// the BigInt suffix (§9.2). The literal's *text* is kept verbatim so an
    /// evidence dump can quote the source; `num` holds the decoded integer.
    fn number(&mut self, line: usize, col: usize) -> Token {
        let start = self.pos;
        let mut big_int = false;
        let mut fractional = false;

        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X' | b'b' | b'B' | b'o' | b'O') {
            self.bump();
            self.bump();
            while matches!(self.peek(0), b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'_') {
                self.bump();
            }
        } else {
            while matches!(self.peek(0), b'0'..=b'9' | b'_') {
                self.bump();
            }
            if self.peek(0) == b'.' {
                fractional = true;
                self.bump();
                while matches!(self.peek(0), b'0'..=b'9' | b'_') {
                    self.bump();
                }
            }
            if matches!(self.peek(0), b'e' | b'E') {
                let save = self.pos;
                self.bump();
                if matches!(self.peek(0), b'+' | b'-') {
                    self.bump();
                }
                if matches!(self.peek(0), b'0'..=b'9') {
                    fractional = true;
                    while matches!(self.peek(0), b'0'..=b'9' | b'_') {
                        self.bump();
                    }
                } else {
                    // `1e` names no exponent; rewind so the `e` lexes as an
                    // identifier instead of being swallowed.
                    self.pos = save;
                }
            }
        }

        if self.peek(0) == b'n' {
            big_int = true;
            self.bump();
        }

        let raw = self.str_from(start);
        let num = decode_num(&raw, big_int, fractional);
        Token { kind: TokenKind::Num, text: raw, num, line, col }
    }

    /// `'...'` / `"..."` with the escape set the analyzable subset needs.
    fn string(&mut self, quote: u8, line: usize, col: usize) -> Token {
        self.bump();
        let mut out = String::new();
        loop {
            if self.at_end() {
                return Token { kind: TokenKind::Bad("unterminated string".into()), text: out, num: None, line, col };
            }
            let c = self.bump();
            if c == quote {
                return Token { kind: TokenKind::Str, text: out, num: None, line, col };
            }
            if c == b'\\' {
                if self.at_end() {
                    return Token { kind: TokenKind::Bad("unterminated string".into()), text: out, num: None, line, col };
                }
                match self.bump() {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'0' => out.push('\0'),
                    b'\\' => out.push('\\'),
                    b'\'' => out.push('\''),
                    b'"' => out.push('"'),
                    b'`' => out.push('`'),
                    b'\n' => {}
                    other => out.push(other as char),
                }
            } else {
                out.push(c as char);
            }
        }
    }

    /// `` `...` `` — the raw span is kept. Template *interpolation* is outside
    /// the analyzable subset (§9.14 lists only `String.length`), but the lexeme
    /// must still tokenize so the parser rejects it at the right place.
    fn template(&mut self, line: usize, col: usize) -> Token {
        self.bump(); // opening backtick
        let start = self.pos;
        loop {
            if self.at_end() {
                let text = self.str_from(start);
                return Token { kind: TokenKind::Bad("unterminated template literal".into()), text, num: None, line, col };
            }
            // The delimiter is checked *before* the bump so the captured span
            // excludes the closing backtick.
            match self.peek(0) {
                b'\\' => {
                    self.bump();
                    self.bump();
                }
                b'`' => {
                    let text = self.str_from(start);
                    self.bump();
                    return Token { kind: TokenKind::Template, text, num: None, line, col };
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// `/pattern/flags`. A `[...]` class may contain `/`, which is why the
    /// scanner tracks class nesting instead of stopping at the second slash.
    fn regex(&mut self, line: usize, col: usize) -> Token {
        self.bump(); // opening slash
        let start = self.pos;
        let mut in_class = false;
        loop {
            if self.at_end() {
                let text = self.str_from(start);
                return Token { kind: TokenKind::Bad("unterminated regex literal".into()), text, num: None, line, col };
            }
            if self.peek(0) == b'\\' {
                self.bump();
                self.bump();
                continue;
            }
            if self.peek(0) == b'\n' && !in_class {
                let text = self.str_from(start);
                return Token { kind: TokenKind::Bad("newline in regex literal".into()), text, num: None, line, col };
            }
            // The closing slash is tested before the bump so the captured body
            // excludes it, exactly as in `template`.
            if self.peek(0) == b'/' && !in_class {
                let text = self.str_from(start);
                self.bump();
                while matches!(self.peek(0), b'a'..=b'z' | b'A'..=b'Z') {
                    self.bump();
                }
                return Token { kind: TokenKind::Regex, text, num: None, line, col };
            }
            match self.bump() {
                b'[' => in_class = true,
                b']' => in_class = false,
                _ => {}
            }
        }
    }

    fn punct(&mut self, line: usize, col: usize) -> Token {
        // Longest match first, so `===` never lexes as `==` then `=`.
        const MULTI: &[(Punct, &str)] = &[
            (Punct::UShiftRight, ">>>"),
            (Punct::EqEqEq, "==="),
            (Punct::NeNe, "!=="),
            (Punct::Ellipsis, "..."),
            (Punct::ShiftLeft, "<<"),
            (Punct::ShiftRight, ">>"),
            (Punct::EqEq, "=="),
            (Punct::Ne, "!="),
            (Punct::Le, "<="),
            (Punct::Ge, ">="),
            (Punct::PlusPlus, "++"),
            (Punct::MinusMinus, "--"),
            (Punct::PlusEq, "+="),
            (Punct::MinusEq, "-="),
            (Punct::StarEq, "*="),
            (Punct::SlashEq, "/="),
            (Punct::PercentEq, "%="),
            (Punct::AmpAmp, "&&"),
            (Punct::PipePipe, "||"),
            (Punct::Nullish, "??"),
            (Punct::QuestionDot, "?."),
            (Punct::Arrow, "=>"),
            (Punct::StarStar, "**"),
        ];
        for (p, form) in MULTI {
            if self.match_str(form) {
                self.pos += form.len();
                return Token { kind: TokenKind::Punct(*p), text: form.to_string(), num: None, line, col };
            }
        }
        const SINGLE: &[(u8, Punct)] = &[
            (b'(', Punct::LParen),
            (b')', Punct::RParen),
            (b'{', Punct::LBrace),
            (b'}', Punct::RBrace),
            (b'[', Punct::LBracket),
            (b']', Punct::RBracket),
            (b';', Punct::Semi),
            (b',', Punct::Comma),
            (b':', Punct::Colon),
            (b'?', Punct::Question),
            (b'.', Punct::Dot),
            (b'+', Punct::Plus),
            (b'-', Punct::Minus),
            (b'*', Punct::Star),
            (b'/', Punct::Slash),
            (b'%', Punct::Percent),
            (b'&', Punct::Amp),
            (b'|', Punct::Pipe),
            (b'^', Punct::Caret),
            (b'~', Punct::Tilde),
            (b'!', Punct::Bang),
            (b'<', Punct::Lt),
            (b'>', Punct::Gt),
            (b'=', Punct::Eq),
        ];
        for (c, p) in SINGLE {
            if self.peek(0) == *c {
                self.bump();
                return Token { kind: TokenKind::Punct(*p), text: (*c as char).to_string(), num: None, line, col };
            }
        }
        let bad = self.bump();
        Token {
            kind: TokenKind::Bad(format!("unexpected character {:?}", bad as char)),
            text: String::new(),
            num: None,
            line,
            col,
        }
    }

    fn match_str(&self, s: &str) -> bool {
        let b = s.as_bytes();
        self.pos + b.len() <= self.src.len() && self.src[self.pos..self.pos + b.len()] == *b
    }
}

/// Decode a numeric literal's text to an exact `i64`, or `None` when it names no
/// integer. A BigInt is lexically a number but is not an analytic integer index
/// (§9.2 lists the suffix, §9.14 keeps BigInt out of the subset), so it decodes
/// to `None` rather than to a truncated `i64`.
pub fn decode_num(raw: &str, big_int: bool, fractional: bool) -> Option<i64> {
    if big_int || raw.is_empty() {
        return None;
    }
    let cleaned: String = raw.chars().filter(|&c| c != '_').collect();
    let lower = cleaned.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        return i64::from_str_radix(hex, 16).ok();
    }
    if let Some(bin) = lower.strip_prefix("0b") {
        return i64::from_str_radix(bin, 2).ok();
    }
    if let Some(oct) = lower.strip_prefix("0o") {
        return i64::from_str_radix(oct, 8).ok();
    }
    if !fractional {
        return cleaned.parse::<i64>().ok();
    }
    // `1e3` and `1.5e3` name integers; `1.5` does not. Going through `f64` is
    // exact for the magnitudes an array index can use.
    let f: f64 = cleaned.parse().ok()?;
    if f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
        Some(f as i64)
    } else {
        None
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}:{}", self.describe(), self.line, self.col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        Lexer::tokenize(src).into_iter().map(|t| t.kind).collect()
    }

    /// Every §9.2 keyword must reach the parser as a `Keyword`, not as an
    /// `Ident`: a keyword seen as an identifier silently parses as a variable
    /// reference instead of the construct it names.
    #[test]
    fn section_9_2_keywords_all_lex_as_keywords() {
        let src = "let const class extends super import export yield await async \
                   of in instanceof typeof void delete";
        let ks = kinds(src);
        let n = ks.iter().filter(|k| matches!(k, TokenKind::Keyword(_))).count();
        assert_eq!(n, 16, "expected 16 keywords, got {ks:?}");
    }

    #[test]
    fn control_and_decl_keywords_lex_as_keywords() {
        for w in ["function", "return", "if", "else", "while", "do", "for", "var",
                  "new", "delete", "this", "true", "false", "null", "undefined",
                  "throw", "try", "catch", "finally", "switch", "case", "default",
                  "break", "continue"] {
            assert_eq!(
                Lexer::tokenize(w)[0].kind,
                TokenKind::Keyword(Keyword::from_str(w).unwrap()),
                "{w} must lex as its keyword"
            );
        }
    }

    #[test]
    fn numeric_separators_and_exponents() {
        assert_eq!(Lexer::tokenize("1_000_000")[0].num, Some(1_000_000));
        assert_eq!(Lexer::tokenize("1e3")[0].num, Some(1000));
        assert_eq!(Lexer::tokenize("1.5e3")[0].num, Some(1500));
        assert_eq!(Lexer::tokenize("1e-2")[0].num, None, "0.01 names no integer");
        assert_eq!(Lexer::tokenize("0x10")[0].num, Some(16));
        assert_eq!(Lexer::tokenize("0b101")[0].num, Some(5));
        assert_eq!(Lexer::tokenize("0o17")[0].num, Some(15));
    }

    #[test]
    fn fractional_literal_is_not_an_integer_index() {
        let t = &Lexer::tokenize("1.5")[0];
        assert_eq!(t.kind, TokenKind::Num);
        assert_eq!(t.num, None);
    }

    /// §9.2 requires the BigInt suffix to *lex*. §9.14 keeps it out of the
    /// analyzable subset, so it must not decode to an `i64`: a `42n` that
    /// silently became `42` would let the analyzer reason about a value the
    /// program never had.
    #[test]
    fn bigint_lexes_but_does_not_decode_to_i64() {
        let t = &Lexer::tokenize("42n")[0];
        assert_eq!(t.kind, TokenKind::Num);
        assert_eq!(t.num, None);
    }

    #[test]
    fn slash_is_regex_only_where_an_expression_may_begin() {
        assert!(kinds("let r = /ab+c/g;").contains(&TokenKind::Regex));
        assert_eq!(kinds("return /x/")[1], TokenKind::Regex);
        // After an operand the same character is division.
        assert_eq!(kinds("a / b")[1], TokenKind::Punct(Punct::Slash));
        assert_eq!(kinds("f(x) / 2")[4], TokenKind::Punct(Punct::Slash));
        assert_eq!(kinds("A[0] / 2")[4], TokenKind::Punct(Punct::Slash));
        assert_eq!(kinds("i++ / 2")[2], TokenKind::Punct(Punct::Slash));
    }

    /// A `/` inside a `[...]` class is data, not the end of the literal.
    #[test]
    fn regex_class_may_contain_a_slash() {
        assert_eq!(kinds("x = /[a/]/")[2], TokenKind::Regex);
    }

    #[test]
    fn template_literals_lex_as_one_token() {
        let ts = Lexer::tokenize("`hi there`");
        assert_eq!(ts[0].kind, TokenKind::Template);
        assert_eq!(ts[0].text, "hi there");
        assert_eq!(ts.len(), 2, "template ends at its backtick");
    }

    #[test]
    fn string_escapes_are_decoded() {
        assert_eq!(Lexer::tokenize(r#"'a\tb\n'"#)[0].text, "a\tb\n");
        assert_eq!(Lexer::tokenize(r#""x\"y""#)[0].text, "x\"y");
    }

    #[test]
    fn unterminated_lexemes_are_bad_and_never_panic() {
        for src in ["\"abc", "'abc", "`abc", "/abc", "/* unclosed", "let x = @"] {
            let ks = kinds(src);
            assert!(ks.iter().any(|k| matches!(k, TokenKind::Bad(_))), "{src:?} -> {ks:?}");
        }
    }

    #[test]
    fn longest_match_wins() {
        assert_eq!(kinds("a === b")[1], TokenKind::Punct(Punct::EqEqEq));
        assert_eq!(kinds("a !== b")[1], TokenKind::Punct(Punct::NeNe));
        assert_eq!(kinds("a >>> b")[1], TokenKind::Punct(Punct::UShiftRight));
        assert_eq!(kinds("a ?? b")[1], TokenKind::Punct(Punct::Nullish));
        assert_eq!(kinds("a?.b")[1], TokenKind::Punct(Punct::QuestionDot));
        assert_eq!(kinds("(x) => y")[3], TokenKind::Punct(Punct::Arrow));
        assert_eq!(kinds("2 ** 3")[1], TokenKind::Punct(Punct::StarStar));
    }

    #[test]
    fn comments_are_trivia_not_tokens() {
        assert_eq!(kinds("// hi\nlet x = 1;")[0], TokenKind::Keyword(Keyword::Let));
        assert_eq!(kinds("/* a\nb */ x")[0], TokenKind::Ident);
        assert_eq!(kinds("/* x */")[0], TokenKind::Eof);
    }

    #[test]
    fn positions_track_lines_and_columns() {
        let ts = Lexer::tokenize("let\n  x = 1;");
        assert_eq!(ts[0].line, 1);
        assert_eq!(ts[1].line, 2);
        assert_eq!(ts[1].col, 2);
    }

    #[test]
    fn token_stream_always_ends_in_eof() {
        for src in ["", "let", "let x = 1;", "}}}{{{", "@@@"] {
            let ks = kinds(src);
            assert_eq!(ks.last(), Some(&TokenKind::Eof), "{src:?}");
        }
    }

    #[test]
    fn decode_num_rejects_overflow() {
        assert_eq!(decode_num("99999999999999999999999", false, false), None);
        assert_eq!(decode_num("9223372036854775807", false, false), Some(i64::MAX));
    }
}


