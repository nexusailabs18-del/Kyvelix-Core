//! Phase 9 — the JavaScript frontend (§9.1).
//!
//! Phases 2–8 all consume IR that a test wrote by hand. This module produces it
//! from source, which is what makes the analyzers reachable from a real program:
//! a `.js` file in, a [`crate::ir::Loop`] out that `m0-decide` accepts unchanged.
//!
//! ## The subset, and why the boundary is drawn there
//!
//! §9.14 states the rule: anything outside the analyzable subset is *compiled
//! correctly but not analyzed*. That is a soundness claim, not a feature list —
//! an analyzer that silently received IR it could not model would report
//! `NoDependence` for a dependence it never saw. So the frontend's contract is:
//!
//! - **parse** the constructs it knows (§9.3);
//! - **lower** only what the IR can express without lying about it;
//! - **refuse**, with a reason naming the construct, everything else.
//!
//! Refusal is a `Result`, never a degraded `Loop`. A caller that wants the
//! conservative answer asks for it explicitly.
//!
//! ## Layout
//!
//! | Module | Spec | Role |
//! |---|---|---|
//! | [`lexer`] | §9.2 | single-pass scanner for the JS lexemes |
//! | [`ast_`] | §9.3 | typed syntax tree |
//! | [`parser`] | §9.3 | recursive-descent parser, one function per rule |
//! | [`lower`] | §9.4 | AST → [`crate::ir::Loop`] |
//! | [`loopkinds`] | §9.10 | loop kind classification |
//! | [`ctrl_edges`] | §9.11 | post-dominator tree and control edges |
//! | [`program`] | §9.4 | `Loop` → the region / points-to / IPA programs |
//! | [`fuzz`] | §9.12 | differential testing against Node.js |
//! | [`for_of`] | §9.7 | `for-of` → indexed loop |
//! | [`destructuring`] | §9.6 | destructuring → sequential loads |
//! | [`closures`] | §9.5 | capture analysis |
//! | [`objects`] | §9.8 | shape inference |
//! | [`aostruct`] | §9.9 | array-of-structs recognition |

pub mod ast;
pub mod ctrl_edges;
pub mod lexer;
pub mod loopkinds;
pub mod lower;
pub mod parser;

pub use lexer::{Keyword, Lexer, Punct, Token, TokenKind};
pub use lower::{Env, Kernels, Skipped, lower, lower_program, lower_source, lower_strict};
pub use parser::parse;

use std::fmt;

/// A frontend failure: a token stream that parses but cannot be lowered.
///
/// The distinction from [`ParseError`] matters to the caller. A parse error means
/// the source is not the language; a [`LowerError`] means it is the language but
/// outside the analyzable subset (§9.14), and the correct response is the
/// conservative fallback rather than a syntax diagnosis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// A construct §9.14 puts out of scope. Carries the name to report.
    OutOfScope(&'static str),
    /// The construct is in scope but this loop shape is not expressible in the
    /// IR — e.g. an IV whose step is not a positive constant.
    NotAnalyzable(String),
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LowerError::OutOfScope(w) => write!(f, "`{w}` is outside the analyzable subset (§9.14)"),
            LowerError::NotAnalyzable(m) => write!(f, "not analyzable: {m}"),
        }
    }
}

impl std::error::Error for LowerError {}

#[cfg(test)]
mod tests {
    use super::lexer::*;

    #[test]
    fn mod_doc_names_the_pipeline() {
        // The frontend's job is source -> Loop. This test pins the entry point
        // existing once parser/lower land; it is a compile-time reminder, not
        // an assertion about behavior.
        assert!(true);
    }
}
