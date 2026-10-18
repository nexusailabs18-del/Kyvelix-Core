//! Phase 3 — Factored Use-Def chains (Wolfe Chapter 6.1–6.3).
//!
//! This module builds the graph-theoretic structure that later phases consume:
//!
//! - [`dominators`] — Cooper-Harvey-Kennedy dominator tree and dominance
//!   frontiers (Wolfe §6.1.3).
//! - [`placement`] — φ-term placement via the iterated dominance frontier
//!   `DF⁺(D(M))` (Wolfe §6.1.4; justified by the theorem `DF⁺(S) = J⁺(S)`).
//! - [`chain`] — Algorithm Chain: each scalar use is linked to the unique
//!   definition (or φ-term) whose value it sees (Wolfe §6.1.5). The reverse
//!   pass that restores `CurrDef` is **mandatory**.
//! - [`reaching`] — Algorithm FollowChain: the *set* of definitions reaching
//!   a use (Wolfe §6.3).
//! - [`frud`] — FRUD chains: the mirror direction, definition → reached uses,
//!   with Υ-terms at reverse merge points (Wolfe §6.7.5).
//!
//! The discipline mirrors the rest of UDDP: the certificate carries the CFG,
//! and the checker **rebuilds** the chain from it instead of trusting the
//! analyzer's chain.

pub mod cfg;
pub mod chain;
pub mod constprop;
pub mod dominators;
pub mod frud;
pub mod induct;
pub mod placement;
pub mod reaching;
pub mod scalardd;
pub mod tripcount;

pub mod alignment;
pub mod crossloop;

#[cfg(test)]
pub mod testgen;

pub use cfg::{build_cfg_from_loop, build_cfg_from_stmts, cfg_matches_loop};
pub use chain::{build_chain_with_phi, construct_chains, ChainLink, FudChain};
pub use constprop::{constant_prop, AbsVal, ConstPropResult};
pub use alignment::{compute_alignment, is_alignable, Alignment};
pub use crossloop::{compute_cross_loop_distance, cross_loop_vectors, CrossLoopDistance};
pub use dominators::{
    compute_dominance_frontiers, compute_dominators, df_plus_of_set, DominanceFrontiers,
    DominatorTree,
};
pub use frud::{construct_frud_chains, FrudChain};
pub use induct::{induct, Class, InductResult, InductionVariable, OpId};
pub use placement::{
    apply_placement, definitions_by_var, phi_blocks_for, place_phi_terms, PhiPlacement,
};
pub use reaching::{find_reaching_defs, ReachingDefs};
pub use scalardd::{
    enclosing_loops, find_back_edges, find_loops, project_dd_facts, scalar_dd, block_preorder,
    DdFact, DependenceKind, Edge, Loc, LoopInfo, LoopSource, ScalarDD, ScalarDependence,
    ScalarDistance,
};
pub use tripcount::{trip_count, TripCount};

use crate::ir::CFG;

/// The bundle of graph structures Phase 3 builds and Phase 4 consumes:
/// the forward FUD chain, the reaching-definition sets, and the FRUD chains.
///
/// The `cfg` these were built from is always the φ-annotated CFG returned by
/// [`build_chain_with_phi`]; pass it to Phase 4 alongside this bundle.
#[derive(Debug, Clone, Default)]
pub struct FudChains {
    pub chain: FudChain,
    pub reaching: ReachingDefs,
    pub frud: FrudChain,
}

impl FudChains {
    /// Build the reaching-definition sets and FRUD chains for an already
    /// φ-annotated `cfg` and its `chain`.
    pub fn from_chain(cfg: &CFG, chain: FudChain) -> FudChains {
        let reaching = find_reaching_defs(cfg, &chain);
        let frud = construct_frud_chains(cfg, &chain);
        FudChains { chain, reaching, frud }
    }
}

/// Convenience: run φ-placement + Algorithm Chain on `cfg`, then build the
/// full [`FudChains`] bundle. Returns the φ-annotated CFG (the ground truth a
/// Phase 4 certificate carries) together with the bundle.
pub fn build_fud_chains(cfg: &CFG) -> (CFG, FudChains) {
    let (annotated, chain) = build_chain_with_phi(cfg);
    let chains = FudChains::from_chain(&annotated, chain);
    (annotated, chains)
}
