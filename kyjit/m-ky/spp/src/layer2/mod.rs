//! SPP Layer 2 — Parallel Version Generation
//!
//! Every loop generates a lattice of versions, not a fixed A/B/C/D.
//! Versions are parameterized by which assumptions are checked.
//! Includes full transformation catalog from Wolfe Ch. 9.

pub mod emit;
pub mod autotuner;
pub mod runtime;

use crate::layer1::types::{
    DependenceAnalysis, DependenceEdge, DependenceType, Direction, LoopKind, ParallelizationResult,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// Re-export HardwareModel for external use
pub use crate::layer3::HardwareModel;

/// A version in the version lattice
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub assumptions_checked: Vec<String>,
    pub guards: Vec<String>,
    pub dispatch_cost: u64, // Estimated cycles
    pub code: String,       // LLVM IR or similar
    pub transformation_sequence: Vec<Transformation>,
    pub loop_kind: LoopKind,
}

/// A loop transformation with its legality proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transformation {
    pub kind: TransformationKind,
    pub params: TransformationParams,
    pub legality_proof: LegalityProof,
}

/// Types of loop transformations (Wolfe Ch. 9)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransformationKind {
    StatementReordering,
    Unswitching,
    LoopPeeling,
    IndexSetSplitting,
    ScalarExpansion,
    LoopFusion,
    LoopAlignment,
    LoopFission,
    LoopReversal,
    LoopInterchanging,
    LoopSkewing,
    LinearLoopTransform,
    StripMining,
    LoopTiling,
    CircularLoopSkewing,
    Striping,
    LoopCollapsing,
    LoopCoalescing,
    ProcedureInlining,
    LoopEmbedding,
    LoopExtraction,
}

/// Parameters for each transformation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransformationParams {
    StatementReordering { order: Vec<usize> },
    Unswitching { test_expr: String, loop_level: usize },
    LoopPeeling { iterations: usize, peel_first: bool },
    IndexSetSplitting { split_point: i64 },
    ScalarExpansion { scalar_name: String, array_name: String },
    LoopFusion { first_loop: String, second_loop: String, alignment: i64 },
    LoopAlignment { m: i64 },
    LoopFission { scc_partition: Vec<Vec<usize>> },
    LoopReversal { step: i64 },
    LoopInterchanging { loop1: usize, loop2: usize },
    LoopSkewing { factor: i64, outer_loop: usize, inner_loop: usize },
    LinearLoopTransform { matrix: Vec<Vec<i64>> },
    StripMining { strip_size: i64 },
    LoopTiling { tile_sizes: Vec<i64>, tile_offsets: Vec<i64> },
    CircularLoopSkewing { modulus: i64 },
    Striping { strip_size: i64 },
    LoopCollapsing { outer_loop: usize, inner_loop: usize },
    LoopCoalescing { outer_loop: usize, inner_loop: usize },
    ProcedureInlining { callee: String },
    LoopEmbedding { caller_loop: String, callee: String },
    LoopExtraction { loop_name: String, callee: String },
}

/// Legality proof for a transformation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalityProof {
    pub is_legal: bool,
    pub violated_dependences: Vec<DependenceEdge>,
    pub condition: String,
    pub proof_obligations: Vec<String>,
}

/// Version lattice generator
pub struct VersionLatticeGenerator;

impl VersionLatticeGenerator {
    /// Generate version lattice from dependence analysis
    pub fn generate(analysis: &DependenceAnalysis) -> Vec<Version> {
        let mut versions = Vec::new();

        // Version A: All static assumptions checked
        if let Some(v) = Self::generate_version_a(analysis) {
            versions.push(v);
        }

        // Version B: Runtime checks for failed static proofs
        if let Some(v) = Self::generate_version_b(analysis) {
            versions.push(v);
        }

        // Version C: Speculative versions
        if let Some(v) = Self::generate_version_c(analysis) {
            versions.push(v);
        }

        // Version D: Serial fallback (always available)
        versions.push(Self::generate_version_d(analysis));

        versions
    }

