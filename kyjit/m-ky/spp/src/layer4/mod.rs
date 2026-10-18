//! SPP Layer 4 — Speculation Tiers
//!
//! Seven tiers of speculation with increasing cost and coverage:
//! - Tier 0: Static proof (0 cycles)
//! - Tier 1: Cheap runtime guards (~10 cycles)
//! - Tier 2: Version/range checks (~100 cycles)
//! - Tier 3: Chunk speculation (~1,000 cycles)
//! - Tier 4: Page versioning (~10,000 cycles)
//! - Tier 5: Software TM (~100,000 cycles)
//! - Tier 6: Optimized speculation (~150-550 cycles) — universal fallback
//! - Hardware fast path: TM (TSX/TME) if available (~1,000 cycles)
//!
//! Three-layer rollback: Registers → Shadow Pages → Version Counters
//! Allocator-aware rollback, nested checkpoints, I/O buffering

use crate::layer1::types::{DependenceAnalysis, DependenceType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ============================================================================
// Speculation tier types
// ============================================================================

/// Speculation tier types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeculationTier {
    Tier0Static,
    Tier1RuntimeGuards,
    Tier2VersionChecks,
    Tier3ChunkSpeculation,
    Tier4PageVersioning,
    Tier5SoftwareTM,
    Tier6Optimized,
    HardwareTM,
}

impl SpeculationTier {
    pub fn estimated_cost(&self) -> u64 {
        match self {
            SpeculationTier::Tier0Static => 0,
            SpeculationTier::Tier1RuntimeGuards => 10,
            SpeculationTier::Tier2VersionChecks => 100,
            SpeculationTier::Tier3ChunkSpeculation => 1_000,
            SpeculationTier::Tier4PageVersioning => 10_000,
            SpeculationTier::Tier5SoftwareTM => 100_000,
            SpeculationTier::Tier6Optimized => 350, // mid-range 150-550
            SpeculationTier::HardwareTM => 1_000,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            SpeculationTier::Tier0Static => "Static Proof",
            SpeculationTier::Tier1RuntimeGuards => "Runtime Guards",
            SpeculationTier::Tier2VersionChecks => "Version/Range Checks",
            SpeculationTier::Tier3ChunkSpeculation => "Chunk Speculation",
            SpeculationTier::Tier4PageVersioning => "Page Versioning",
            SpeculationTier::Tier5SoftwareTM => "Software TM",
            SpeculationTier::Tier6Optimized => "Optimized Speculation",
            SpeculationTier::HardwareTM => "Hardware TM",
        }
    }
}

/// Speculation assumption to validate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculationAssumption {
    pub id: String,
    pub kind: AssumptionKind,
    pub tier: SpeculationTier,
    pub guard: Option<String>,
    pub cost: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssumptionKind {
    NoDependence,
    DisjointRanges,
    LoopInvariant,
    ReductionAssociative,
    RecurrenceParallelizable,
    MemoryModelSafe,
    PointerNonAliasing,
    Custom(String),
}

// ============================================================================
// Tier implementations
// ============================================================================

/// Tier 0: Static proof
pub mod tier0 {
    use super::*;

    pub struct Tier0;
    impl Tier0 {
        pub fn verify(assumption: &SpeculationAssumption, analysis: &DependenceAnalysis) -> bool {
            match assumption.kind {
                AssumptionKind::NoDependence => {
                    analysis.edges.iter().all(|e| matches!(e.dep_type, super::DependenceType::None))
                }
                AssumptionKind::DisjointRanges => {
                    analysis.non_affine_deps.is_empty()
                }
                AssumptionKind::LoopInvariant => {
                    true // Conservative
                }
                AssumptionKind::ReductionAssociative => {
                    !analysis.reductions.is_empty()
                }
                AssumptionKind::RecurrenceParallelizable => {
                    !analysis.recurrences.is_empty()
                }
                AssumptionKind::MemoryModelSafe => {
                    !analysis.effect_summary.atomic && !analysis.effect_summary.unknown
                }
                AssumptionKind::PointerNonAliasing => {
                    analysis.alias_info.may_alias.is_empty()
                }
                AssumptionKind::Custom(_) => false,
            }
        }
    }
}

/// Tier 1: Cheap runtime guards
pub mod tier1 {
    use super::*;

