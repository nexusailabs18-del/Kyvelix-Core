//! SPP Layer 2 — Autotuner Integration
//!
//! Integrates the measurement harness with candidate generation to find
//! optimal kernel configurations through empirical measurement.

use crate::layer2::emit::{emit_candidates, loop_ir_from_analysis, CompiledKernel, TransformCandidate};
use crate::layer1::types::{DependenceAnalysis, LoopKind};
use crate::HardwareModel;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

/// Autotuner configuration
#[derive(Debug, Clone)]
pub struct AutotunerConfig {
    pub max_configs_per_kernel: usize,
    pub timeout_secs: u64,
    pub working_sets: Vec<WorkingSetSize>,
}

/// Working set size category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkingSetSize {
    L1,
    L2,
    DRAM,
}

impl WorkingSetSize {
    pub fn element_count(&self) -> usize {
        match self {
            WorkingSetSize::L1 => 2048,    // ~16 KB for doubles
            WorkingSetSize::L2 => 65536,   // ~512 KB for doubles
            WorkingSetSize::DRAM => 8_000_000, // ~64 MB for doubles
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            WorkingSetSize::L1 => "L1",
            WorkingSetSize::L2 => "L2",
            WorkingSetSize::DRAM => "DRAM",
        }
    }
}

impl Default for AutotunerConfig {
    fn default() -> Self {
        Self {
            max_configs_per_kernel: 50,
            timeout_secs: 300,
            working_sets: vec![WorkingSetSize::L1, WorkingSetSize::L2, WorkingSetSize::DRAM],
        }
    }
}

/// Result of autotuning for a single kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutotuneResult {
    pub kernel: String,
    pub best_config: Option<BestConfig>,
    pub all_results: Vec<ConfigResult>,
}

/// Best configuration found
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestConfig {
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
    pub speedup: f64,
    pub working_set: WorkingSetSize,
}

/// Result for a single configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
    pub speedup: f64,
    pub valid: bool,
    pub working_set: WorkingSetSize,
}

/// Re-export TransformKind from emit for public API
pub use crate::layer2::emit::TransformKind;

/// Re-export TransformParams from emit for public API
pub use crate::layer2::emit::TransformParams;

/// Main autotuner
pub struct Autotuner {
    config: AutotunerConfig,
    hw_model: HardwareModel,
}

impl Autotuner {
    pub fn new(config: AutotunerConfig) -> Self {
        let hw_model = HardwareModel::detect();
        Self { config, hw_model }
    }

    /// Run autotuning for a kernel
    pub fn autotune(&self, kernel: &str, analysis: &DependenceAnalysis) -> AutotuneResult {
        let loop_ir = loop_ir_from_analysis(analysis, kernel);
        let candidates = emit_candidates(&loop_ir, &self.hw_model);

        // Filter and limit candidates
        let mut filtered = self.filter_candidates(candidates, &self.config);
        filtered.truncate(self.config.max_configs_per_kernel);

        let mut all_results = Vec::new();
        let mut best: Option<BestConfig> = None;
        let mut best_speedup = 1.0;

        for candidate in filtered {
            for ws in &self.config.working_sets {
                let result = self.measure_candidate(kernel, &candidate, *ws);
                all_results.push(ConfigResult {
                    tile_size: candidate.code.tile_size,
                    simd_width: candidate.code.simd_width,
                    thread_count: candidate.code.thread_count,
                    speedup: result.speedup,
                    valid: result.valid,
                    working_set: *ws,
                });

                if result.valid && result.speedup > best_speedup {
                    best_speedup = result.speedup;
                    best = Some(BestConfig {
                        tile_size: candidate.code.tile_size,
                        simd_width: candidate.code.simd_width,
                        thread_count: candidate.code.thread_count,
                        speedup: result.speedup,
                        working_set: *ws,
                    });
                }
            }
        }

        AutotuneResult {
            kernel: kernel.to_string(),
            best_config: best,
            all_results,
        }
    }

    /// Filter candidates based on config
    fn filter_candidates(&self, candidates: Vec<TransformCandidate>, config: &AutotunerConfig) -> Vec<TransformCandidate> {
        // Keep only practical configurations
        candidates.into_iter()
            .filter(|c| {
                // Must have valid speedup prediction
                c.code.predicted_log2_speedup > 0.0 &&
                // Reasonable tile sizes
                c.code.tile_size <= 256 &&
                // Reasonable thread counts
                c.code.thread_count <= 64
            })
            .collect()
    }

    /// Measure a candidate configuration
    fn measure_candidate(&self, kernel: &str, candidate: &TransformCandidate, ws: WorkingSetSize) -> MeasurementResult {
        // Run the bench harness to measure actual speedup
        let result = self.run_bench_harness(kernel, &candidate, ws);

        MeasurementResult {
            speedup: result.speedup,
            valid: result.valid,
        }
    }

    /// Run the bench harness to measure a candidate
    fn run_bench_harness(&self, kernel: &str, candidate: &TransformCandidate, ws: WorkingSetSize) -> MeasurementResult {
        // For now, return a mock result based on cost model prediction
        // In a full implementation, this would:
        // 1. Generate the kernel C code
        // 2. Compile it with the bench harness
        // 3. Run the measurement protocol
        // 4. Return actual measured speedup

        MeasurementResult {
            speedup: 2.0_f64.powf(candidate.code.predicted_log2_speedup),
            valid: true,
        }
    }
}

/// Result of measuring a candidate
struct MeasurementResult {
    speedup: f64,
    valid: bool,
}

/// Save autotune results to file
pub fn save_results(results: &[AutotuneResult], path: &Path) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(results)?;
    fs::write(path, json)
}

/// Load autotune results from file
pub fn load_results(path: &Path) -> std::io::Result<Vec<AutotuneResult>> {
    let json = fs::read_to_string(path)?;
    serde_json::from_str(&json).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer1::types::{DependenceAnalysis, LoopKind, DependenceEdge, DependenceType, Direction, EffectSummary, AliasInfo, ReductionInfo, RecurrenceInfo, CrossLoopDependence, NonAffineDependence};

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
    fn test_autotuner_config_default() {
        let config = AutotunerConfig::default();
        assert_eq!(config.max_configs_per_kernel, 50);
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.working_sets.len(), 3);
    }
}