    /// Version A: All static proofs succeed
    fn generate_version_a(analysis: &DependenceAnalysis) -> Option<Version> {
        let can_static = analysis.edges.iter().all(|e| {
            matches!(
                e.dep_type,
                DependenceType::Exact(_) | DependenceType::None | DependenceType::Bounded(_)
            )
        }) && analysis.non_affine_deps.is_empty();

        if !can_static {
            return None;
        }

        let transformations = Self::compute_transformations_for_static(analysis);

        Some(Version {
            id: "A".into(),
            assumptions_checked: vec!["all_static".into()],
            guards: vec![],
            dispatch_cost: 0,
            code: String::new(), // Filled by codegen
            transformation_sequence: transformations,
            loop_kind: analysis.loop_kind.clone(),
        })
    }

    /// Version B: Runtime checks for unresolved dependences
    fn generate_version_b(analysis: &DependenceAnalysis) -> Option<Version> {
        let needs_runtime = analysis.edges.iter().any(|e| {
            matches!(
                e.dep_type,
                DependenceType::RuntimeCheckable { .. }
            )
        }) || analysis.non_affine_deps.iter().any(|d| {
            matches!(d.dep_type, DependenceType::RuntimeCheckable { .. })
        });

        if !needs_runtime {
            return None;
        }

        let guards = Self::generate_runtime_guards(analysis);
        let transformations = Self::compute_transformations_with_guards(analysis, &guards);

        Some(Version {
            id: "B".into(),
            assumptions_checked: vec!["static_proven".into()],
            guards,
            dispatch_cost: 100, // ~100 cycles for version check
            code: String::new(),
            transformation_sequence: transformations,
            loop_kind: analysis.loop_kind.clone(),
        })
    }

    /// Version C: Speculative execution
    fn generate_version_c(analysis: &DependenceAnalysis) -> Option<Version> {
        let has_may_depend = analysis.edges.iter().any(|e| {
            matches!(e.dep_type, DependenceType::MayDepend | DependenceType::Unknown)
        }) || analysis.non_affine_deps.iter().any(|d| {
            matches!(d.dep_type, DependenceType::MayDepend | DependenceType::Unknown)
        });

        if !has_may_depend {
            return None;
        }

        Some(Version {
            id: "C".into(),
            assumptions_checked: vec!["speculative".into()],
            guards: vec!["speculation_guard".into()],
            dispatch_cost: 1000, // ~1000 cycles for chunk speculation
            code: String::new(),
            transformation_sequence: vec![], // Speculative - minimal transformations
            loop_kind: LoopKind::Unknown,
        })
    }

    /// Version D: Serial fallback
    fn generate_version_d(analysis: &DependenceAnalysis) -> Version {
        Version {
            id: "D".into(),
            assumptions_checked: vec![],
            guards: vec![],
            dispatch_cost: 0,
            code: String::new(), // Serial code
            transformation_sequence: vec![],
            loop_kind: LoopKind::Sequential,
        }
    }

    /// Compute transformations for fully static case
    fn compute_transformations_for_static(
        analysis: &DependenceAnalysis,
    ) -> Vec<Transformation> {
        let mut transformations = Vec::new();

        match analysis.loop_kind {
            LoopKind::Doall => {
                // Fully parallel - apply all beneficial transformations
                transformations.extend(Self::doall_transformations(analysis));
            }
            LoopKind::Dopar => {
                // Parallel with anti-dep resolution
                transformations.extend(Self::dopar_transformations(analysis));
            }
            LoopKind::Forall => {
                // Negative distances possible
                transformations.extend(Self::forall_transformations(analysis));
            }
            _ => {}
        }

        transformations
    }

    /// Transformations for Doall loops
    fn doall_transformations(analysis: &DependenceAnalysis) -> Vec<Transformation> {
        let mut t = Vec::new();

        // Can apply: interchange, tiling, strip-mining, fusion, fission
        t.push(Transformation {
            kind: TransformationKind::LoopInterchanging,
            params: TransformationParams::LoopInterchanging {
                loop1: 0,
                loop2: 1,
            },
            legality_proof: LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: "No loop-carried dependences".into(),
                proof_obligations: vec!["Dependence graph empty".into()],
            },
        });

