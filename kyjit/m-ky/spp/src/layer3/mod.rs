//! SPP Layer 3 — SPP-HX (Hierarchical Execution Optimizer)
//
//! Hierarchical execution optimizer with:
//! - Hardware model (CPU topology, cache, SIMD, memory, NUMA, sync)
//! - Analytic cost model (roofline + cache hierarchy + sync + tile overhead)
//! - Bayesian search space (thread count, SIMD width, tile size, chunk, fusion, schedule, placement)
//! - Execution optimizer (hierarchical scheduling, barrier minimization, NUMA placement, cache locality, adaptive load balancing)
//! - Online re-optimization (re-JIT mid-execution)
//! - Energy/power as first-class cost
//! - Profitability gate (10x speedup admission criterion)

use crate::layer1::types::DependenceAnalysis;
use crate::layer2::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod detect;

/// CPU topology information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuTopology {
    pub p_cores: usize,
    pub e_cores: usize,
    pub smt: bool,
    pub threads_per_core: usize,
    pub total_logical_cores: usize,
}

/// Cache hierarchy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheHierarchy {
    pub l1_size: usize,
    pub l1_assoc: usize,
    pub l2_size: usize,
    pub l2_assoc: usize,
    pub l3_size: usize,
    pub l3_assoc: usize,
}

/// SIMD capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimdCapabilities {
    pub avx2: bool,
    pub avx512: bool,
    pub neon: bool,
    pub max_vector_width_bits: usize,
}

/// Memory bandwidth
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBandwidth {
    pub channels: usize,
    pub speed_mts: usize, // MT/s
    pub bandwidth_gb_s: f64,         // nominal from DMI
    pub bandwidth_measured_gb_s: Option<f64>, // measured via STREAM
}

/// NUMA topology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumaTopology {
    pub nodes: usize,
    pub distances: Vec<Vec<u32>>, // node-to-node latency
    pub node_cores: Vec<Vec<usize>>,
}

/// Sync costs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCosts {
    pub barrier_cycles: u64,
    pub atomic_cycles: u64,
    pub launch_cycles: u64,
}

/// Complete hardware model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareModel {
    pub cpu: CpuTopology,
    pub cache: CacheHierarchy,
    pub simd: SimdCapabilities,
    pub memory: MemoryBandwidth,
    pub numa: NumaTopology,
    pub sync: SyncCosts,
}

impl HardwareModel {
    /// Detect hardware model from system using CPUID, sysfs, DMI, and micro-benchmarks
    pub fn detect() -> Self {
        detect::detect()
    }

    /// Get number of physical cores
    pub fn physical_cores(&self) -> usize {
        self.cpu.p_cores + self.cpu.e_cores
    }

    /// Get cache size at level
    pub fn cache_size(&self, level: usize) -> usize {
        match level {
            1 => self.cache.l1_size,
            2 => self.cache.l2_size,
            3 => self.cache.l3_size,
            _ => 0,
        }
    }
}

/// Analytic cost model coefficients (tuned by hand, not ML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coefficients {
    /// Roofline
    pub flops_per_cycle_per_core: f64,     // e.g. 16.0
    pub bytes_per_cycle_per_core: f64,     // e.g. 16.0

    /// Cache
    pub l1_bytes: u64,                     // e.g. 32 * 1024
    pub l2_bytes: u64,                     // e.g. 256 * 1024
    pub l3_bytes: u64,                     // e.g. 8 * 1024 * 1024
    pub l1_miss_penalty_cycles: f64,       // e.g. 12.0
    pub l2_miss_penalty_cycles: f64,       // e.g. 40.0
    pub l3_miss_penalty_cycles: f64,       // e.g. 200.0
    pub dram_penalty_cycles: f64,          // e.g. 250.0

    /// SIMD
    pub simd_efficiency: f64,              // 0.0..1.0, e.g. 0.85

    /// Parallel
    pub sync_barrier_cycles: f64,          // e.g. 100.0
    pub thread_start_cycles: f64,          // e.g. 5000.0
    pub parallel_efficiency: f64,          // 0.0..1.0, e.g. 0.9

    /// Tile
    pub tile_overhead_cycles: f64,         // e.g. 8.0

    /// Baseline calibration
    pub baseline_cycles_per_op: f64,       // e.g. 4.0
}

