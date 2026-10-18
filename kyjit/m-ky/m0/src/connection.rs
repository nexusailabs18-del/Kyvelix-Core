//! Connection Layer for M0 Dependence Decision Engine
//!
//! This module provides a simple, clean API for integrating the M0 engine
//! into other systems. It wraps the internal IR and decision pipeline
//! with ergonomic builder patterns and common use-case helpers.

use crate::ir::*;
use crate::decide::{decide, Decision};
use crate::certificate::Certificate;
use crate::frontend::{parse, lower, Env};

/// Simple configuration for the decision engine
#[derive(Debug, Clone, Default)]
pub struct DecideConfig {
    /// Maximum phases to run (None = all phases)
    pub max_phase: Option<u8>,
    /// Enable certificate emission
    pub emit_certificate: bool,
    /// Timeout in milliseconds (None = no timeout)
    pub timeout_ms: Option<u64>,
    /// Verbose logging
    pub verbose: bool,
}

/// Result of a dependence query
#[derive(Debug, Clone)]
pub struct DependenceResult {
    /// The decision
    pub decision: Decision,
    /// Optional certificate
    pub certificate: Option<Certificate>,
    /// Which phase decided
    pub decided_at_phase: String,
    /// Time taken in microseconds
    pub elapsed_us: u64,
}

/// Builder for constructing a Loop IR programmatically
pub struct LoopBuilder {
    depth: usize,
    ivs: Vec<IV>,
    body: Vec<Access>,
    scalars: Vec<ScalarVar>,
    stmts: Option<Vec<LoopStmt>>,
}

impl LoopBuilder {
    /// Create a new loop builder with given depth
    pub fn new(depth: usize) -> Self {
        Self {
            depth,
            ivs: Vec::new(),
            body: Vec::new(),
            scalars: Vec::new(),
            stmts: None,
        }
    }

    /// Add an induction variable
    pub fn iv(mut self, name: impl Into<String>, lower: i64, upper: i64, step: i64) -> Self {
        self.ivs.push(IV::new(name, lower, upper, step));
        self
    }

    /// Add a scalar variable
    pub fn scalar(mut self, name: impl Into<String>, ty: ScalarTy) -> Self {
        self.scalars.push(ScalarVar { name: name.into(), ty });
        self
    }

    /// Add a scalar assignment statement
    pub fn stmt(mut self, target: &str, expr: ScalarExpr) -> Self {
        let target_idx = self.scalars.iter().position(|s| s.name == target).unwrap_or_else(|| {
            self.scalars.push(ScalarVar { name: target.into(), ty: ScalarTy::I64 });
            self.scalars.len() - 1
        });
        let stmt = LoopStmt {
            id: self.stmts.as_ref().map(|s| s.len()).unwrap_or(0),
            kind: StmtKind::Assign { target: target_idx, expr },
        };
        self.stmts.get_or_insert_with(Vec::new).push(stmt);
        self
    }

    /// Add a memory access (read or write)
    pub fn access(
        mut self,
        kind: AccessKind,
        base: impl Into<String>,
        index: Vec<i64>,
        offset: OffsetValue,
        elem_size: u64,
    ) -> Self {
        let mut access = Access {
            kind,
            base: base.into(),
            index,
            elem_size: elem_size as i64,
            offset,
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        };
        self.body.push(access);
        self
    }

    /// Add an access with an expression string (will be normalized by M2g)
    pub fn access_expr(
        mut self,
        kind: AccessKind,
        base: impl Into<String>,
        expr: impl Into<String>,
        elem_size: u64,
    ) -> Self {
        let mut access = Access {
            kind,
            base: base.into(),
            index: vec![],
            elem_size: elem_size as i64,
            offset: OffsetValue::Const(0),
            expr: Some(expr.into()),
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        };
        self.body.push(access);
        self
    }

    /// Build the Loop IR
    pub fn build(self) -> Loop {
        Loop {
            depth: self.depth,
            ivs: self.ivs,
            body: self.body,
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: self.scalars,
            stmts: self.stmts,
        }
    }
}
/// High-level API for dependence queries
pub struct DependenceEngine {
    config: DecideConfig,
}

impl DependenceEngine {
    /// Create a new engine with default config
    pub fn new() -> Self {
        Self { config: DecideConfig::default() }
    }

