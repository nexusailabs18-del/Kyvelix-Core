//! SPP Layer 1 — Dependence Engine types
//!
//! This module defines the core dependence types that flow from UDDP
//! through SPP Layer 1 and into Layer 2 (Parallel Version Generation).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The ten dependence types SPP Layer 1 produces (from spp/work.md §2.1)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependenceType {
    /// Distance is exactly d — wavefront with step d
    Exact(Vec<i64>),
    /// Distance is at most d_max — wavefront with d_max
    Bounded(Vec<i64>),
    /// Distance is a function of parameters — specialize at runtime
    Parametric(String), // Parameter expression
    /// Dependence exists only if guard holds — runtime check
    Conditional(String), // Guard expression
    /// Recognized reduction — private partial states
    Reduction {
        op: ReductionKind,
        var: usize,
    },
    /// Recognized recurrence — prefix scan
    Recurrence {
        scan_type: ScanType,
        var: usize,
    },
    /// No dependence — fully independent
    None,
    /// May depend — speculate
    MayDepend,
    /// Must depend — serial
    MustDepend,
    /// Cheap runtime test decides — version B
    RuntimeCheckable {
        test: RuntimeTest,
    },
    /// Genuinely undecidable — speculate or serial
    Unknown,
}

/// Reduction kinds recognized by Layer 1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReductionKind {
    Sum,
    Product,
    Min,
    Max,
    And,
    Or,
    Xor,
    /// User-defined associative-commutative operation
    UserDefined {
        name: String,
        identity: i64,
    },
}

/// Scan types for recurrences
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanType {
    Inclusive,
    Exclusive,
    Segmented,
}

/// Runtime test kinds for Tier 1/2 speculation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeTest {
    /// Index range check: index in [lo, hi]
    RangeCheck {
        base: String,
        lo: i64,
        hi: i64,
    },
    /// Version check: array version unchanged
    VersionCheck {
        base: String,
        version: u64,
    },
    /// Pointer equality check
    PointerEquality {
        base1: String,
        base2: String,
    },
    /// Modulo check: index % m == 0
    ModuloCheck {
        base: String,
        modulus: i64,
    },
}

/// A dependence edge between two accesses
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependenceEdge {
    pub src: usize,      // Source access index
    pub dst: usize,      // Destination access index
    pub dep_type: DependenceType,
    pub level: usize,    // Loop level carrying the dependence
    pub distance: Vec<i64>, // Distance vector
    pub direction: Vec<Direction>, // Direction vector
}

/// Direction vector entries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Lt,   // <
    Eq,   // =
    Gt,   // >
    Ne,   // !=
    Unknown,
}

impl Direction {
    pub fn of_delta(delta: i64) -> Direction {
        match delta.cmp(&0) {
            std::cmp::Ordering::Less => Direction::Lt,
            std::cmp::Ordering::Equal => Direction::Eq,
            std::cmp::Ordering::Greater => Direction::Gt,
        }
    }
}

/// Memory ordering kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryOrder {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

/// Atomic operation kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtomicOp {
    Load,
    Store,
    Add,
    Sub,
    And,
    Or,
    Xor,
    CompareExchange,
    Exchange,
    FetchAdd,
    FetchSub,
    FetchAnd,
    FetchOr,
    FetchXor,
}

/// Non-affine dependence result (minimal for cross-module use)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonAffineDependence {
    pub src: usize,
    pub dst: usize,
    pub dep_type: DependenceType,
    pub runtime_check: Option<String>, // Simplified
}

/// Dependence with memory ordering constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedDependence {
    pub src: usize,
    pub dst: usize,
    pub dep_type: DependenceType,
    pub src_order: MemoryOrder,
    pub dst_order: MemoryOrder,
    pub requires_fence: bool,
    pub fence_order: Option<MemoryOrder>,
}

/// Result of parallelizability check under memory model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParallelizationResult {
    Parallelizable,
    RequiresSeqCstFence,
    DataRace,
    Unknown,
}

