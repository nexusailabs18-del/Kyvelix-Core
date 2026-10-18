//! SPP — Speculative Polyhedral Parallelization
//!
//! Layer 1: Dependence Engine
//! Layer 2: Parallel Version Generation
//! Layer 3: SPP-HX (Hierarchical Execution Optimizer)
//! Layer 4: Speculation Tiers
//! Layer 5: Acceleration

pub mod layer1;
pub mod layer2;
pub mod layer3;
pub mod layer4;
pub mod layer5;
pub mod common;

// Re-export layer1 types for public API
pub use layer1::{
    DependenceAnalysis, Layer1Config, LoopKind, EnumCaps, EffectSummary,
    DependenceType, Direction, ReductionKind, ScanType, NonAffineDependence,
    ParallelizationResult, MemoryOrder, AtomicOp, AliasInfo, ReductionInfo,
    RecurrenceInfo, CrossLoopDependence, DependenceEdge, RuntimeTest, OrderedDependence,
    spp_layer1_analyze,
};

// Re-export layer2 emit module, autotuner, runtime, and HardwareModel from layer3
pub use layer2::emit;
pub use layer2::autotuner;
pub use layer2::runtime;
pub use layer3::HardwareModel;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::LoopStub;

    #[test]
    fn test_layer1_entry() {
        let loop_ = LoopStub::new(1);
        let config = Layer1Config::default();
        let analysis = spp_layer1_analyze(&loop_, &config);
        assert_eq!(analysis.loop_hash, loop_.hash);
    }
}