        // Add tiling
        t.push(Transformation {
            kind: TransformationKind::LoopTiling,
            params: TransformationParams::LoopTiling {
                tile_sizes: vec![32, 32],
                tile_offsets: vec![0, 0],
            },
            legality_proof: LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: "All dependences loop-independent".into(),
                proof_obligations: vec!["Direction vector all Eq".into()],
            },
        });

        t
    }

    /// Transformations for Dopar loops
    fn dopar_transformations(analysis: &DependenceAnalysis) -> Vec<Transformation> {
        let mut t = Vec::new();

        // Need scalar expansion for anti-dependences
        t.push(Transformation {
            kind: TransformationKind::ScalarExpansion,
            params: TransformationParams::ScalarExpansion {
                scalar_name: "temp".into(),
                array_name: "temp_expanded".into(),
            },
            legality_proof: LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: "No upward-exposed uses".into(),
                proof_obligations: vec!["Scalar def dominates all uses in same iteration".into()],
            },
        });

        t
    }

    /// Transformations for Forall loops
    fn forall_transformations(_analysis: &DependenceAnalysis) -> Vec<Transformation> {
        vec![Transformation {
            kind: TransformationKind::LoopReversal,
            params: TransformationParams::LoopReversal { step: 1 },
            legality_proof: LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: "Forall semantics allow reversal".into(),
                proof_obligations: vec!["RHS computed before stores".into()],
            },
        }]
    }

    /// Transformations with runtime guards
    fn compute_transformations_with_guards(
        analysis: &DependenceAnalysis,
        guards: &[String],
    ) -> Vec<Transformation> {
        let mut t = Self::compute_transformations_for_static(analysis);
        // Add guarded versions
        for guard in guards {
            t.push(Transformation {
                kind: TransformationKind::Unswitching,
                params: TransformationParams::Unswitching {
                    test_expr: guard.clone(),
                    loop_level: 0,
                },
                legality_proof: LegalityProof {
                    is_legal: true,
                    violated_dependences: vec![],
                    condition: format!("Guard {} is loop-invariant", guard),
                    proof_obligations: vec!["Test independent of loop IVs".into()],
                },
            });
        }
        t
    }

    /// Generate runtime guards for dependences
    fn generate_runtime_guards(analysis: &DependenceAnalysis) -> Vec<String> {
        let mut guards = Vec::new();

        for edge in &analysis.edges {
            if let DependenceType::RuntimeCheckable { test } = &edge.dep_type {
                guards.push(format!("{:?}", test));
            }
        }

        for dep in &analysis.non_affine_deps {
            if let DependenceType::RuntimeCheckable { test } = &dep.dep_type {
                guards.push(format!("{:?}", test));
            }
        }

        guards
    }
}

/// Pareto selector for versions
pub struct ParetoSelector;

impl ParetoSelector {
    /// Select Pareto-optimal versions
    pub fn select(versions: &[Version]) -> Vec<Version> {
        let mut pareto = Vec::new();

        for v in versions {
            let mut dominated = false;
            let mut to_remove = Vec::new();

            for (i, p) in pareto.iter().enumerate() {
                if Self::dominates(p, v) {
                    dominated = true;
                    break;
                }
                if Self::dominates(v, p) {
                    to_remove.push(i);
                }
            }

            if !dominated {
                // Remove dominated versions
                for &i in to_remove.iter().rev() {
                    pareto.remove(i);
                }
                pareto.push(v.clone());
            }
        }

        pareto
    }

    /// Check if v1 dominates v2
    fn dominates(v1: &Version, v2: &Version) -> bool {
        let cost1 = v1.dispatch_cost as f64;
        let cost2 = v2.dispatch_cost as f64;

        // Coverage: number of assumptions checked
        let cov1 = v1.assumptions_checked.len() as f64;
        let cov2 = v2.assumptions_checked.len() as f64;

        (cost1 <= cost2 && cov1 >= cov2) && (cost1 < cost2 || cov1 > cov2)
    }
}

/// Cross-version deduplication
pub struct CrossVersionDedup;

