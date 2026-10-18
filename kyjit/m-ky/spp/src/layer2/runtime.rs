//! SPP Layer 2 — Runtime Measurement Loop (Phase 3b)
//!
//! Runtime decision tree: cold → serial, warm → analytic model, hot → autotune + cache.
//! Reuses the harness's measurement protocol at runtime for continuous optimization.

use crate::layer2::emit::{emit_candidates, loop_ir_from_analysis, CompiledKernel, TransformCandidate, TransformKind, TransformParams};
use crate::layer2::autotuner::{Autotuner, AutotunerConfig, AutotuneResult, ConfigResult, WorkingSetSize, BestConfig};
use crate::layer1::types::{DependenceAnalysis, LoopKind};
use crate::HardwareModel;
use crate::layer3::CostModel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

/// Runtime profiling state for a loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopProfile {
    /// Loop hash from analysis
    pub loop_hash: String,
    /// Number of times this loop has been executed
    pub execution_count: u64,
    /// Cumulative execution time (cycles)
    pub total_cycles: u64,
    /// Current best configuration found
    pub best_config: Option<RuntimeConfig>,
    /// Whether this loop is hot enough for autotuning
    pub is_hot: bool,
}

/// Runtime configuration for a loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub transform_kind: String,
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
    pub predicted_log2_speedup: f64,
    pub measured_speedup: f64,
}

/// Decision tree result for a loop
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeDecision {
    /// Loop is cold - run serial version
    Cold,
    /// Loop is warm - use analytic cost model
    Warm,
    /// Loop is hot - run autotuner and cache result
    Hot,
}

/// Runtime measurement result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementResult {
    pub config: String,  // Configuration identifier
    pub cycles: u64,
    pub valid: bool,
    pub timestamp: u64,
}

/// Runtime autotuner with caching
pub struct RuntimeAutotuner {
    autotuner: Autotuner,
    /// Cache of profiles by loop hash
    profiles: Arc<Mutex<HashMap<String, LoopProfile>>>,
    /// Autotune cache by (loop_hash, hardware_fingerprint)
    autotune_cache: Arc<Mutex<HashMap<(String, String), BestConfig>>>,
    /// Measurement protocol for runtime validation
    protocol: MeasurementProtocol,
    config: RuntimeAutotunerConfig,
}

/// Runtime autotuner configuration
#[derive(Debug, Clone)]
pub struct RuntimeAutotunerConfig {
    /// Threshold for considering a loop "hot" (execution count)
    pub hot_threshold: u64,
    /// Threshold for considering a loop "warm" (execution count)
    pub warm_threshold: u64,
    /// Maximum configurations to test at runtime
    pub max_runtime_configs: usize,
    /// Minimum speedup to consider a config worthwhile
    pub min_speedup_threshold: f64,
    /// Enable runtime autotuning
    pub enabled: bool,
}

impl Default for RuntimeAutotunerConfig {
    fn default() -> Self {
        Self {
            hot_threshold: 1000,
            warm_threshold: 100,
            max_runtime_configs: 10,
            min_speedup_threshold: 1.2,
            enabled: true,
        }
    }
}

/// Measurement protocol for runtime (reuses harness protocol)
struct MeasurementProtocol {
    hw_model: crate::HardwareModel,
}

impl MeasurementProtocol {
    fn new() -> Self {
        Self {
            hw_model: crate::HardwareModel::detect(),
        }
    }
}

impl RuntimeAutotuner {
    pub fn new(config: RuntimeAutotunerConfig) -> Self {
        let autotuner_config = AutotunerConfig::default();
        Self {
            autotuner: Autotuner::new(autotuner_config),
            profiles: Arc::new(Mutex::new(HashMap::new())),
            autotune_cache: Arc::new(Mutex::new(HashMap::new())),
            protocol: MeasurementProtocol::new(),
            config,
        }
    }

    /// Get or create profile for a loop
    fn get_or_create_profile(&self, loop_hash: &str) -> LoopProfile {
        let mut profiles = self.profiles.lock().unwrap();
        profiles.entry(loop_hash.to_string()).or_insert_with(|| LoopProfile {
            loop_hash: loop_hash.to_string(),
            execution_count: 0,
            total_cycles: 0,
            best_config: None,
            is_hot: false,
        }).clone()
    }

    /// Update profile after execution
    fn update_profile(&self, loop_hash: &str, cycles: u64) {
        let mut profiles = self.profiles.lock().unwrap();
        let profile = profiles.entry(loop_hash.to_string()).or_insert_with(|| LoopProfile {
            loop_hash: loop_hash.to_string(),
            execution_count: 0,
            total_cycles: 0,
            best_config: None,
            is_hot: false,
        });
        profile.execution_count += 1;
        profile.total_cycles += cycles;
        // Update hot/cold status
        profile.is_hot = profile.execution_count >= self.config.hot_threshold;
    }