/// Complete dependence analysis result for a loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependenceAnalysis {
    pub loop_hash: String,
    pub edges: Vec<DependenceEdge>,
    pub loop_kind: LoopKind,
    pub reductions: Vec<ReductionInfo>,
    pub recurrences: Vec<RecurrenceInfo>,
    pub alias_info: AliasInfo,
    pub effect_summary: EffectSummary,
    pub cross_loop_deps: Vec<CrossLoopDependence>,
    pub non_affine_deps: Vec<NonAffineDependence>,
}

/// Loop kind classification (Wolfe Ch. 9)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopKind {
    Doall,    // No dependences — fully parallel
    Dopar,    // Resolvable conflicts — parallel with anti-dep resolution
    Forall,   // Negative distances possible — parallel with RHS-first
    Sequential, // Must run sequentially
    Unknown,
}

/// Reduction information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReductionInfo {
    pub var: usize,
    pub kind: ReductionKind,
    pub loop_level: usize,
    pub access_indices: Vec<usize>,
}

/// Recurrence information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurrenceInfo {
    pub var: usize,
    pub scan_type: ScanType,
    pub loop_level: usize,
    pub access_indices: Vec<usize>,
}

/// Alias analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasInfo {
    pub may_alias: Vec<(String, String)>,      // Pairs that may alias
    pub must_alias: Vec<(String, String)>,     // Pairs that must alias
    pub no_alias: Vec<(String, String)>,       // Pairs that cannot alias
    pub disjoint_arrays: Vec<(String, String)>, // Proven disjoint
}

/// Effect summary for a loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectSummary {
    pub reads: Vec<String>,     // Arrays read
    pub writes: Vec<String>,    // Arrays written
    pub allocates: Vec<String>, // Allocations
    pub throws: bool,           // May throw
    pub blocks: bool,           // May block (I/O, sync)
    pub atomic: bool,           // Has atomic operations
    pub io: bool,               // Has I/O
    pub unknown: bool,          // Unknown effects
}

/// Cross-loop dependence (for fusion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossLoopDependence {
    pub first_loop_hash: String,
    pub second_loop_hash: String,
    pub distance: Vec<i64>,
    pub dep_type: DependenceType,
}

/// Configuration for Layer 1 analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer1Config {
    /// Enable interprocedural alias analysis
    pub enable_ipa: bool,
    /// Enable non-affine gray zone analysis
    pub enable_non_affine: bool,
    /// Enable reduction generalization
    pub enable_reductions: bool,
    /// Enable cross-iteration alias speculation
    pub enable_cross_iter_speculation: bool,
    /// Enable memory-model awareness
    pub enable_memory_model: bool,
    /// Enable library effect summaries
    pub enable_library_effects: bool,
    /// Maximum contexts for context-sensitive analysis
    pub max_contexts: usize,
    /// Enumeration caps (soundness boundaries)
    pub enum_caps: EnumCaps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumCaps {
    pub m6_products: usize,      // Default: 4000
    pub m7_hash_domain: usize,   // Default: 65536
    pub m8_recurrence_domain: usize, // Default: 4000
    pub m10_fixed_point: usize,  // Default: 65536
}

impl Default for Layer1Config {
    fn default() -> Self {
        Self {
            enable_ipa: true,
            enable_non_affine: true,
            enable_reductions: true,
            enable_cross_iter_speculation: false, // SPP-only per stubs.md
            enable_memory_model: true,
            enable_library_effects: true,
            max_contexts: 32,
            enum_caps: EnumCaps {
                m6_products: 4000,
                m7_hash_domain: 65536,
                m8_recurrence_domain: 4000,
                m10_fixed_point: 65536,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction_of_delta() {
        assert_eq!(Direction::of_delta(-5), Direction::Lt);
        assert_eq!(Direction::of_delta(0), Direction::Eq);
        assert_eq!(Direction::of_delta(7), Direction::Gt);
    }

    #[test]
    fn test_dependence_type_serialization() {
        let dt = DependenceType::Exact(vec![1, 0, -2]);
        let json = serde_json::to_string(&dt).unwrap();
        let back: DependenceType = serde_json::from_str(&json).unwrap();
        assert_eq!(dt, back);
    }
}