    /// Create with custom config
    pub fn with_config(config: DecideConfig) -> Self {
        Self { config }
    }

    /// Query dependence between two accesses in a loop
    pub fn query(&self, loop_: &Loop, access_p: usize, access_q: usize) -> DependenceResult {
        let start = std::time::Instant::now();
        let (decision, certificate) = decide(loop_, access_p, access_q);
        let elapsed_us = start.elapsed().as_micros() as u64;

        DependenceResult {
            decision,
            certificate: if self.config.emit_certificate { certificate } else { None },
            decided_at_phase: "M4".to_string(),
            elapsed_us,
        }
    }

    /// Query from source code string
    pub fn query_source(&self, source: &str, access_p: usize, access_q: usize) -> Result<DependenceResult, String> {
        let env = Env::default();
        let loops = lower(source, &env).map_err(|e| format!("Lower error: {:?}", e))?;
        if loops.is_empty() {
            return Err("No loops found in source".to_string());
        }
        Ok(self.query(&loops[0], access_p, access_q))
    }

    /// Query using the builder API
    pub fn query_builder(&self, builder: LoopBuilder, access_p: usize, access_q: usize) -> DependenceResult {
        let loop_ = builder.build();
        self.query(&loop_, access_p, access_q)
    }
}

impl Default for DependenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple predicate helpers for common queries
pub mod predicates {
    use super::*;

    /// Check if two accesses are proven independent
    pub fn is_independent(result: &DependenceResult) -> bool {
        matches!(result.decision, Decision::None)
    }

    /// Check if two accesses have a proven dependence
    pub fn is_dependent(result: &DependenceResult) -> bool {
        matches!(result.decision, Decision::Exact(_))
    }

    /// Check if the engine could not decide
    pub fn is_unknown(result: &DependenceResult) -> bool {
        matches!(result.decision, Decision::Unknown(_))
    }

    /// Get distance vector if exact dependence
    pub fn distance(result: &DependenceResult) -> Option<&Vec<i64>> {
        match &result.decision {
            Decision::Exact(dist) => Some(dist),
            _ => None,
        }
    }
}

/// Batch query multiple access pairs
pub fn query_batch(loop_: &Loop, pairs: &[(usize, usize)]) -> Vec<DependenceResult> {
    let engine = DependenceEngine::new();
    pairs.iter()
        .map(|(p, q)| engine.query(loop_, *p, *q))
        .collect()
}

/// Quick one-liner for common case: single loop, two accesses from source
pub fn quick_decide(source: &str, access_p: usize, access_q: usize) -> Result<DependenceResult, String> {
    DependenceEngine::new().query_source(source, access_p, access_q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let loop_ = LoopBuilder::new(1)
            .iv("i", 0, 100, 1)
            .access(AccessKind::Write, "A", vec![1], OffsetValue::Const(0), 8)
            .access(AccessKind::Read, "A", vec![1], OffsetValue::Const(0), 8)
            .build();

        let engine = DependenceEngine::new();
        let result = engine.query(&loop_, 0, 1);
        assert!(predicates::is_dependent(&result));
    }

    #[test]
    fn test_builder_no_dep() {
        let loop_ = LoopBuilder::new(1)
            .iv("i", 0, 100, 1)
            .access(AccessKind::Write, "A", vec![2], OffsetValue::Const(0), 8)
            .access(AccessKind::Read, "A", vec![2], OffsetValue::Const(1), 8)
            .build();

        let engine = DependenceEngine::new();
        let result = engine.query(&loop_, 0, 1);
        assert!(predicates::is_independent(&result));
    }

    #[test]
    fn test_quick_decide() {
        let src = r#"
        for (let i = 0; i < 100; i++) {
            A[i] = B[i];
            C[i] = A[i];
        }
        "#;
        let result = quick_decide(src, 0, 1).unwrap();
        assert!(predicates::is_dependent(&result));
    }

    #[test]
    fn test_independent() {
        let src = r#"
        for (let i = 0; i < 100; i++) {
            A[i] = 0;
            B[i] = 0;
        }
        "#;
        let result = quick_decide(src, 0, 1).unwrap();
        assert!(predicates::is_independent(&result));
    }
}