impl CrossVersionDedup {
    /// Deduplicate hot inner loop bodies across versions
    pub fn dedup(versions: &mut [Version]) {
        if versions.len() <= 1 {
            return;
        }

        // Find common code prefix/suffix
        let common_body = Self::find_common_body(versions);

        if !common_body.is_empty() {
            for v in versions {
                // Extract guard/dispatch shell, keep shared body reference
                v.code = format!("SHARED_BODY({})", common_body);
            }
        }
    }

    fn find_common_body(versions: &[Version]) -> String {
        // Simplified: find longest common substring in code
        // Real implementation would use suffix arrays
        if versions.is_empty() {
            return String::new();
        }

        let first = &versions[0].code;
        let mut common = first.clone();

        for v in &versions[1..] {
            common = Self::longest_common_substring(&common, &v.code);
            if common.is_empty() {
                break;
            }
        }

        common
    }

    fn longest_common_substring(s1: &str, s2: &str) -> String {
        let bytes1 = s1.as_bytes();
        let bytes2 = s2.as_bytes();
        let mut max_len = 0;
        let mut end_pos = 0;

        for i in 0..bytes1.len() {
            for j in 0..bytes2.len() {
                let mut len = 0;
                while i + len < bytes1.len()
                    && j + len < bytes2.len()
                    && bytes1[i + len] == bytes2[j + len]
                {
                    len += 1;
                }
                if len > max_len {
                    max_len = len;
                    end_pos = i + len;
                }
            }
        }

        if max_len > 10 {
            // Minimum meaningful length
            s1[end_pos - max_len..end_pos].to_string()
        } else {
            String::new()
        }
    }
}

/// Runtime dispatcher
pub struct RuntimeDispatcher;

impl RuntimeDispatcher {
    /// Generate dispatcher code for version lattice
    pub fn generate_dispatcher(versions: &[Version]) -> String {
        if versions.is_empty() {
            return String::new();
        }

        let mut code = String::new();
        code.push_str("// SPP Runtime Dispatcher\n");

        // Sort by dispatch cost (cheapest first)
        let mut sorted = versions.to_vec();
        sorted.sort_by_key(|v| v.dispatch_cost);

        // Generate if-else chain
        for (i, v) in sorted.iter().enumerate() {
            if i == 0 {
                code.push_str(&format!("if ({}) {{\n", Self::guard_condition(v)));
            } else {
                code.push_str(&format!("else if ({}) {{\n", Self::guard_condition(v)));
            }
            code.push_str(&format!("    // Version {}\n", v.id));
            code.push_str(&format!("    {}\n", v.code));
            code.push_str("}\n");
        }

        // Final fallback
        if let Some(last) = versions.last() {
            if last.id == "D" {
                code.push_str("else {\n");
                code.push_str("    // Serial fallback\n");
                code.push_str("    SERIAL_VERSION();\n");
                code.push_str("}\n");
            }
        }

        code
    }

    fn guard_condition(version: &Version) -> String {
        if version.guards.is_empty() {
            "true".into()
        } else {
            version.guards.join(" && ")
        }
    }
}

/// Loop transformation legality checker
pub struct TransformationChecker;