    /// Decision tree: cold → serial, warm → analytic model, hot → autotune + cache
    pub fn decide(&self, loop_hash: &str) -> RuntimeDecision {
        let profile = self.get_or_create_profile(loop_hash);

        if profile.execution_count < self.config.warm_threshold {
            RuntimeDecision::Cold
        } else if profile.execution_count < self.config.hot_threshold {
            RuntimeDecision::Warm
        } else {
            RuntimeDecision::Hot
        }
    }

    /// Get best configuration for current runtime state
    pub fn get_config(&self, loop_hash: &str, analysis: &crate::layer1::types::DependenceAnalysis) -> Option<RuntimeConfig> {
        let decision = self.decide(loop_hash);

        match decision {
            RuntimeDecision::Cold => None, // Use serial
            RuntimeDecision::Warm => {
                // Use analytic cost model prediction
                let loop_ir = crate::layer2::emit::loop_ir_from_analysis(analysis, "unknown");
                let hw = crate::HardwareModel::detect();
                let candidates = crate::layer2::emit::emit_candidates(&loop_ir, &hw);

                // Find best predicted config
                candidates.into_iter()
                    .filter(|c| c.code.predicted_log2_speedup > 0.0)
                    .max_by(|a, b| a.code.predicted_log2_speedup.partial_cmp(&b.code.predicted_log2_speedup).unwrap())
                    .map(|c| RuntimeConfig {
                        transform_kind: format!("{:?}", c.kind),
                        tile_size: c.code.tile_size,
                        simd_width: c.code.simd_width,
                        thread_count: c.code.thread_count,
                        predicted_log2_speedup: c.code.predicted_log2_speedup,
                        measured_speedup: 0.0,
                    })
            }
            RuntimeDecision::Hot => {
                // Check cache first
                let hw = crate::HardwareModel::detect();
                let hw_fingerprint = format!("{:?}", hw.cpu);

                let cache = self.autotune_cache.lock().unwrap();
                if let Some(cached) = cache.get(&(loop_hash.to_string(), hw_fingerprint.clone())) {
                    return Some(RuntimeConfig {
                        transform_kind: format!("{:?}", TransformKind::Full),
                        tile_size: cached.tile_size,
                        simd_width: cached.simd_width,
                        thread_count: cached.thread_count,
                        predicted_log2_speedup: cached.speedup.log2(),
                        measured_speedup: cached.speedup,
                    });
                }
                drop(cache);

                // Run autotuner
                let hw = crate::HardwareModel::detect();
                let analysis = crate::layer1::types::DependenceAnalysis {
                    loop_hash: "runtime".into(),
                    edges: vec![],
                    loop_kind: crate::layer1::types::LoopKind::Doall,
                    reductions: vec![],
                    recurrences: vec![],
                    alias_info: crate::layer1::types::AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
                    effect_summary: crate::layer1::types::EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
                    cross_loop_deps: vec![],
                    non_affine_deps: vec![],
                };

                let loop_ir = crate::layer2::emit::loop_ir_from_analysis(&crate::layer1::types::DependenceAnalysis {
                    loop_hash: "runtime".into(),
                    edges: vec![],
                    loop_kind: crate::layer1::types::LoopKind::Doall,
                    reductions: vec![],
                    recurrences: vec![],
                    alias_info: crate::layer1::types::AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
                    effect_summary: crate::layer1::types::EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
                    cross_loop_deps: vec![],
                    non_affine_deps: vec![],
                }, "runtime");

                let autotuner = crate::layer2::autotuner::Autotuner::new(crate::layer2::autotuner::AutotunerConfig::default());
                let result = autotuner.autotune("runtime", &crate::layer1::types::DependenceAnalysis {
                    loop_hash: "runtime".into(),
                    edges: vec![],
                    loop_kind: crate::layer1::types::LoopKind::Doall,
                    reductions: vec![],
                    recurrences: vec![],
                    alias_info: crate::layer1::types::AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
                    effect_summary: crate::layer1::types::EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
                    cross_loop_deps: vec![],
                    non_affine_deps: vec![],
                });

                if let Some(best) = result.best_config {
                    // Cache the result
                    let mut cache = self.autotune_cache.lock().unwrap();
                    cache.insert((loop_hash.to_string(), format!("cpu_cores_{}_smt_{}", crate::HardwareModel::detect().cpu.total_logical_cores, crate::HardwareModel::detect().cpu.smt)), crate::layer2::autotuner::BestConfig {
                        tile_size: best.tile_size,
                        simd_width: best.simd_width,
                        thread_count: best.thread_count,
                        speedup: best.speedup,
                        working_set: crate::layer2::autotuner::WorkingSetSize::L2,
                    });

                    Some(RuntimeConfig {
                        transform_kind: "Full".into(),
                        tile_size: best.tile_size,
                        simd_width: best.simd_width,
                        thread_count: best.thread_count,
                        predicted_log2_speedup: best.speedup.log2(),
                        measured_speedup: best.speedup,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Record a measurement result
    pub fn record_measurement(&self, loop_hash: &str, result: MeasurementResult) {
        self.update_profile(loop_hash, result.cycles);

        // If we got a significant speedup, consider updating best config
        if result.valid && result.cycles > 0 {
            let mut profiles = self.profiles.lock().unwrap();
            if let Some(profile) = profiles.get_mut(loop_hash) {
                if profile.best_config.is_none() || profile.best_config.as_ref().unwrap().measured_speedup < 1.0 {
                    // Could update with new config info
                }
            }
        }
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let profiles = self.profiles.lock().unwrap().len();
        let cache = self.autotune_cache.lock().unwrap().len();
        (profiles, cache)
    }
}

/// Global runtime autotuner instance
static RUNTIME_AUTOTUNER: OnceLock<RuntimeAutotuner> = OnceLock::new();

/// Get or create global runtime autotuner
pub fn get_runtime_autotuner() -> &'static RuntimeAutotuner {
    RUNTIME_AUTOTUNER.get_or_init(|| RuntimeAutotuner::new(RuntimeAutotunerConfig::default()))
}

/// Convenience function for runtime decision
pub fn runtime_decide(loop_hash: &str) -> RuntimeDecision {
    get_runtime_autotuner().decide(loop_hash)
}

/// Convenience function to get runtime config
pub fn runtime_get_config(loop_hash: &str, analysis: &crate::layer1::types::DependenceAnalysis) -> Option<RuntimeConfig> {
    get_runtime_autotuner().get_config(loop_hash, analysis)
}

/// Record a runtime measurement
pub fn runtime_record_measurement(loop_hash: &str, cycles: u64, valid: bool) {
    let result = crate::layer2::runtime::MeasurementResult {
        config: "runtime".into(),
        cycles,
        valid,
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
    };
    get_runtime_autotuner().record_measurement(loop_hash, result);
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
            alias_info: crate::layer1::types::AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
            effect_summary: crate::layer1::types::EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
            cross_loop_deps: vec![],
            non_affine_deps: vec![],
        }
    }

    #[test]
    fn test_runtime_decision_cold() {
        let autotuner = RuntimeAutotuner::new(RuntimeAutotunerConfig::default());
        let decision = autotuner.decide("cold_loop");
        assert_eq!(decision, RuntimeDecision::Cold);
    }

    #[test]
    fn test_runtime_decision_warm() {
        let autotuner = RuntimeAutotuner::new(RuntimeAutotunerConfig::default());
        // Simulate warm loop
        for _ in 0..150 {
            autotuner.update_profile("warm_loop", 1000);
        }
        let decision = autotuner.decide("warm_loop");
        assert_eq!(decision, RuntimeDecision::Warm);
    }

    #[test]
    fn test_runtime_decision_hot() {
        let autotuner = RuntimeAutotuner::new(RuntimeAutotunerConfig::default());
        // Simulate hot loop
        for _ in 0..2000 {
            autotuner.update_profile("hot_loop", 1000);
        }
        let decision = autotuner.decide("hot_loop");
        assert_eq!(decision, RuntimeDecision::Hot);
    }

    #[test]
    fn test_runtime_config_cold() {
        let autotuner = RuntimeAutotuner::new(RuntimeAutotunerConfig::default());
        let analysis = crate::layer1::types::DependenceAnalysis {
            loop_hash: "test".into(),
            edges: vec![],
            loop_kind: crate::layer1::types::LoopKind::Doall,
            reductions: vec![],
            recurrences: vec![],
            alias_info: crate::layer1::types::AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
            effect_summary: crate::layer1::types::EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
            cross_loop_deps: vec![],
            non_affine_deps: vec![],
        };

        let config = autotuner.get_config("cold_loop", &analysis);
        assert!(config.is_none()); // Cold returns None (serial)
    }

    #[test]
    fn test_cache_stats() {
        let autotuner = RuntimeAutotuner::new(RuntimeAutotunerConfig::default());
        let (profiles, cache) = autotuner.cache_stats();
        assert_eq!(profiles, 0);
        assert_eq!(cache, 0);
    }
}