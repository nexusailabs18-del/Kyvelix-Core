//! Phase 6 — Array regions (Wolfe Chapter 8.1).
//!
//! The array IR (`crate::ir::Access`) answers "which element does this access
//! touch on this iteration?". Phase 6 answers the aggregate questions a loop or
//! function has to answer before points-to (Phase 7) or interprocedural
//! analysis (Phase 8) can run:
//!
//! - [`bounds`] — §8.1.2 *simple bounds*: the conservative box a set of accesses
//!   sweeps, with union and intersection.
//! - [`triplet`] — §8.1.3 *triplet notation*: `[l:u:s]` regions that keep the
//!   stride of a strided access, so `A[2i]` is not reported as touching `A[1]`.
//!   Union is exact via a three-argument gcd; intersection solves a Diophantine
//!   equation per dimension using Phase 2's [`crate::solver::gcd`].
//! - [`dataflow`] — §8.1.1 `MayUse` / `MayMod` / `Kill`, the meet of `Kill` being
//!   an intersection over predecessors.
//! - [`kill`] — §8.1.4 the array kill analysis: the access-matrix rank test for
//!   "each iteration writes a different element", and the coverage test for
//!   "this write kills that one".
//!
//! ## Regions are derived, never asserted
//!
//! Every region here is computed by enumerating or evaluating an `Access` over
//! the `IV` domain the frontend recorded, and the §6.2 checker arms recompute
//! the same thing from the loop on disk. An analyzer that mis-derives a region
//! therefore produces a certificate its own checker rejects — the discipline the
//! whole certificate layer runs on.
//!
//! ## The scalar-CFG limitation, stated up front
//!
//! The CFG that [`crate::fud`] builds models **scalar** variables: its uses and
//! defs carry a scalar index into `CFG::vars`, and an array access lowers to a
//! statement with no uses and no defs
//! (`crate::ir::stmt::Stmt::lower`). So no region dataflow can be run over a
//! `CFG` alone — it would have no array operations to see and would return empty
//! sets that *look* like a proved kill analysis.
//!
//! [`dataflow::RegionProgram`] is therefore the flow graph **with** the accesses
//! attached, and [`dataflow::RegionProgram::from_loop`] builds it from a `Loop`
//! in the Phase 9a five-block shape. This is the deviation from the spec's
//! `dataflow(cfg: &CFG)` signature, and [`dataflow::loop_summary`] is the
//! whole-array triple that does not need a graph at all.

pub mod bounds;
pub mod dataflow;
pub mod kill;
pub mod triplet;

pub use bounds::{
    eval_location, is_read, locations_of_access, region_of_access, region_of_accesses, region_of_kind,
    region_of_range, Region, RegionSet, MAX_DIMS,
};
pub use dataflow::{dataflow, loop_summary, Dataflow, RegionBlock, RegionProgram, Summary};
pub use kill::{
    access_matrix, fullness, is_invariant_write, is_self_conflict_free, killed_region, kills, rank,
    Fullness,
};
pub use triplet::{access_stride, lcm, triplet_of_access, Triplet, TripletRegion};
