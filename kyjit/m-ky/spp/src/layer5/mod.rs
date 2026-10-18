//! SPP Layer 5 — Acceleration
//!
//! Acceleration components:
//! - Portable vector IR (LLVM VPlan / MLIR vector dialect)
//! - Software prefetch
//! - Cache-blocked tiling
//! - GPU transfer cost model (PCIe vs compute tradeoff)
//! - CPU/GPU partitioning
//! - Concurrency engine (scheduling, sync, reductions, alignment, strip-mining)
//! - Vectorization engine (SIMD, conditionals, reductions, cycle-breaking)

use crate::layer1::types::DependenceAnalysis;
use crate::layer2::{Version, Transformation, TransformationKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Portable vector IR (LLVM VPlan / MLIR vector dialect)
pub mod vector_ir {
    use super::*;

    /// Vector IR representation
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VectorIR {
        pub operations: Vec<VectorOp>,
        pub vector_width: usize,
        pub mask_register: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum VectorOp {
        Add { dst: String, src1: String, src2: String },
        Mul { dst: String, src1: String, src2: String },
        FAdd { dst: String, src1: String, src2: String },
        FMul { dst: String, src1: String, src2: String },
        Load { dst: String, base: String, offset: String, mask: Option<String> },
        Store { base: String, offset: String, src: String, mask: Option<String> },
        Compare { dst: String, src1: String, src2: String, predicate: ComparePredicate },
        Blend { dst: String, mask: String, true_val: String, false_val: String },
        Reduce { dst: String, src: String, op: ReduceOp },
        Shuffle { dst: String, src: String, indices: Vec<usize> },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum ComparePredicate {
        Eq, Lt, Le, Gt, Ge, Ne,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum ReduceOp {
        Add, Mul, FAdd, FMul, Min, Max, And, Or,
    }

    impl VectorIR {
        pub fn new(vector_width: usize) -> Self {
            Self {
                operations: Vec::new(),
                vector_width,
                mask_register: None,
            }
        }

        pub fn add_op(&mut self, op: VectorOp) {
            self.operations.push(op);
        }

        /// Select vector length based on hardware
        pub fn select_vector_length(hardware_width: usize, trip_count: usize) -> usize {
            hardware_width.min(trip_count * 8) // Simplified
        }
    }
}

/// Software prefetch
pub mod prefetch {
    use super::*;

    /// Prefetcher for cache optimization
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Prefetcher {
        pub distance: usize,
        pub strategy: PrefetchStrategy,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum PrefetchStrategy {
        /// Prefetch next N iterations
        NextIterations(usize),
        /// Prefetch based on reuse distance
        ReuseDistance(usize),
        /// Adaptive based on miss rate
        Adaptive,
    }

    impl Prefetcher {
        pub fn new() -> Self {
            Self {
                distance: 4,
                strategy: PrefetchStrategy::NextIterations(4),
            }
        }

        /// Generate prefetch instructions
        pub fn generate_prefetches(&self, loop_nest: &[usize], base_addr: &str, stride: usize) -> Vec<String> {
            let mut prefetches = Vec::new();
            for level in loop_nest {
                match &self.strategy {
                    PrefetchStrategy::NextIterations(n) => {
                        prefetches.push(format!(
                            "prefetch {}, {} bytes ahead",
                            base_addr,
                            n * stride * level
                        ));
                    }
                    PrefetchStrategy::ReuseDistance(d) => {
                        prefetches.push(format!(
                            "prefetch {}, {} bytes reuse distance",
                            base_addr,
                            d * stride
                        ));
                    }
                    PrefetchStrategy::Adaptive => {
                        prefetches.push(format!("prefetch {} adaptive", base_addr));
                    }
                }
            }
            prefetches
        }
    }
}

/// Cache-blocked tiling
pub mod tiling {
    use super::*;

    /// Cache-blocked tiler
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Tiler {
        pub cache_sizes: Vec<usize>,
        pub tile_sizes: Vec<usize>,
    }

    impl Tiler {
        pub fn new(cache_sizes: Vec<usize>) -> Self {
            Self {
                cache_sizes,
                tile_sizes: Vec::new(),
            }
        }

        /// Compute optimal tile sizes for cache hierarchy
        pub fn compute_tile_sizes(&mut self, element_size: usize, num_arrays: usize) -> Vec<usize> {
            let mut tiles = Vec::new();
            let remaining_cache = self.cache_sizes.iter().cloned().collect::<Vec<_>>();

            for (i, &cache) in self.cache_sizes.iter().enumerate() {
                // Reserve cache for working set
                let working_set = element_size * num_arrays;
                let available = cache.saturating_sub(working_set);
                let tile = (available / (element_size * 2)).max(16);
                tiles.push(tile);
            }

            self.tile_sizes = tiles.clone();
            tiles
        }

        /// Generate tiled loop nest
        pub fn generate_tiled_loop(&self, original_loops: &[LoopBounds]) -> Vec<LoopBounds> {
            let mut tiled = Vec::new();

            for (i, bounds) in original_loops.iter().enumerate() {
                if i < self.tile_sizes.len() {
                    let tile = self.tile_sizes[i];
                    tiled.push(LoopBounds {
                        lo: bounds.lo,
                        hi: bounds.hi,
                        step: bounds.step,
                        outer: Some(Box::new(bounds.clone())),
                        inner: Some(Box::new(LoopBounds::new(0, tile, 1))),
                        tile_size: tile,
                    });
                } else {
                    tiled.push(bounds.clone());
                }
            }

            tiled
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LoopBounds {
        pub lo: usize,
        pub hi: usize,
        pub step: usize,
        pub outer: Option<Box<LoopBounds>>,
        pub inner: Option<Box<LoopBounds>>,
        pub tile_size: usize,
    }

    impl LoopBounds {
        pub fn new(lo: usize, hi: usize, step: usize) -> Self {
            Self {
                lo,
                hi,
                step,
                outer: None,
                inner: None,
                tile_size: 0,
            }
        }
    }
}

/// GPU transfer cost model
pub mod gpu_cost_model {
    use super::*;

    /// GPU cost model for offload decisions
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct GpuCostModel {
        pub pcie_bandwidth_gb_s: f64,
        pub gpu_compute_tflops: f64,
        pub cpu_compute_tflops: f64,
        pub transfer_overhead_us: f64,
        pub kernel_launch_overhead_us: f64,
    }

    impl GpuCostModel {
        pub fn new() -> Self {
            Self {
                pcie_bandwidth_gb_s: 16.0,  // PCIe 4.0 x16
                gpu_compute_tflops: 10.0,
                cpu_compute_tflops: 1.0,
                transfer_overhead_us: 5.0,
                kernel_launch_overhead_us: 10.0,
            }
        }

        /// Total time = Transfer + Compute
        pub fn total_time(&self, data_size_bytes: usize, flops: usize) -> f64 {
            let t_transfer = data_size_bytes as f64 / (self.pcie_bandwidth_gb_s * 1e9) + self.transfer_overhead_us * 1e-6;
            let t_compute = (flops as f64 / (self.gpu_compute_tflops * 1e12)) + self.kernel_launch_overhead_us * 1e-6;
            t_transfer + t_compute
        }

        /// CPU time for comparison
        pub fn cpu_time(&self, flops: usize) -> f64 {
            flops as f64 / (self.cpu_compute_tflops * 1e12)
        }

        /// Offload decision
        pub fn should_offload(&self, data_size_bytes: usize, flops: usize) -> bool {
            self.total_time(data_size_bytes, flops) < self.cpu_time(flops)
        }

        /// Speedup estimate
        pub fn speedup(&self, data_size_bytes: usize, flops: usize) -> f64 {
            self.cpu_time(flops) / self.total_time(data_size_bytes, flops)
        }
    }
}

/// CPU/GPU partitioning
pub mod cpu_gpu_partition {
    use super::*;

    /// CPU/GPU partitioner
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Partitioner {
        pub gpu_model: crate::layer5::gpu_cost_model::GpuCostModel,
    }

    impl Partitioner {
        pub fn new(gpu_model: crate::layer5::gpu_cost_model::GpuCostModel) -> Self {
            Self { gpu_model }
        }

        /// Partition work between CPU and GPU
        pub fn partition(&self, total_work: usize, data_size: usize) -> Partition {
            let gpu_fraction = self.compute_gpu_fraction(total_work, data_size);
            let gpu_work = (total_work as f64 * gpu_fraction) as usize;
            let cpu_work = total_work - gpu_work;

            Partition { cpu_work, gpu_work, gpu_fraction }
        }

        fn compute_gpu_fraction(&self, work: usize, data_size: usize) -> f64 {
            if !self.gpu_model.should_offload(data_size, work) {
                return 0.0;
            }

            // Heuristic: more work -> more GPU
            let work_factor = (work as f64 / 1e6).min(1.0);
            let speedup = self.gpu_model.speedup(data_size, work);
            (work_factor * speedup / 10.0).min(1.0)
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Partition {
        pub cpu_work: usize,
        pub gpu_work: usize,
        pub gpu_fraction: f64,
    }
}

/// Concurrency engine (scheduling, sync, reductions, alignment, strip-mining)
pub mod concurrency {
    use super::*;

    /// Concurrency engine
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ConcurrencyEngine {
        pub hardware: crate::layer3::HardwareModel,
    }

    impl ConcurrencyEngine {
        pub fn new(hardware: crate::layer3::HardwareModel) -> Self {
            Self { hardware }
        }

        /// Generate scheduling code
        pub fn generate_schedule(&self, policy: SchedulePolicy, work: usize) -> ScheduleCode {
            match policy {
                SchedulePolicy::Static => ScheduleCode {
                    code: format!("static_schedule({})", work),
                    overhead_cycles: 10,
                },
                SchedulePolicy::Dynamic => ScheduleCode {
                    code: format!("dynamic_schedule({})", work),
                    overhead_cycles: 100,
                },
                SchedulePolicy::Guided => ScheduleCode {
                    code: format!("guided_schedule({})", work),
                    overhead_cycles: 50,
                },
                SchedulePolicy::WorkStealing => ScheduleCode {
                    code: format!("work_stealing_schedule({})", work),
                    overhead_cycles: 200,
                },
            }
        }

        /// Generate synchronization primitives
        pub fn generate_sync(&self, kind: SyncKind) -> SyncCode {
            match kind {
                SyncKind::Barrier => SyncCode {
                    code: "barrier()".into(),
                    cycles: self.hardware.sync.barrier_cycles,
                },
                SyncKind::Atomic => SyncCode {
                    code: "atomic_op()".into(),
                    cycles: self.hardware.sync.atomic_cycles,
                },
                SyncKind::LockFree => SyncCode {
                    code: "lock_free_queue()".into(),
                    cycles: self.hardware.sync.atomic_cycles * 2,
                },
            }
        }

        /// Generate reduction code
        pub fn generate_reduction(&self, op: ReduceOp, data: &str, workers: usize) -> String {
            match op {
                ReduceOp::Sum => format!("parallel_sum({}, {})", data, workers),
                ReduceOp::Product => format!("parallel_product({}, {})", data, workers),
                ReduceOp::Min => format!("parallel_min({}, {})", data, workers),
                ReduceOp::Max => format!("parallel_max({}, {})", data, workers),
                ReduceOp::Custom(name) => format!("parallel_reduce_{}({}, {})", name, data, workers),
            }
        }

        /// Alignment for dependence resolution
        pub fn align_dependences(&self, dependences: &[Dependence]) -> AlignmentResult {
            // Build alignment graph and check for zero-distance cycles
            let has_zero_cycle = dependences.iter().any(|d| d.distance == 0 && d.cyclic);

            AlignmentResult {
                aligned: !has_zero_cycle,
                transformations: if has_zero_cycle { vec![] } else { vec!["alignment".into()] },
            }
        }

        /// Strip-mining for parallelism
        pub fn strip_mine(&self, loop_bounds: LoopBounds, dependence_distance: usize) -> StripMinedLoop {
            let strip_size = dependence_distance.max(1);
            StripMinedLoop {
                strip_loop: LoopBounds::new(loop_bounds.lo, loop_bounds.hi, strip_size),
                element_loop: LoopBounds::new(0, strip_size, 1),
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum SchedulePolicy {
        Static,
        Dynamic,
        Guided,
        WorkStealing,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScheduleCode {
        pub code: String,
        pub overhead_cycles: u64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum SyncKind {
        Barrier,
        Atomic,
        LockFree,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SyncCode {
        pub code: String,
        pub cycles: u64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum ReduceOp {
        Sum,
        Product,
        Min,
        Max,
        Custom(String),
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Dependence {
        pub distance: usize,
        pub cyclic: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AlignmentResult {
        pub aligned: bool,
        pub transformations: Vec<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LoopBounds {
        pub lo: usize,
        pub hi: usize,
        pub step: usize,
    }

    impl LoopBounds {
        pub fn new(lo: usize, hi: usize, step: usize) -> Self {
            Self { lo, hi, step }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StripMinedLoop {
        pub strip_loop: LoopBounds,
        pub element_loop: LoopBounds,
    }
}

/// Vectorization engine (SIMD, conditionals, reductions, cycle-breaking)
pub mod vectorization {
    use super::*;

    /// Vectorization engine
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VectorizationEngine {
        pub hardware_width: usize,
    }

    impl VectorizationEngine {
        pub fn new(hardware_width: usize) -> Self {
            Self { hardware_width }
        }

        /// Check if loop can be vectorized
        pub fn can_vectorize(&self, analysis: &DependenceAnalysis) -> bool {
            // Vector execution preserves lexically forward dependences
            // Only sequential loops with acyclic dependence graphs can be vectorized
            let acyclic = analysis.edges.iter().all(|e| {
                !matches!(e.dep_type, crate::layer1::types::DependenceType::Exact(_) | crate::layer1::types::DependenceType::Bounded(_))
                    || e.direction.iter().all(|d| matches!(d, crate::layer1::types::Direction::Eq | crate::layer1::types::Direction::Gt))
            });
            acyclic
        }

        /// Generate vectorized code
        pub fn vectorize(&self, loop_body: &str, simd_width: usize) -> VectorizedCode {
            let vl = simd_width.min(self.hardware_width);
            VectorizedCode {
                code: format!("vectorized({}, {})", loop_body, vl),
                vector_length: vl,
                masked: false,
            }
        }

        /// Handle conditionals with masking
        pub fn vectorize_conditional(&self, condition: &str, true_body: &str, false_body: &str) -> VectorizedCode {
            VectorizedCode {
                code: format!("vectorized_if({}, {}, {})", condition, true_body, false_body),
                vector_length: self.hardware_width,
                masked: true,
            }
        }

        /// Vector reduction
        pub fn vector_reduce(&self, op: VectorReduceOp, data: &str) -> String {
            match op {
                VectorReduceOp::Sum => format!("vector_sum({})", data),
                VectorReduceOp::Product => format!("vector_product({})", data),
                VectorReduceOp::Min => format!("vector_min({})", data),
                VectorReduceOp::Max => format!("vector_max({})", data),
            }
        }

        /// Handle dependence cycles
        pub fn break_cycles(&self, cycles: &[CycleInfo]) -> CycleBreakingResult {
            let mut result = CycleBreakingResult::default();
            for cycle in cycles {
                if cycle.is_anti_dependence {
                    result.strategies.push("anti_dependence_split".into());
                } else if cycle.multiple_statements {
                    result.strategies.push("operation_granularity".into());
                } else if cycle.partial_expression {
                    result.strategies.push("partial_expr_vectorization".into());
                } else {
                    result.strategies.push("strip_mining".into());
                }
            }
            result
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VectorizedCode {
        pub code: String,
        pub vector_length: usize,
        pub masked: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum VectorReduceOp {
        Sum,
        Product,
        Min,
        Max,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CycleInfo {
        pub is_anti_dependence: bool,
        pub multiple_statements: bool,
        pub partial_expression: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct CycleBreakingResult {
        pub strategies: Vec<String>,
    }
}

/// Main entry point for Layer 5
pub fn spp_layer5_accelerate(
    versions: &[Version],
    analysis: &DependenceAnalysis,
    hardware: &crate::layer3::HardwareModel,
) -> (AcceleratedPlan, RuntimeAcceleration) {
    let vector_ir = vector_ir::VectorIR::new(hardware.simd.max_vector_width_bits / 8);
    let prefetcher = prefetch::Prefetcher::new();
    let mut tiler = tiling::Tiler::new(vec![hardware.cache.l1_size, hardware.cache.l2_size, hardware.cache.l3_size]);
    let gpu_model = gpu_cost_model::GpuCostModel::new();
    let partitioner = cpu_gpu_partition::Partitioner::new(gpu_model.clone());
    let concurrency = concurrency::ConcurrencyEngine::new(hardware.clone());
    let vectorization = vectorization::VectorizationEngine::new(hardware.simd.max_vector_width_bits / 8);

    let mut accelerated_versions = Vec::new();

    for v in versions {
        let accelerated = accelerate_version(v, analysis, &concurrency, &vectorization, &partitioner, &tiler, &gpu_model);
        accelerated_versions.push(accelerated);
    }

    let plan = AcceleratedPlan {
        versions: accelerated_versions,
        vector_ir,
        gpu_model,
    };

    let runtime = RuntimeAcceleration {
        prefetcher,
        tiler,
        partitioner,
        concurrency,
        vectorization,
    };

    (plan, runtime)
}

fn accelerate_version(
    version: &Version,
    analysis: &DependenceAnalysis,
    concurrency: &concurrency::ConcurrencyEngine,
    vectorization: &vectorization::VectorizationEngine,
    partitioner: &cpu_gpu_partition::Partitioner,
    tiler: &tiling::Tiler,
    gpu_model: &gpu_cost_model::GpuCostModel,
) -> AcceleratedVersion {
    let schedule = concurrency.generate_schedule(concurrency::SchedulePolicy::WorkStealing, 1000);
    let partition = partitioner.partition(1000, 1024 * 1024);

    let can_vectorize = vectorization.can_vectorize(analysis);
    let vectorized = if can_vectorize {
        let _vcode = vectorization.vectorize(&version.code, 512);
        Some(vector_ir::VectorIR::new(512))
    } else {
        None
    };

    let sync = concurrency.generate_sync(concurrency::SyncKind::Barrier);
    let reduction = concurrency.generate_reduction(concurrency::ReduceOp::Sum, "data", 16);

    AcceleratedVersion {
        original_version: version.clone(),
        schedule,
        partition,
        vectorized,
        sync,
        reduction_code: reduction,
    }
}

/// Accelerated execution plan (serializable subset)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratedPlan {
    pub versions: Vec<AcceleratedVersion>,
    pub vector_ir: vector_ir::VectorIR,
    pub gpu_model: gpu_cost_model::GpuCostModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratedVersion {
    pub original_version: Version,
    pub schedule: concurrency::ScheduleCode,
    pub partition: cpu_gpu_partition::Partition,
    pub vectorized: Option<vector_ir::VectorIR>,
    pub sync: concurrency::SyncCode,
    pub reduction_code: String,
}

/// Runtime acceleration components (not serialized)
pub struct RuntimeAcceleration {
    pub prefetcher: prefetch::Prefetcher,
    pub tiler: tiling::Tiler,
    pub partitioner: cpu_gpu_partition::Partitioner,
    pub concurrency: concurrency::ConcurrencyEngine,
    pub vectorization: vectorization::VectorizationEngine,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_ir() {
        let mut ir = vector_ir::VectorIR::new(512);
        ir.add_op(vector_ir::VectorOp::Add { dst: "v1".into(), src1: "v2".into(), src2: "v3".into() });
        assert_eq!(ir.operations.len(), 1);
    }

    #[test]
    fn test_prefetch() {
        let prefetcher = prefetch::Prefetcher::new();
        let prefetches = prefetcher.generate_prefetches(&[1, 2, 3], "base", 64);
        assert_eq!(prefetches.len(), 3);
    }

    #[test]
    fn test_tiling() {
        let mut tiler = tiling::Tiler::new(vec![32 * 1024, 256 * 1024, 32 * 1024 * 1024]);
        let tiles = tiler.compute_tile_sizes(8, 4);
        assert_eq!(tiles.len(), 3);
        assert!(tiles.iter().all(|&t| t >= 16));
    }

    #[test]
    fn test_gpu_cost_model() {
        let model = gpu_cost_model::GpuCostModel::new();
        let offload = model.should_offload(1024 * 1024, 1_000_000);
        let speedup = model.speedup(1024 * 1024, 1_000_000);
        assert!(speedup > 0.0);
    }

    #[test]
    fn test_cpu_gpu_partition() {
        let model = gpu_cost_model::GpuCostModel::new();
        let partitioner = cpu_gpu_partition::Partitioner::new(model);
        let partition = partitioner.partition(10000, 1024 * 1024);
        assert!(partition.cpu_work + partition.gpu_work == 10000);
    }

    #[test]
    fn test_concurrency_schedule() {
        let hw = crate::layer3::HardwareModel::detect();
        let engine = concurrency::ConcurrencyEngine::new(hw);
        let schedule = engine.generate_schedule(concurrency::SchedulePolicy::WorkStealing, 1000);
        assert!(!schedule.code.is_empty());
    }

    #[test]
    fn test_vectorization() {
        let hw = crate::layer3::HardwareModel::detect();
        let engine = vectorization::VectorizationEngine::new(hw.simd.max_vector_width_bits / 8);
        let analysis = DependenceAnalysis {
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
        assert!(engine.can_vectorize(&analysis));
    }
}