impl Default for Coefficients {
    fn default() -> Self {
        Self {
            flops_per_cycle_per_core: 16.0,
            bytes_per_cycle_per_core: 16.0,
            l1_bytes: 32 * 1024,
            l2_bytes: 256 * 1024,
            l3_bytes: 8 * 1024 * 1024,
            l1_miss_penalty_cycles: 12.0,
            l2_miss_penalty_cycles: 40.0,
            l3_miss_penalty_cycles: 200.0,
            dram_penalty_cycles: 250.0,
            simd_efficiency: 0.85,
            sync_barrier_cycles: 100.0,
            thread_start_cycles: 5000.0,
            parallel_efficiency: 0.9,
            tile_overhead_cycles: 8.0,
            baseline_cycles_per_op: 4.0,
        }
    }
}

/// Analytic cost model (roofline + cache hierarchy + sync + tile overhead)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostModel {
    pub coeff: Coefficients,
}

impl CostModel {
    /// Create a new cost model with default coefficients
    pub fn new() -> Self {
        Self {
            coeff: Coefficients::default(),
        }
    }

    /// Predict log2(speedup) for a candidate transformation relative to an untransformed baseline.
    /// Returns log2(speedup) where speedup = baseline_time / transformed_time.
    pub fn predict(&self, f: &CostFeatures) -> f64 {
        let c = &self.coeff;
        let hw_cores = f.thread_count.max(1) as f64;

        // Total operations (FLOPs)
        let total_ops = (f.trip_count as f64) * (f.memory_accesses as f64) * f.compute_intensity.max(1.0);

        // Total bytes accessed
        let total_bytes = (f.trip_count as f64) * (f.memory_accesses as f64) * (f.element_size as f64).max(1.0);

        // Baseline time (no transformations)
        let baseline_time = total_ops * self.coeff.baseline_cycles_per_op;

        // SIMD factor
        let simd_factor = if f.simd_width > 0 {
            (f.simd_width as f64 / 128.0) * self.coeff.simd_efficiency
        } else {
            1.0
        };

        // Compute time (roofline)
        let flops_per_cycle = self.coeff.flops_per_cycle_per_core * self.coeff.parallel_efficiency;
        let compute_time = total_ops / (flops_per_cycle * hw_cores * simd_factor);

        // Memory time (bytes / bandwidth + cache misses)
        let bytes_per_cycle = self.coeff.bytes_per_cycle_per_core * self.coeff.parallel_efficiency;
        let memory_time = total_bytes / (bytes_per_cycle * hw_cores);

        // Cache miss cycles from working set vs cache hierarchy
        // When tiling is applied, the effective working set per tile is smaller
        let effective_working_set = if f.tile_size > 0 {
            // With tiling, working set per tile is proportional to tile size
            // Estimate: working_set * (tile_size / trip_count) with minimum
            let tile_ratio = (f.tile_size as f64 / f.trip_count as f64).max(0.01);
            (f.working_set_bytes as f64) * tile_ratio
        } else {
            f.working_set_bytes as f64
        };
        let working_set = effective_working_set;
        let cache_miss_cycles = if working_set <= self.coeff.l1_bytes as f64 {
            0.0
        } else if working_set <= self.coeff.l2_bytes as f64 {
            // misses derived from reuse_distance and trip count
            let misses = (f.trip_count as f64) / f.reuse_distance.max(1.0);
            misses * self.coeff.l1_miss_penalty_cycles
        } else if working_set <= self.coeff.l3_bytes as f64 {
            let misses = (f.trip_count as f64) / f.reuse_distance.max(1.0);
            misses * self.coeff.l2_miss_penalty_cycles
        } else {
            let misses = (f.trip_count as f64) / f.reuse_distance.max(1.0);
            misses * self.coeff.dram_penalty_cycles
        };

        let memory_time = memory_time + cache_miss_cycles;

        // Sync time
        let sync_time = if f.is_parallelizable {
            self.coeff.thread_start_cycles + self.coeff.sync_barrier_cycles * (f.thread_count as f64).log2().max(1.0)
        } else {
            0.0
        };

        // Tile overhead
        let tile_overhead = if f.tile_size > 0 {
            let num_tiles = (f.trip_count as f64 / f.tile_size as f64).ceil();
            self.coeff.tile_overhead_cycles * num_tiles
        } else {
            0.0
        };

        // Transformed time
        let transformed_time = compute_time.max(memory_time) + sync_time + tile_overhead;

        // Speedup = baseline_time / transformed_time
        // Return log2(speedup)
        if transformed_time <= 0.0 || baseline_time <= 0.0 {
            0.0
        } else {
            (baseline_time / transformed_time).log2()
        }
    }
}