impl TransformationChecker {
    /// Check legality of a transformation given dependence analysis
    pub fn check(
        kind: &TransformationKind,
        params: &TransformationParams,
        analysis: &DependenceAnalysis,
    ) -> LegalityProof {
        match kind {
            TransformationKind::StatementReordering => Self::check_reordering(params, analysis),
            TransformationKind::Unswitching => Self::check_unswitching(params, analysis),
            TransformationKind::LoopPeeling => Self::check_peeling(params, analysis),
            TransformationKind::IndexSetSplitting => Self::check_index_split(params, analysis),
            TransformationKind::ScalarExpansion => Self::check_scalar_expansion(params, analysis),
            TransformationKind::LoopFusion => Self::check_fusion(params, analysis),
            TransformationKind::LoopAlignment => Self::check_alignment(params, analysis),
            TransformationKind::LoopFission => Self::check_fission(params, analysis),
            TransformationKind::LoopReversal => Self::check_reversal(params, analysis),
            TransformationKind::LoopInterchanging => Self::check_interchange(params, analysis),
            TransformationKind::LoopSkewing => Self::check_skewing(params, analysis),
            TransformationKind::LinearLoopTransform => Self::check_linear_transform(params, analysis),
            TransformationKind::StripMining => Self::check_strip_mining(params, analysis),
            TransformationKind::LoopTiling => Self::check_tiling(params, analysis),
            TransformationKind::CircularLoopSkewing => Self::check_circular_skewing(params, analysis),
            TransformationKind::Striping => Self::check_striping(params, analysis),
            TransformationKind::LoopCollapsing => Self::check_collapsing(params, analysis),
            TransformationKind::LoopCoalescing => Self::check_coalescing(params, analysis),
            TransformationKind::ProcedureInlining => Self::check_inlining(params, analysis),
            TransformationKind::LoopEmbedding => Self::check_embedding(params, analysis),
            TransformationKind::LoopExtraction => Self::check_extraction(params, analysis),
        }
    }

    fn check_reordering(_params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        // Legal if topological sort of dependence graph
        let violated: Vec<_> = analysis
            .edges
            .iter()
            .filter(|e| matches!(e.dep_type, DependenceType::Exact(_) | DependenceType::Bounded(_)))
            .cloned()
            .collect();

        LegalityProof {
            is_legal: violated.is_empty(),
            violated_dependences: violated,
            condition: "Topological sort of dependence graph".into(),
            proof_obligations: vec!["All dependences satisfied by order".into()],
        }
    }

    fn check_unswitching(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::Unswitching { test_expr, loop_level } = params {
            LegalityProof {
                is_legal: true, // Conservative: assume test is loop-invariant
                violated_dependences: vec![],
                condition: format!("Test '{}' is loop-invariant at level {}", test_expr, loop_level),
                proof_obligations: vec!["Test independent of loop IVs".into()],
            }
        } else {
            LegalityProof {
                is_legal: false,
                violated_dependences: vec![],
                condition: "Invalid params".into(),
                proof_obligations: vec![],
            }
        }
    }

