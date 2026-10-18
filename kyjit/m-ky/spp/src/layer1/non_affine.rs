//! SPP Layer 1 — Non-Affine Dependence (Gray Zone) (stub for independent build)
//!
//! Range-based runtime disambiguation for non-affine accesses.
//! Handles array-of-structs, indirect indexing (A[B[i]]), pointer chasing.
//!
//! This is the "middle tier" between full affine analysis and full speculation.

use crate::layer1::types::{DependenceType, RuntimeTest, NonAffineDependence};
use crate::common::LoopStub;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A non-affine access descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonAffineAccess {
    pub access_idx: usize,
    pub base: String,
    pub index_kind: NonAffineIndexKind,
    pub elem_size: i64,
    pub offset: i64,
}

/// Kinds of non-affine index expressions we can handle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NonAffineIndexKind {
    /// Indirect indexing: A[B[i]] where B is known
    Indirect {
        indirect_base: String,
        indirect_index: Vec<i64>,
        indirect_offset: i64,
    },
    /// Modulo indexing: A[i % C]
    Modulo {
        iv: usize,
        modulus: i64,
    },
    /// Min/Max clamping: A[min(i, K)] or A[max(i, K)]
    Clamp {
        iv: usize,
        bound: i64,
        is_min: bool,
    },
    /// Symbolic range: index in [lo_expr, hi_expr]
    SymbolicRange {
        lo: String,
        hi: String,
    },
    /// Pointer chase: A[ptr] where ptr follows a known chain
    PointerChase {
        chain_var: String,
        table: Vec<i64>,
        init: i64,
        max_steps: usize,
    },
}

/// Range of possible indices for an access
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRange {
    pub lo: i64,
    pub hi: i64,
    pub stride: i64,
    pub is_exact: bool,
}

/// Non-affine dependence analyzer (stub)
pub struct NonAffineAnalyzer {
    non_affine_accesses: Vec<NonAffineAccess>,
    range_cache: HashMap<usize, IndexRange>,
}

impl NonAffineAnalyzer {
    pub fn new(_loop_: LoopStub) -> Self {
        Self {
            non_affine_accesses: Vec::new(),
            range_cache: HashMap::new(),
        }
    }

    /// Compute the index range for a non-affine access
    pub fn compute_range(&mut self, _access_idx: usize) -> IndexRange {
        // Stub implementation
        IndexRange {
            lo: i64::MIN / 2,
            hi: i64::MAX / 2,
            stride: 1,
            is_exact: false,
        }
    }

    /// Check if two non-affine accesses are provably disjoint
    pub fn check_disjoint(&mut self, _p: usize, _q: usize) -> DisjointResult {
        DisjointResult::Unknown
    }

    /// Generate a runtime check for a dependence that couldn't be proven disjoint
    pub fn generate_runtime_check(&mut self, _p: usize, _q: usize) -> Option<RuntimeCheck> {
        None
    }
}

/// Result of disjointness check
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisjointResult {
    Disjoint,
    Overlapping,
    Unknown,
}

/// Runtime check specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeCheck {
    RangeCheck {
        base: String,
        lo: i64,
        hi: i64,
    },
    VersionCheck {
        base: String,
        version: u64,
    },
    PointerEquality {
        base1: String,
        base2: String,
    },
    ModuloCheck {
        base: String,
        modulus: i64,
    },
}

/// Non-affine dependence result
// Re-exported from crate::layer1::types

/// Analyze non-affine dependences for a loop (stub)
pub fn analyze_non_affine_dependences(_loop_: &LoopStub) -> Vec<NonAffineDependence> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_affine_stub() {
        let loop_ = LoopStub::new(1);
        let deps = analyze_non_affine_dependences(&loop_);
        assert!(deps.is_empty());
    }
}