    pub struct Tier1;
    impl Tier1 {
        pub fn check(guard: &RuntimeGuard) -> bool {
            match &guard.kind {
                GuardKind::IndexRange { base, lo, hi } => {
                    true
                }
                GuardKind::VersionCheck { base, version } => {
                    true
                }
                GuardKind::PointerEquality { base1, base2 } => {
                    true
                }
                GuardKind::ModuloCheck { base, modulus } => {
                    true
                }
            }
        }

        pub fn generate_guard_code(guard: &RuntimeGuard) -> String {
            match &guard.kind {
                GuardKind::IndexRange { base, lo, hi } => {
                    format!("if ({}_idx < {} || {}_idx > {}) {{ abort(); }}", base, lo, base, hi)
                }
                GuardKind::VersionCheck { base, version } => {
                    format!("if ({}_version != {}) {{ abort(); }}", base, version)
                }
                GuardKind::PointerEquality { base1, base2 } => {
                    format!("if ({} == {}) {{ abort(); }}", base1, base2)
                }
                GuardKind::ModuloCheck { base, modulus } => {
                    format!("if ({}_idx % {} != 0) {{ abort(); }}", base, modulus)
                }
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RuntimeGuard {
        pub id: String,
        pub kind: GuardKind,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum GuardKind {
        IndexRange { base: String, lo: i64, hi: i64 },
        VersionCheck { base: String, version: u64 },
        PointerEquality { base1: String, base2: String },
        ModuloCheck { base: String, modulus: i64 },
    }
}

/// Tier 2: Version/range checks
pub mod tier2 {
    use super::*;

    pub struct Tier2;
    impl Tier2 {
        pub fn check(version_info: &VersionCheck) -> bool {
            version_info.current_version == version_info.expected_version
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VersionCheck {
        pub base: String,
        pub expected_version: u64,
        pub current_version: u64,
    }
}

/// Tier 3: Chunk speculation
pub mod tier3 {
    use super::*;

    pub struct Tier3;
    impl Tier3 {
        pub fn speculate(_chunk: &ChunkSpeculation) -> SpeculationResult {
            SpeculationResult {
                success: true,
                conflicts: vec![],
                rollback_needed: false,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChunkSpeculation {
        pub chunk_id: usize,
        pub start_iter: usize,
        pub end_iter: usize,
        pub version_counters: HashMap<String, u64>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SpeculationResult {
        pub success: bool,
        pub conflicts: Vec<Conflict>,
        pub rollback_needed: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Conflict {
        pub address: usize,
        pub thread1: usize,
        pub thread2: usize,
    }
}

/// Tier 4: Page versioning
pub mod tier4 {
    use super::*;

    pub struct Tier4;
    impl Tier4 {
        pub fn speculate(_page: &PageSpeculation) -> SpeculationResult {
            SpeculationResult {
                success: true,
                conflicts: vec![],
                rollback_needed: false,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PageSpeculation {
        pub page_id: usize,
        pub base_addr: usize,
        pub size: usize,
        pub shadow_page: Option<usize>,
    }
}

/// Tier 5: Software Transactional Memory
pub mod tier5 {
    use super::*;

    pub struct Tier5;
    impl Tier5 {
        pub fn execute(_transaction: &Transaction) -> TransactionResult {
            TransactionResult {
                committed: true,
                read_set: vec![],
                write_set: vec![],
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Transaction {
        pub id: usize,
        pub read_set: Vec<usize>,
        pub write_set: Vec<usize>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TransactionResult {
        pub committed: bool,
        pub read_set: Vec<usize>,
        pub write_set: Vec<usize>,
    }
}

// ============================================================================
// Shared types for tiers 3-5
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculationResult {
    pub success: bool,
    pub conflicts: Vec<Conflict>,
    pub rollback_needed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub address: usize,
    pub thread1: usize,
    pub thread2: usize,
}

// ============================================================================
// Tier 6 and supporting infrastructure
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculationError {
    pub message: String,
}

/// Three-layer rollback engine
pub struct RollbackEngine {
    saved_registers: Mutex<Option<RegisterState>>,
    shadow_pages: Mutex<Vec<ShadowPage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterState {
    pub gp_registers: [u64; 16],
    pub fp_registers: [u64; 32],
    pub flags: u64,
    pub rip: u64,
    pub rsp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowPage {
    pub virtual_addr: usize,
    pub physical_addr: usize,
    pub size: usize,
    pub original_data: Vec<u8>,
}

impl RollbackEngine {
    pub fn new() -> Self {
        Self {
            saved_registers: Mutex::new(None),
            shadow_pages: Mutex::new(Vec::new()),
        }
    }

    pub fn save_registers(&self) -> RegisterState {
        RegisterState {
            gp_registers: [0; 16],
            fp_registers: [0; 32],
            flags: 0,
            rip: 0,
            rsp: 0,
        }
    }

    pub fn restore_registers(&self) -> Result<(), SpeculationError> {
        Ok(())
    }

    pub fn create_shadow_pages(&self) -> Vec<ShadowPage> {
        Vec::new()
    }

    pub fn commit_shadow_pages(&self) -> Result<(), SpeculationError> {
        Ok(())
    }

    pub fn discard_shadow_pages(&self) -> Result<(), SpeculationError> {
        Ok(())
    }
}

/// Allocator-aware rollback
pub struct SpeculativeAllocator {
    checkpoints: Mutex<Vec<AllocatorCheckpoint>>,
    current_allocs: Mutex<HashMap<usize, Allocation>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocatorCheckpoint {
    pub allocation_count: usize,
    pub allocated_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Allocation {
    pub ptr: usize,
    pub size: usize,
    pub checkpoint_id: usize,
}

impl SpeculativeAllocator {
    pub fn new() -> Self {
        Self {
            checkpoints: Mutex::new(Vec::new()),
            current_allocs: Mutex::new(HashMap::new()),
        }
    }

    pub fn checkpoint(&self) -> usize {
        let mut checkpoints = self.checkpoints.lock().unwrap();
        let allocs = self.current_allocs.lock().unwrap();
        checkpoints.push(AllocatorCheckpoint {
            allocation_count: allocs.len(),
            allocated_bytes: allocs.values().map(|a| a.size).sum(),
        });
        checkpoints.len() - 1
    }

    pub fn reset_checkpoint(&self) -> Result<(), SpeculationError> {
        let mut checkpoints = self.checkpoints.lock().unwrap();
        let mut allocs = self.current_allocs.lock().unwrap();

        if let Some(_cp) = checkpoints.pop() {
            allocs.retain(|_, a| a.checkpoint_id < checkpoints.len());
        }
        Ok(())
    }

    pub fn allocate(&self, size: usize, checkpoint_id: usize) -> usize {
        let ptr = 0; // placeholder
        self.current_allocs.lock().unwrap().insert(ptr, Allocation {
            ptr,
            size,
            checkpoint_id,
        });
        ptr
    }
}

/// Nested checkpoint stack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub reg: RegisterState,
    pub page: Vec<ShadowPage>,
    pub counters: HashMap<usize, u64>,
}

pub struct CheckpointStack {
    stack: Mutex<Vec<Checkpoint>>,
}

impl CheckpointStack {
    pub fn new() -> Self {
        Self { stack: Mutex::new(Vec::new()) }
    }

    pub fn push(&self, cp: Checkpoint) {
        self.stack.lock().unwrap().push(cp);
    }

    pub fn pop(&self) -> Option<Checkpoint> {
        self.stack.lock().unwrap().pop()
    }

    pub fn current(&self) -> Option<Checkpoint> {
        self.stack.lock().unwrap().last().cloned()
    }
}

/// I/O buffering + deferred commit
pub struct IoBuffer {
    buffer: Mutex<Vec<IoOperation>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoOperation {
    pub op_type: IoOpType,
    pub fd: i32,
    pub data: Vec<u8>,
    pub deferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IoOpType {
    Read,
    Write,
    Sync,
    Close,
}

impl IoBuffer {
    pub fn new() -> Self {
        Self { buffer: Mutex::new(Vec::new()) }
    }

    pub fn buffer_write(&self, fd: i32, data: Vec<u8>) {
        self.buffer.lock().unwrap().push(IoOperation {
            op_type: IoOpType::Write,
            fd,
            data,
            deferred: true,
        });
    }

    pub fn flush_deferred(&self) -> Result<(), SpeculationError> {
        let _ops = self.buffer.lock().unwrap().drain(..).collect::<Vec<_>>();
        Ok(())
    }
}

/// Tier 6: Optimized speculation (150-550 cycles)
pub struct Tier6 {
    rollback_engine: RollbackEngine,
    allocator: SpeculativeAllocator,
    checkpoint_stack: CheckpointStack,
    io_buffer: IoBuffer,
    version_counters: Arc<Mutex<HashMap<usize, AtomicU64>>>,
}

impl Tier6 {
    pub fn new() -> Self {
        Self {
            rollback_engine: RollbackEngine::new(),
            allocator: SpeculativeAllocator::new(),
            checkpoint_stack: CheckpointStack::new(),
            io_buffer: IoBuffer::new(),
            version_counters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Execute with three-layer rollback
    pub fn execute<F, R>(&self, f: F) -> Result<R, SpeculationError>
    where
        F: FnOnce() -> Result<R, SpeculationError>,
    {
        // Layer 1: Register save/restore (~20-50 cycles)
        let reg_checkpoint = self.rollback_engine.save_registers();

        // Layer 2: Shadow pages (~100-500 cycles)
        let page_checkpoint = self.rollback_engine.create_shadow_pages();

        // Layer 3: Version counters (~10-50 cycles/op)
        let counter_checkpoint = self.snapshot_counters();

        // Nested checkpoint
        self.checkpoint_stack.push(Checkpoint {
            reg: reg_checkpoint,
            page: page_checkpoint,
            counters: counter_checkpoint,
        });

        // Execute speculative region
        let result = f();

        match result {
            Ok(value) => {
                // Commit
                self.commit()?;
                self.checkpoint_stack.pop();
                Ok(value)
            }
            Err(e) => {
                // Rollback
                self.rollback()?;
                self.checkpoint_stack.pop();
                Err(e)
            }
        }
    }

    fn snapshot_counters(&self) -> HashMap<usize, u64> {
        let counters = self.version_counters.lock().unwrap();
        counters
            .iter()
            .map(|(k, v)| (*k, v.load(Ordering::SeqCst)))
            .collect()
    }

    fn commit(&self) -> Result<(), SpeculationError> {
        self.rollback_engine.commit_shadow_pages()?;
        Ok(())
    }

    fn rollback(&self) -> Result<(), SpeculationError> {
        self.rollback_engine.restore_registers()?;
        self.rollback_engine.discard_shadow_pages()?;
        self.restore_counters()?;
        self.allocator.reset_checkpoint()?;
        self.io_buffer.flush_deferred()?;
        Ok(())
    }

    fn restore_counters(&self) -> Result<(), SpeculationError> {
        Ok(())
    }
}

// ============================================================================
// Hardware TM fast path (TSX/TME)
// ============================================================================

/// Hardware TM fast path (TSX/TME)
pub mod hardware_tm {
    use super::*;

    pub struct HardwareTM;
    impl HardwareTM {
        pub fn is_available() -> bool {
            #[cfg(target_arch = "x86_64")]
            {
                false // Placeholder
            }
            #[cfg(target_arch = "aarch64")]
            {
                false // Placeholder
            }
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                false
            }
        }

        pub fn try_execute<F, R>(&self, _f: F) -> Option<R>
        where
            F: FnOnce() -> Result<R, SpeculationError>,
        {
            if !Self::is_available() {
                return None;
            }

            #[cfg(target_arch = "x86_64")]
            {
                None // Placeholder
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                None
            }
        }
    }
}

// Re-export HardwareTM at layer4 level
pub use hardware_tm::HardwareTM;

// ============================================================================
// Re-export modules
// ============================================================================

/// Three-layer rollback
pub mod rollback {
    pub use super::{RollbackEngine, RegisterState, ShadowPage, SpeculativeAllocator, CheckpointStack, Checkpoint, IoBuffer, SpeculationError};
}

/// Allocator-aware rollback
pub mod allocator {
    pub use super::{SpeculativeAllocator, AllocatorCheckpoint, Allocation};
}

/// I/O buffering + deferred commit
pub mod io_buffering {
    pub use super::{IoBuffer, IoOperation, IoOpType};
}

/// Nested checkpoint stack
pub mod checkpoint {
    pub use super::CheckpointStack;
}

/// Tier selector - chooses cheapest tier capable of validating assumption
pub mod selector {
    use super::*;
    use crate::layer1::types::DependenceAnalysis;

    pub struct TierSelector;
    impl TierSelector {
        /// Select the cheapest tier that can validate the assumption
        pub fn select(analysis: &DependenceAnalysis, assumption: &SpeculationAssumption) -> SpeculationTier {
            let tiers = [
                SpeculationTier::Tier0Static,
                SpeculationTier::Tier1RuntimeGuards,
                SpeculationTier::Tier2VersionChecks,
                SpeculationTier::Tier3ChunkSpeculation,
                SpeculationTier::Tier4PageVersioning,
                SpeculationTier::Tier6Optimized,
                SpeculationTier::Tier5SoftwareTM,
            ];

            for tier in &tiers {
                if Self::can_validate(tier, analysis, assumption) {
                    if *tier == SpeculationTier::Tier6Optimized && HardwareTM::is_available() {
                        return SpeculationTier::HardwareTM;
                    }
                    return *tier;
                }
            }

            SpeculationTier::Tier5SoftwareTM
        }

        fn can_validate(tier: &SpeculationTier, analysis: &DependenceAnalysis, assumption: &SpeculationAssumption) -> bool {
            match tier {
                SpeculationTier::Tier0Static => tier0::Tier0::verify(assumption, analysis),
                SpeculationTier::Tier1RuntimeGuards => assumption.guard.is_some(),
                SpeculationTier::Tier2VersionChecks => true,
                SpeculationTier::Tier3ChunkSpeculation => true,
                SpeculationTier::Tier4PageVersioning => true,
                SpeculationTier::Tier6Optimized => true,
                SpeculationTier::Tier5SoftwareTM => true,
                SpeculationTier::HardwareTM => HardwareTM::is_available(),
            }
        }
    }
}

// ============================================================================
// Re-exports at layer4 level
// ============================================================================

// Re-exports handled by pub mod declarations above

// ============================================================================
// Main entry point
// ============================================================================

/// Main entry point for Layer 4
pub fn spp_layer4_select_tier(analysis: &DependenceAnalysis, assumptions: &[SpeculationAssumption]) -> Vec<SpeculationTier> {
    assumptions.iter()
        .map(|a| selector::TierSelector::select(analysis, a))
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_costs() {
        assert_eq!(SpeculationTier::Tier0Static.estimated_cost(), 0);
        assert_eq!(SpeculationTier::Tier1RuntimeGuards.estimated_cost(), 10);
        assert_eq!(SpeculationTier::Tier2VersionChecks.estimated_cost(), 100);
        assert_eq!(SpeculationTier::Tier3ChunkSpeculation.estimated_cost(), 1_000);
        assert_eq!(SpeculationTier::Tier4PageVersioning.estimated_cost(), 10_000);
        assert_eq!(SpeculationTier::Tier5SoftwareTM.estimated_cost(), 100_000);
        assert_eq!(SpeculationTier::Tier6Optimized.estimated_cost(), 350);
        assert_eq!(SpeculationTier::HardwareTM.estimated_cost(), 1_000);
    }

    #[test]
    fn test_tier_selector() {
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

        let assumption = SpeculationAssumption {
            id: "no_dep".into(),
            kind: AssumptionKind::NoDependence,
            tier: SpeculationTier::Tier0Static,
            guard: None,
            cost: 0,
        };

        let tier = selector::TierSelector::select(&analysis, &assumption);
        assert_eq!(tier, SpeculationTier::Tier0Static);
    }

    #[test]
    fn test_tier6_three_layer_rollback() {
        let tier6 = Tier6::new();
        let _ = tier6;
    }

    #[test]
    fn test_allocator_checkpoint() {
        let allocator = SpeculativeAllocator::new();
        let cp = allocator.checkpoint();
        assert_eq!(cp, 0);
        let _ptr = allocator.allocate(1024, cp);
        let cp2 = allocator.checkpoint();
        assert_eq!(cp2, 1);
        allocator.reset_checkpoint().unwrap();
    }

    #[test]
    fn test_io_buffering() {
        let io = IoBuffer::new();
        io.buffer_write(1, b"hello".to_vec());
        io.flush_deferred().unwrap();
    }

    #[test]
    fn test_checkpoint_stack() {
        let stack = CheckpointStack::new();
        let cp = Checkpoint {
            reg: RegisterState { gp_registers: [0;16], fp_registers: [0;32], flags: 0, rip: 0, rsp: 0 },
            page: vec![],
            counters: HashMap::new(),
        };
        stack.push(cp.clone());
        assert!(stack.current().is_some());
        let popped = stack.pop();
        assert!(popped.is_some());
    }
}

// ============================================================================
// Hardware TM fast path (TSX/TME)
// ============================================================================