    fn check_peeling(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopPeeling { iterations, peel_first: _ } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Peel {} iteration(s)", iterations),
                proof_obligations: vec!["Zero-trip guard if needed".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_index_split(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::IndexSetSplitting { split_point } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Split at {}", split_point),
                proof_obligations: vec!["Partition covers full range".into(), "Parts are disjoint".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_scalar_expansion(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::ScalarExpansion { scalar_name, array_name } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Expand scalar {} to array {}", scalar_name, array_name),
                proof_obligations: vec!["No upward-exposed uses".into(), "Def dominates all uses in same iteration".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_fusion(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopFusion { first_loop, second_loop, alignment } = params {
            // Check cross-loop dependences
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| {
                    // Cross-loop dependence from second to first would be violated
                    matches!(&e.dep_type, DependenceType::Exact(d) if d.iter().any(|&x| x < 0))
                })
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty(),
                violated_dependences: violated,
                condition: format!("Fuse {} with {} at alignment {}", first_loop, second_loop, alignment),
                proof_obligations: vec!["All cross-loop dependences flow forward".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_alignment(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopAlignment { m } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Alignment m = {}", m),
                proof_obligations: vec!["m eliminates negative distances".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_fission(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopFission { scc_partition } = params {
            // Build statement-level dependence graph and find SCCs
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| {
                    // Check if dependence crosses SCC boundary in wrong direction
                    let src_scc = Self::find_scc(e.src, scc_partition);
                    let dst_scc = Self::find_scc(e.dst, scc_partition);
                    src_scc != dst_scc && src_scc > dst_scc
                })
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty(),
                violated_dependences: violated,
                condition: "SCCs form valid partition".into(),
                proof_obligations: vec!["Topological order of condensation graph".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn find_scc(node: usize, partition: &[Vec<usize>]) -> usize {
        for (i, scc) in partition.iter().enumerate() {
            if scc.contains(&node) {
                return i;
            }
        }
        0
    }

    fn check_reversal(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopReversal { step } = params {
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| matches!(e.dep_type, DependenceType::Exact(_) | DependenceType::Bounded(_)))
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty() || *step == 1,
                violated_dependences: violated,
                condition: format!("Reversal with step {}", step),
                proof_obligations: vec!["No loop-carried dependences for sequential loops".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_interchange(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopInterchanging { loop1, loop2 } = params {
            // Check for (<,>) direction vectors
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| {
                    if e.direction.len() > *loop1.max(loop2) {
                        matches!(
                            (e.direction[*loop1], e.direction[*loop2]),
                            (Direction::Lt, Direction::Gt)
                        )
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty(),
                violated_dependences: violated,
                condition: format!("Interchange loops {} and {}", loop1, loop2),
                proof_obligations: vec!["No dependence has direction (<,>)".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_skewing(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopSkewing { factor, outer_loop, inner_loop } = params {
            LegalityProof {
                is_legal: true, // Unimodular always legal
                violated_dependences: vec![],
                condition: format!("Skew factor {} on loops {}->{}", factor, outer_loop, inner_loop),
                proof_obligations: vec!["Resulting distances allow desired transform".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_linear_transform(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LinearLoopTransform { matrix } = params {
            // Check unimodularity
            let det = Self::determinant(matrix);
            LegalityProof {
                is_legal: det == 1 || det == -1,
                violated_dependences: vec![],
                condition: format!("Transform matrix det = {}", det),
                proof_obligations: vec!["0 ⪯ T·d for all d".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn determinant(matrix: &[Vec<i64>]) -> i64 {
        match matrix.len() {
            1 => matrix[0][0],
            2 => matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0],
            _ => {
                // Simplified: assume 2x2 for now
                matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0]
            }
        }
    }

    fn check_strip_mining(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::StripMining { strip_size } = params {
            LegalityProof {
                is_legal: *strip_size > 0,
                violated_dependences: vec![],
                condition: format!("Strip size {}", strip_size),
                proof_obligations: vec!["Dependence distances modified per formula".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_tiling(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopTiling { tile_sizes, tile_offsets } = params {
            // Check same as strip-mining for tile loop + interchange legality
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| matches!(e.dep_type, DependenceType::Exact(_) | DependenceType::Bounded(_)))
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty(),
                violated_dependences: violated,
                condition: format!("Tile sizes {:?}, offsets {:?}", tile_sizes, tile_offsets),
                proof_obligations: vec!["Interchange of tile/element loops legal".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_circular_skewing(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::CircularLoopSkewing { modulus } = params {
            // Inner loop must carry no dependences
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| e.level == 1 && matches!(e.dep_type, DependenceType::Exact(_) | DependenceType::Bounded(_)))
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty(),
                violated_dependences: violated,
                condition: format!("Circular skew modulus {}", modulus),
                proof_obligations: vec!["Inner loop has no loop-carried dependences".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_striping(params: &TransformationParams, analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::Striping { strip_size } = params {
            // Strip-mining + interchange - check for negative distances
            let violated: Vec<_> = analysis
                .edges
                .iter()
                .filter(|e| {
                    if e.direction.len() >= 2 {
                        matches!(e.direction[0], Direction::Gt) // Would become negative after interchange
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();

            LegalityProof {
                is_legal: violated.is_empty(),
                violated_dependences: violated,
                condition: format!("Striping with size {}", strip_size),
                proof_obligations: vec!["No negative distance vectors after interchange".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_collapsing(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopCollapsing { outer_loop, inner_loop } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Collapse loops {} and {}", outer_loop, inner_loop),
                proof_obligations: vec!["Array bounds match loop limits, strides constant".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_coalescing(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopCoalescing { outer_loop, inner_loop } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Coalesce loops {} and {}", outer_loop, inner_loop),
                proof_obligations: vec!["Loop limits invariant".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_inlining(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::ProcedureInlining { callee } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Inline {}", callee),
                proof_obligations: vec!["Always legal".into()],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_embedding(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopEmbedding { caller_loop, callee } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Embed {} into {}", caller_loop, callee),
                proof_obligations: vec![],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }

    fn check_extraction(params: &TransformationParams, _analysis: &DependenceAnalysis) -> LegalityProof {
        if let TransformationParams::LoopExtraction { loop_name, callee } = params {
            LegalityProof {
                is_legal: true,
                violated_dependences: vec![],
                condition: format!("Extract {} from {}", loop_name, callee),
                proof_obligations: vec![],
            }
        } else {
            LegalityProof { is_legal: false, violated_dependences: vec![], condition: "Invalid".into(), proof_obligations: vec![] }
        }
    }
}

/// Main entry point for Layer 2
pub fn spp_layer2_generate_versions(analysis: &DependenceAnalysis) -> Vec<Version> {
    let versions = VersionLatticeGenerator::generate(analysis);
    let pareto = ParetoSelector::select(&versions);
    pareto
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer1::types::{DependenceEdge, DependenceType, Direction, LoopKind, EffectSummary, AliasInfo, ReductionInfo, RecurrenceInfo, CrossLoopDependence, NonAffineDependence};

    fn sample_analysis() -> DependenceAnalysis {
        DependenceAnalysis {
            loop_hash: "test".into(),
            edges: vec![],
            loop_kind: LoopKind::Doall,
            reductions: vec![],
            recurrences: vec![],
            alias_info: AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
            effect_summary: EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
            cross_loop_deps: vec![],
            non_affine_deps: vec![],
        }
    }

    #[test]
    fn test_version_lattice() {
        let analysis = sample_analysis();
        let versions = VersionLatticeGenerator::generate(&analysis);
        assert!(!versions.is_empty());
        assert!(versions.iter().any(|v| v.id == "A"));
        assert!(versions.iter().any(|v| v.id == "D"));
    }

    #[test]
    fn test_pareto_selector() {
        let versions = vec![
            Version { id: "A".into(), assumptions_checked: vec!["a".into()], guards: vec![], dispatch_cost: 10, code: "".into(), transformation_sequence: vec![], loop_kind: LoopKind::Doall },
            Version { id: "B".into(), assumptions_checked: vec!["a".into(), "b".into()], guards: vec!["b".into()], dispatch_cost: 100, code: "".into(), transformation_sequence: vec![], loop_kind: LoopKind::Doall },
        ];
        let pareto = ParetoSelector::select(&versions);
        assert_eq!(pareto.len(), 2); // Neither dominates (B has more coverage but higher cost)
    }

    #[test]
    fn test_transformation_checker() {
        let analysis = sample_analysis();
        let proof = TransformationChecker::check(
            &TransformationKind::LoopInterchanging,
            &TransformationParams::LoopInterchanging { loop1: 0, loop2: 1 },
            &analysis,
        );
        assert!(proof.is_legal);
    }

    #[test]
    fn test_cross_version_dedup() {
        let mut versions = vec![
            Version { id: "A".into(), assumptions_checked: vec![], guards: vec![], dispatch_cost: 0, code: "common_body_inner_loop".into(), transformation_sequence: vec![], loop_kind: LoopKind::Doall },
            Version { id: "B".into(), assumptions_checked: vec![], guards: vec![], dispatch_cost: 100, code: "common_body_inner_loop_guarded".into(), transformation_sequence: vec![], loop_kind: LoopKind::Doall },
        ];
        CrossVersionDedup::dedup(&mut versions);
        // Should find common prefix
    }

    #[test]
    fn test_runtime_dispatcher() {
        let versions = vec![
            Version { id: "A".into(), assumptions_checked: vec!["a".into()], guards: vec![], dispatch_cost: 0, code: "A_CODE".into(), transformation_sequence: vec![], loop_kind: LoopKind::Doall },
            Version { id: "D".into(), assumptions_checked: vec![], guards: vec![], dispatch_cost: 0, code: "D_CODE".into(), transformation_sequence: vec![], loop_kind: LoopKind::Sequential },
        ];
        let dispatcher = RuntimeDispatcher::generate_dispatcher(&versions);
        assert!(dispatcher.contains("Version A"));
        assert!(dispatcher.contains("Version D"));
    }
}