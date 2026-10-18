//! Phase 7 — Points-to / aliasing (Wolfe Chapter 8.2).
//!
//! The array IR answers "which element does `A[e]` touch?". Phase 7 answers the
//! question that precedes it for a *pointer*: **which array and element does
//! `A[p]` touch, when `p` is a variable?**
//!
//! The phase is nine modules following the spec's structure:
//!
//! - [`relation`] — §7.2 the `(p, q, d)` triple and [`PointsToGraph`].
//! - [`transfer`] — §7.3 the four assignment cases of Wolfe §8.2.1.
//! - [`fixpoint`] — §7.4 the CFG fixpoint that iterates them.
//! - [`dopvec`] — §7.5 Fortran 90 dope vectors and array-pointer association.
//! - [`ptrarith`] — §7.6 reconstructing `*(p + i·k + c)` to `A[i·k + c]`.
//! - [`allocdep`] — §7.7 allocation dependence (F90 vs C address escape).
//! - [`alias`] — §7.8 the Must/May/No alias query, plus the brute-force oracle.
//! - [`certificate`] — §7.8 the claim vocabulary and the checker replay.
//!
//! ## The public API
//!
//! [`analyze`] runs the fixpoint over a [`PtrProgram`]; [`query`] reads the
//! resulting [`PointsToGraph`]; [`AliasClaim`]/[`AliasBasis`] name a fact about
//! it for a certificate, and [`check_alias_claim`] re-derives that fact from the
//! carried program so `m0-check` can verify it.
//!
//! ## What is derivable and what is trusted
//!
//! Nothing the analysis concluded crosses the certificate boundary: a
//! certificate carries the [`PtrProgram`] (ground truth) and a basis, and the
//! checker re-runs [`analyze`] and re-derives the answer. The design choice that
//! makes `must`/`no` sound is the lattice [`relation::PointsToGraph::meet_with`]
//! at CFG merges and the `UNKNOWN_TARGET` every unresolved pointer starts at —
//! so a relation is only ever reported `Definite` when *every* path agrees, and
//! an unresolved pointer may alias anything.
//!
//! ## The one input the IR does not yet produce
//!
//! The array IR has no pointer variable, so a [`PtrProgram`] is produced by a
//! frontend that lowers pointers into it (Phase 9) rather than derived from a
//! `Loop`. This mirrors Phase 6's `RegionProgram`: the analysis runs over the
//! IR that actually models the thing being analysed.

pub mod alias;
pub mod allocdep;
pub mod certificate;
pub mod dopvec;
pub mod fixpoint;
pub mod ptrarith;
pub mod relation;
pub mod transfer;

pub use alias::{arrays_disjoint, enumerate_alias, query, query_refs, Alias};
pub use allocdep::{
    allocation_dependence, escapes, escaping_vars, AllocDependence, AllocStmt, Language,
};
pub use certificate::{
    alias_certificate, check_alias_claim, must_alias_certificate, points_to, AliasBasis, AliasClaim,
};
pub use dopvec::{associate, Association, Dim, DopeVector};
pub use fixpoint::{analyze, Analysis, PtrBlock, PtrDecl, PtrProgram, MAX_ITERS};
pub use ptrarith::{
    increment_matches, reconstruct, ArrayInfo, ArrayTable, PtrStep, ReconstructedRef,
};
pub use relation::{Definiteness, PointsToGraph, PtrRelation, VarId, UNKNOWN_TARGET};
pub use transfer::{apply_effect, effect_of_pointer_copy, Effect};