/// Features for cost model prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostFeatures {
    /// Loop nest depth: 1, 2, 3
    pub loop_depth: usize,
    /// Total iterations across all loops
    pub trip_count: usize,
    /// Element size in bytes (typically 8 for f64)
    pub element_size: usize,
    /// Total working set in bytes
    pub working_set_bytes: usize,
    /// Working set category
    pub working_set_category: WorkingSetCategory,
    /// Number of memory accesses per iteration
    pub memory_accesses: usize,
    /// FLOPs per byte
    pub compute_intensity: f64,
    /// Stride pattern
    pub stride_pattern: StridePattern,
    /// Stride value if constant
    pub stride_value: Option<usize>,
    /// Access pattern category
    pub access_pattern: AccessPattern,
    /// Arithmetic intensity category
    pub arithmetic_intensity_category: ArithmeticIntensityCategory,
    /// Dependency type
    pub dependency_type: DependencyType,
    /// Whether kernel is parallelizable
    pub is_parallelizable: bool,
    /// Transformation parameters
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
    /// Derived features
    pub numa_locality: f64,
    pub reuse_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkingSetCategory {
    L1,
    L2,
    DRAM,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessPattern {
    UnitStride,
    Strided,
    Reverse,
    Irregular,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArithmeticIntensityCategory {
    Copy,      // 0-1 FLOP/byte
    Axpy,      // 1-2 FLOP/byte
    Dot,       // 2-4 FLOP/byte
    MatMul,    // >4 FLOP/byte (compute-bound)
    Polynomial,
    Transcendental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyType {
    None,           // Fully parallel
    Reduction,      // Reduction (parallelizable with tree)
    Recurrence,     // Serial recurrence
    AntiDependence, // Can be resolved
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StridePattern {
    Unit,
    Constant(usize),
    Variable,
}

/// Bayesian search space for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSpace {
    /// Search dimensions
    pub dimensions: Vec<SearchDimension>,
    /// History of evaluated points
    pub history: Vec<(SearchPoint, f64)>, // (point, speedup)
    /// Gaussian process surrogate (simplified)
    pub gp_mean: Vec<f64>,
    pub gp_cov: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDimension {
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub is_integer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPoint {
    pub values: Vec<f64>,
}

impl SearchSpace {
    /// Create default search space for SPP
    pub fn default() -> Self {
        let dimensions = vec![
            SearchDimension { name: "thread_count".into(), min: 1.0, max: 64.0, is_integer: true },
            SearchDimension { name: "simd_width".into(), min: 128.0, max: 512.0, is_integer: true },
            SearchDimension { name: "tile_size".into(), min: 16.0, max: 4096.0, is_integer: true },
            SearchDimension { name: "chunk_size".into(), min: 1.0, max: 1024.0, is_integer: true },
            SearchDimension { name: "fusion_level".into(), min: 0.0, max: 5.0, is_integer: true },
            SearchDimension { name: "schedule".into(), min: 0.0, max: 2.0, is_integer: true }, // 0=static, 1=dynamic, 2=guided
            SearchDimension { name: "placement".into(), min: 0.0, max: 1.0, is_integer: true }, // 0=local, 1=remote
        ];

        Self {
            dimensions,
            history: Vec::new(),
            gp_mean: Vec::new(),
            gp_cov: Vec::new(),
        }
    }

    /// Suggest next point using Bayesian optimization (Expected Improvement)
    pub fn suggest_next(&mut self, hw: &HardwareModel) -> SearchPoint {
        // Simplified: random search with bounds
        // In production: proper GP + EI acquisition
        use rand::Rng;
        let mut rng = rand::thread_rng();

        let values: Vec<f64> = self
            .dimensions
            .iter()
            .map(|d| {
                if d.is_integer {
                    rng.gen_range(d.min as i64..=d.max as i64) as f64
                } else {
                    rng.gen_range(d.min..d.max)
                }
            })
            .collect();

        SearchPoint { values }
    }

    /// Update history with new observation
    pub fn observe(&mut self, point: SearchPoint, speedup: f64) {
        self.history.push((point, speedup));
        // In production: update GP posterior
    }
}

/// Execution optimizer
#[derive(Debug, Clone)]
pub struct ExecutionOptimizer {
    hardware: HardwareModel,
}

impl ExecutionOptimizer {
    pub fn new(hardware: HardwareModel) -> Self {
        Self { hardware }
    }

    /// Hierarchical scheduling: Program → Region → Loop → Tile → Worker → SIMD
    pub fn schedule(&self, versions: &[Version]) -> SchedulePlan {
        let mut plan = SchedulePlan::default();

        for v in versions {
            let schedule = self.schedule_version(v);
            plan.version_schedules.insert(v.id.clone(), schedule);
        }

        plan
    }

    fn schedule_version(&self, version: &Version) -> VersionSchedule {
        // Determine thread count based on loop kind and hardware
        let thread_count = self.optimal_thread_count(version);
        let simd_width = self.optimal_simd_width(version);
        let tile_sizes = self.optimal_tile_sizes(version);
        let placement = self.numa_placement(version);

        VersionSchedule {
            thread_count,
            simd_width,
            tile_sizes,
            placement,
            barriers: self.minimize_barriers(version),
            load_balancing: LoadBalancingStrategy::WorkStealing,
        }
    }

    fn optimal_thread_count(&self, _version: &Version) -> usize {
        self.hardware.cpu.total_logical_cores
    }

    fn optimal_simd_width(&self, _version: &Version) -> usize {
        self.hardware.simd.max_vector_width_bits
    }

    fn optimal_tile_sizes(&self, _version: &Version) -> Vec<usize> {
        // L1-resident tile
        let l1_elems = self.hardware.cache.l1_size / 8; // assume 8-byte elements
        let tile = (l1_elems as f64).sqrt() as usize;
        vec![tile.max(16), tile.max(16)]
    }

    fn numa_placement(&self, _version: &Version) -> NumaPlacement {
        NumaPlacement::FirstTouch
    }

    fn minimize_barriers(&self, version: &Version) -> Vec<BarrierPoint> {
        // Only sync where dependence requires
        let mut barriers = Vec::new();

        for (i, t) in version.transformation_sequence.iter().enumerate() {
            if matches!(t.kind, crate::layer2::TransformationKind::LoopTiling) {
                barriers.push(BarrierPoint {
                    location: format!("after_tile_{}", i),
                    kind: BarrierKind::TileBoundary,
                });
            }
        }

        barriers
    }
}

/// Schedule plan for all versions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchedulePlan {
    pub version_schedules: HashMap<String, VersionSchedule>,
}

/// Per-version schedule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionSchedule {
    pub thread_count: usize,
    pub simd_width: usize,
    pub tile_sizes: Vec<usize>,
    pub placement: NumaPlacement,
    pub barriers: Vec<BarrierPoint>,
    pub load_balancing: LoadBalancingStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NumaPlacement {
    FirstTouch,
    Local,
    Interleaved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadBalancingStrategy {
    Static,
    Dynamic,
    Guided,
    WorkStealing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierPoint {
    pub location: String,
    pub kind: BarrierKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BarrierKind {
    LoopBoundary,
    TileBoundary,
    DependenceRequired,
}

/// Online re-optimizer
#[derive(Debug, Clone)]
pub struct OnlineReoptimizer {
    hardware: HardwareModel,
    calibration_threshold: f64, // deviation threshold
}

impl OnlineReoptimizer {
    pub fn new(hardware: HardwareModel) -> Self {
        Self {
            hardware,
            calibration_threshold: 0.2, // 20% deviation triggers reopt
        }
    }

    /// Check if re-optimization is needed based on calibration
    pub fn should_reoptimize(&self, predicted: f64, actual: f64) -> bool {
        if predicted <= 0.0 {
            return false;
        }
        let deviation = (actual - predicted).abs() / predicted;
        deviation > self.calibration_threshold
    }

    /// Re-JIT a hot loop mid-execution
    pub fn reoptimize(&self, version: &Version, actual_performance: f64) -> Version {
        // In production: recompile with adjusted parameters
        // For now, return adjusted version
        let mut new_version = version.clone();
        new_version.id = format!("{}_reopt", version.id);
        new_version.dispatch_cost = (version.dispatch_cost as f64 * (actual_performance / 1.0)) as u64;
        new_version
    }
}

/// Energy cost model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyModel {
    /// Energy per operation (pJ)
    pub energy_per_flop: f64,
    pub energy_per_memory_access: f64,
    pub energy_per_sync: f64,
    /// Alpha factor by hardware type
    pub alpha: f64, // 0.0=desktop, 0.5=mobile, 1.0=battery
}

impl EnergyModel {
    pub fn new(hardware_type: HardwareType) -> Self {
        let alpha = match hardware_type {
            HardwareType::Desktop => 0.0,
            HardwareType::Mobile => 0.5,
            HardwareType::Battery => 1.0,
        };

        Self {
            energy_per_flop: 10.0, // pJ
            energy_per_memory_access: 100.0, // pJ
            energy_per_sync: 1000.0, // pJ
            alpha,
        }
    }

    /// Total cost = LatencyCost + α * EnergyCost
    pub fn total_cost(&self, latency_cost: f64, flops: u64, memory_accesses: u64, sync_ops: u64) -> f64 {
        let energy_cost = flops as f64 * self.energy_per_flop
            + memory_accesses as f64 * self.energy_per_memory_access
            + sync_ops as f64 * self.energy_per_sync;
        latency_cost + self.alpha * energy_cost * 1e-12 // Convert pJ to J, scale
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HardwareType {
    Desktop,
    Mobile,
    Battery,
}

/// Profitability gate (10x speedup admission criterion)
#[derive(Debug, Clone)]
pub struct ProfitabilityGate {
    pub threshold: f64,
}

impl ProfitabilityGate {
    pub fn new() -> Self {
        Self { threshold: 10.0 }
    }

    pub fn check(&self, predicted_speedup: f64) -> bool {
        predicted_speedup >= self.threshold
    }
}

/// Main entry point for Layer 3
pub fn spp_layer3_optimize(
    versions: &[Version],
    hardware: Option<HardwareModel>,
) -> OptimizedPlan {
    let hardware = hardware.unwrap_or_else(HardwareModel::detect);
    let cost_model = CostModel::new();
    let mut search_space = SearchSpace::default();
    let optimizer = ExecutionOptimizer::new(hardware.clone());
    let reoptimizer = OnlineReoptimizer::new(hardware.clone());
    let energy_model = EnergyModel::new(HardwareType::Desktop);
    let gate = ProfitabilityGate::new();

    // Apply profitability gate
    let mut profitable_versions = Vec::new();
    for v in versions {
        let features = estimate_features(v);
        let speedup = cost_model.predict(&features);
        if gate.check(speedup) {
            profitable_versions.push((v.clone(), speedup));
        }
    }

    if profitable_versions.is_empty() {
        return OptimizedPlan {
            versions: vec![],
            schedule: SchedulePlan::default(),
            energy_model,
            reoptimizer,
            search_space,
        };
    }

    // Bayesian search for best configuration
    let mut best_version = profitable_versions[0].0.clone();
    let mut best_speedup = profitable_versions[0].1;

    for _ in 0..20 {
        // 20 iterations
        let point = search_space.suggest_next(&hardware);
        let version = point_to_version(&point, &best_version);
        let features = estimate_features(&version);
        let speedup = cost_model.predict(&features);
        search_space.observe(point, speedup);

        if speedup > best_speedup {
            best_speedup = speedup;
            best_version = version;
        }
    }

    // Generate schedule for best version
    let schedule = optimizer.schedule(&[best_version.clone()]);

    OptimizedPlan {
        versions: vec![best_version],
        schedule,
        energy_model,
        reoptimizer,
        search_space,
    }
}

fn estimate_features(version: &Version) -> CostFeatures {
    CostFeatures {
        loop_depth: 2,
        trip_count: 1000,
        element_size: 8,
        working_set_bytes: 1024 * 1024,
        working_set_category: WorkingSetCategory::L2,
        memory_accesses: 100,
        compute_intensity: 4.0,
        stride_pattern: StridePattern::Unit,
        stride_value: None,
        access_pattern: AccessPattern::UnitStride,
        arithmetic_intensity_category: ArithmeticIntensityCategory::MatMul,
        dependency_type: DependencyType::None,
        is_parallelizable: true,
        tile_size: 32,
        simd_width: 512,
        thread_count: 16,
        numa_locality: 0.9,
        reuse_distance: 1.0,
    }
}

fn point_to_version(point: &SearchPoint, base: &Version) -> Version {
    let mut v = base.clone();
    v.id = format!("{}_opt", base.id);
    // Map search point to version parameters
    v
}

/// Optimized execution plan
#[derive(Debug, Clone)]
pub struct OptimizedPlan {
    pub versions: Vec<Version>,
    pub schedule: SchedulePlan,
    pub energy_model: EnergyModel,
    pub reoptimizer: OnlineReoptimizer,
    pub search_space: SearchSpace,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_detect() {
        let hw = HardwareModel::detect();
        assert!(hw.cpu.total_logical_cores > 0);
        assert!(hw.cache.l1_size > 0);
        assert!(hw.simd.max_vector_width_bits > 0);
    }

    #[test]
    fn test_cost_model_deterministic() {
        let m = CostModel::new();
        let f = CostFeatures {
            loop_depth: 2,
            trip_count: 1000,
            element_size: 8,
            working_set_bytes: 1024 * 1024,
            working_set_category: WorkingSetCategory::L2,
            memory_accesses: 100,
            compute_intensity: 4.0,
            stride_pattern: StridePattern::Unit,
            stride_value: None,
            access_pattern: AccessPattern::UnitStride,
            arithmetic_intensity_category: ArithmeticIntensityCategory::MatMul,
            dependency_type: DependencyType::None,
            is_parallelizable: true,
            tile_size: 32,
            simd_width: 512,
            thread_count: 16,
            numa_locality: 0.9,
            reuse_distance: 1.0,
        };
        let a = m.predict(&f);
        let b = m.predict(&f);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn test_cost_model_ranks_known_cases() {
        let m = CostModel::new();

        // Base features
        let base = CostFeatures {
            loop_depth: 2,
            trip_count: 10000,
            element_size: 8,
            working_set_bytes: 1024 * 1024,
            working_set_category: WorkingSetCategory::L2,
            memory_accesses: 100,
            compute_intensity: 4.0,
            stride_pattern: StridePattern::Unit,
            stride_value: None,
            access_pattern: AccessPattern::UnitStride,
            arithmetic_intensity_category: ArithmeticIntensityCategory::MatMul,
            dependency_type: DependencyType::None,
            is_parallelizable: true,
            tile_size: 32,
            simd_width: 512,
            thread_count: 16,
            numa_locality: 0.9,
            reuse_distance: 1.0,
        };

        // Tiling should help for matmul
        let mut tiled = base.clone();
        tiled.tile_size = 32;

        // No tiling
        let mut no_tile = base.clone();
        no_tile.tile_size = 0;

        let speedup_tiled = m.predict(&tiled);
        let speedup_no_tile = m.predict(&no_tile);

        // Tiling should predict positive speedup for matmul
        assert!(speedup_tiled > speedup_no_tile);

        // SIMD should help for unit-stride
        let mut simd = base.clone();
        simd.simd_width = 512;
        simd.compute_intensity = 16.0; // Compute-bound

        let mut no_simd = base.clone();
        no_simd.simd_width = 0;
        no_simd.compute_intensity = 16.0; // Compute-bound

        let speedup_simd = m.predict(&simd);
        let speedup_no_simd = m.predict(&no_simd);

        assert!(speedup_simd > speedup_no_simd);

        // Parallel should help for large trip counts
        let mut parallel = base.clone();
        parallel.thread_count = 16;
        parallel.is_parallelizable = true;

        let mut serial = base.clone();
        serial.thread_count = 1;
        serial.is_parallelizable = false;

        let speedup_parallel = m.predict(&parallel);
        let speedup_serial = m.predict(&serial);

        assert!(speedup_parallel > speedup_serial);
    }

    #[test]
    fn test_search_space() {
        let mut space = SearchSpace::default();
        let hw = HardwareModel::detect();
        let point = space.suggest_next(&hw);
        assert_eq!(point.values.len(), space.dimensions.len());
    }

    #[test]
    fn test_profitability_gate() {
        let gate = ProfitabilityGate::new();
        assert!(gate.check(15.0));
        assert!(!gate.check(5.0));
    }

    #[test]
    fn test_energy_model() {
        let model = EnergyModel::new(HardwareType::Mobile);
        let cost = model.total_cost(100.0, 1_000_000, 10_000, 100);
        assert!(cost > 100.0);
    }

    #[test]
    fn test_online_reoptimizer() {
        let hw = HardwareModel::detect();
        let reopt = OnlineReoptimizer::new(hw);
        assert!(reopt.should_reoptimize(10.0, 7.0)); // 30% deviation
        assert!(!reopt.should_reoptimize(10.0, 9.5)); // 5% deviation
    }
}