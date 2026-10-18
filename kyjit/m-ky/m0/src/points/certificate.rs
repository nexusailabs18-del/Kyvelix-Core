//! Phase 7 §7.8 — the alias claim, its basis, and the checker replay.
//!
//! This is the bridge between the analysis in the sibling modules and the
//! certificate vocabulary in [`crate::certificate`]. It defines the two types a
//! Phase 7 certificate names — [`AliasClaim`] (what is asserted) and
//! [`AliasBasis`] (which part of the carried program the assertion is about) —
//! and the single function that re-derives the assertion from the program.
//!
//! # The trust boundary
//!
//! Nothing the analyzer concluded about the graph crosses the certificate
//! boundary. The witness carries the [`PtrProgram`] (ground truth) and the
//! basis; [`check_alias_claim`] re-runs [`crate::points::analyze`] and
//! re-derives the answer. A producer that mis-ran the fixpoint, or forged a
//! relation, produces a certificate whose re-derivation disagrees with the
//! claim — which is exactly when the checker rejects it.
//!
//! The spec's `Witness::AliasWitness { relation, basis }` carries the relation
//! itself; that would be trusting the analyzer's output, which this crate never
//! does for any claim kind. The program is carried instead, and the relation is
//! recomputed. This is the one deliberate deviation from the spec's witness and
//! it is a strengthening.

use serde::{Deserialize, Serialize};

use crate::points::alias::{arrays_disjoint, query, Alias};
use crate::points::allocdep::{
    allocation_dependence, escaping_vars, AllocDependence, AllocStmt, Language,
};
use crate::points::fixpoint::{analyze, PtrProgram};
use crate::points::ptrarith::{reconstruct, ArrayTable, PtrStep};
use crate::points::relation::{PointsToGraph, VarId};

/// What a Phase 7 certificate asserts (Wolfe §8.2.2–§8.2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AliasClaim {
    /// `pointer` definitely points at `target` and nothing else, after `block`.
    MustAlias {
        pointer: VarId,
        target: VarId,
        block: usize,
    },
    /// `p` and `q` may be the same location after `block`.
    MayAlias { p: VarId, q: VarId, block: usize },
    /// `p` and `q` are definitely different locations after `block`.
    NoAlias { p: VarId, q: VarId, block: usize },
    /// The two walked pointers sweep disjoint elements (Wolfe §8.2.3).
    DisjointArrays { first: PtrStep, second: PtrStep },
}

/// Where in the carried program a claim is anchored — the spec's `AliasBasis`,
/// extended so each claim kind names the evidence it needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "basis", rename_all = "snake_case")]
pub enum AliasBasis {
    /// Read the points-to graph after this block.
    PointsTo { block: usize },
    /// Reconstruct the two pointer steps against these declared arrays.
    Reconstructed { arrays: ArrayTable },
    /// The two pointers are the results of these allocations, and their order is
    /// independent under this language's allocation semantics.
    AllocationOrder {
        language: Language,
        first: AllocStmt,
        second: AllocStmt,
    },
}

/// Re-derive `claim` from `program` and `basis`. `Ok(())` iff the program
/// entails the claim.
pub fn check_alias_claim(
    claim: &AliasClaim,
    program: &PtrProgram,
    basis: &AliasBasis,
) -> Result<(), String> {
    program
        .validate()
        .map_err(|e| format!("carried pointer program is malformed: {}", e))?;
    let analysis =
        analyze(program).ok_or_else(|| "points-to fixpoint did not converge".to_string())?;

    // The graph after `block`, whatever the basis kind is.
    let graph_at = |want: usize| -> Result<&PointsToGraph, String> {
        analysis
            .out_of(want)
            .ok_or_else(|| format!("block {} is not in the carried program", want))
    };
    // A `PointsTo` basis must name the claim's own block.
    let require_points_to = |want: usize| -> Result<(), String> {
        match basis {
            AliasBasis::PointsTo { block } if *block == want => Ok(()),
            AliasBasis::PointsTo { block } => Err(format!(
                "basis block {} disagrees with the claim block {}",
                block, want
            )),
            _ => Err(format!(
                "claim is anchored at block {} but the basis is not a PointsTo basis",
                want
            )),
        }
    };

    match claim {
        AliasClaim::MustAlias { pointer, target, block } => {
            require_points_to(*block)?;
            let g = graph_at(*block)?;
            if !g.must_point_to(*pointer, *target) {
                return Err(format!(
                    "rebuilt graph does not make {} point only at {} (targets: {:?})",
                    pointer.display(),
                    target.display(),
                    g.targets_of(*pointer)
                ));
            }
            Ok(())
        }
        AliasClaim::MayAlias { p, q, block } => {
            require_points_to(*block)?;
            let g = graph_at(*block)?;
            if query(g, *p, *q) == Alias::NoAlias {
                return Err(format!(
                    "rebuilt graph proves {} and {} disjoint, so they cannot may-alias",
                    p.display(),
                    q.display()
                ));
            }
            Ok(())
        }
        AliasClaim::NoAlias { p, q, block } => {
            // A `NoAlias` rests on the graph (`PointsTo`) and may *additionally*
            // invoke an allocation-order argument (`AllocationOrder`).
            match basis {
                AliasBasis::PointsTo { block: b } if *b == *block => {}
                AliasBasis::AllocationOrder { .. } => {}
                AliasBasis::PointsTo { block: b } => {
                    return Err(format!(
                        "basis block {} disagrees with the claim block {}",
                        b, block
                    ));
                }
                _ => {
                    return Err(
                        "NoAlias requires a PointsTo or AllocationOrder basis".to_string()
                    );
                }
            }
            let g = graph_at(*block)?;
            if query(g, *p, *q) != Alias::NoAlias {
                return Err(format!(
                    "rebuilt graph does not prove {} and {} disjoint",
                    p.display(),
                    q.display()
                ));
            }
            // An `AllocationOrder` basis must additionally discharge the
            // allocation-order argument for the two named allocations.
            if let AliasBasis::AllocationOrder { language, first, second } = basis {
                let esc = escaping_vars(g);
                if allocation_dependence(*language, first, second, &esc) != AllocDependence::None {
                    return Err(
                        "allocation order is observable, so the two results cannot be assumed disjoint"
                            .to_string(),
                    );
                }
                let ok_order = (first.result == *p && second.result == *q)
                    || (first.result == *q && second.result == *p);
                if !ok_order {
                    return Err(format!(
                        "AllocationOrder basis names {} and {} but the claim is about {} and {}",
                        first.result.display(),
                        second.result.display(),
                        p.display(),
                        q.display()
                    ));
                }
            }
            Ok(())
        }
        AliasClaim::DisjointArrays { first, second } => {
            let arrays = match basis {
                AliasBasis::Reconstructed { arrays } => arrays,
                _ => return Err("DisjointArrays requires a Reconstructed basis".to_string()),
            };
            let r1 = reconstruct(first, arrays).ok_or_else(|| {
                "first pointer did not reconstruct against the declared arrays".to_string()
            })?;
            let r2 = reconstruct(second, arrays).ok_or_else(|| {
                "second pointer did not reconstruct against the declared arrays".to_string()
            })?;
            if !arrays_disjoint(&r1, &r2) {
                return Err(format!(
                    "reconstructed references (array {}, coeff {}, offset {}) and (array {}, coeff {}, offset {}) are not provably disjoint",
                    r1.array.display(), r1.coeff, r1.offset,
                    r2.array.display(), r2.coeff, r2.offset
                ));
            }
            Ok(())
        }
    }
}

/// The `PointsTo` basis for a claim anchored at `block`.
pub fn points_to(block: usize) -> AliasBasis {
    AliasBasis::PointsTo { block }
}

/// Build a Phase 7 [`crate::certificate::Certificate`] for `claim` over
/// `program`, anchored to the loop file `loop_` (which supplies the hash and the
/// access pair the certificate format carries).
///
/// Returns `None` when the checker would reject the claim — a producer never
/// writes a certificate it cannot evidence, so the emitter runs
/// [`check_alias_claim`] itself first.
pub fn alias_certificate(
    loop_: &crate::ir::Loop,
    program: &PtrProgram,
    claim: AliasClaim,
    basis: AliasBasis,
    level: &str,
) -> Option<crate::certificate::Certificate> {
    if check_alias_claim(&claim, program, &basis).is_err() {
        return None;
    }
    Some(crate::certificate::Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair: crate::certificate::emit::evidence_pair(loop_),
        claim: crate::certificate::Claim::Alias(claim),
        witness: crate::certificate::Witness::AliasEvidence {
            program: Box::new(program.clone()),
            basis,
        },
        checker: "m0-check".to_string(),
    })
}

/// A `MustAlias` certificate for `pointer → target` after `block`.
pub fn must_alias_certificate(
    loop_: &crate::ir::Loop,
    program: &PtrProgram,
    pointer: VarId,
    target: VarId,
    block: usize,
    level: &str,
) -> Option<crate::certificate::Certificate> {
    alias_certificate(
        loop_,
        program,
        AliasClaim::MustAlias { pointer, target, block },
        points_to(block),
        level,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::points::fixpoint::{PtrBlock, PtrDecl};
    use crate::points::ptrarith::ArrayTable;
    use crate::points::transfer::Effect;

    /// One block that sets `p` (id 1) to `&tx` (id 0).
    fn assign_program() -> PtrProgram {
        PtrProgram::new(
            vec![PtrDecl::target("tx"), PtrDecl::pointer("p")],
            0,
            vec![PtrBlock::new(
                0,
                vec![],
                vec![Effect::AddressOf { lhs: VarId(1), target: VarId(0) }],
            )],
        )
    }

    /// `tx`/`ty` targets (ids 0/1) and `p`/`q` pointers (ids 2/3), one block
    /// setting `p = &tx` and `q = &ty`.
    fn two_pointer_program() -> PtrProgram {
        PtrProgram::new(
            vec![
                PtrDecl::target("tx"),
                PtrDecl::target("ty"),
                PtrDecl::pointer("p"),
                PtrDecl::pointer("q"),
            ],
            0,
            vec![PtrBlock::new(
                0,
                vec![],
                vec![
                    Effect::AddressOf { lhs: VarId(2), target: VarId(0) },
                    Effect::AddressOf { lhs: VarId(3), target: VarId(1) },
                ],
            )],
        )
    }

    #[test]
    fn must_alias_claim_is_accepted_when_the_program_entails_it() {
        let p = assign_program();
        let c = AliasClaim::MustAlias { pointer: VarId(1), target: VarId(0), block: 0 };
        assert!(check_alias_claim(&c, &p, &points_to(0)).is_ok());
    }

    #[test]
    fn must_alias_claim_is_rejected_without_the_program() {
        let p = assign_program();
        // Swap the declared roles so block 0 does not establish anything about v2.
        let c = AliasClaim::MustAlias { pointer: VarId(2), target: VarId(0), block: 0 };
        assert!(check_alias_claim(&c, &p, &points_to(0)).is_err());
    }

    #[test]
    fn basis_block_must_agree_with_claim_block() {
        let p = assign_program();
        let c = AliasClaim::MustAlias { pointer: VarId(1), target: VarId(0), block: 0 };
        assert!(check_alias_claim(&c, &p, &points_to(1)).is_err());
    }

    #[test]
    fn no_alias_accepts_an_allocation_basis_when_the_order_is_free() {
        let p = two_pointer_program();
        let basis = AliasBasis::AllocationOrder {
            language: Language::Fortran90,
            first: AllocStmt::new(VarId(2), 0),
            second: AllocStmt::new(VarId(3), 1),
        };
        let c = AliasClaim::NoAlias { p: VarId(2), q: VarId(3), block: 0 };
        assert!(check_alias_claim(&c, &p, &basis).is_ok());
    }

    #[test]
    fn allocation_basis_must_name_the_claim_pointers() {
        let p = two_pointer_program();
        let c = AliasClaim::NoAlias { p: VarId(2), q: VarId(3), block: 0 };
        // The reversed pairing still names the same two allocations.
        let reversed = AliasBasis::AllocationOrder {
            language: Language::Fortran90,
            first: AllocStmt::new(VarId(3), 0),
            second: AllocStmt::new(VarId(2), 1),
        };
        assert!(check_alias_claim(&c, &p, &reversed).is_ok());
        // A basis naming unrelated variables is rejected.
        let unrelated = AliasBasis::AllocationOrder {
            language: Language::Fortran90,
            first: AllocStmt::new(VarId(0), 0),
            second: AllocStmt::new(VarId(1), 1),
        };
        assert!(check_alias_claim(&c, &p, &unrelated).is_err());
    }

    #[test]
    fn must_alias_claims_need_a_points_to_basis() {
        let p = two_pointer_program();
        let c = AliasClaim::MustAlias { pointer: VarId(2), target: VarId(0), block: 0 };
        assert!(check_alias_claim(&c, &p, &points_to(0)).is_ok());
        let basis = AliasBasis::AllocationOrder {
            language: Language::Fortran90,
            first: AllocStmt::new(VarId(2), 0),
            second: AllocStmt::new(VarId(3), 1),
        };
        assert!(check_alias_claim(&c, &p, &basis).is_err());
    }

    #[test]
    fn disjoint_arrays_claim_needs_a_reconstructed_basis() {
        let p = assign_program();
        let arrays = ArrayTable::of(&[(VarId(10), 8), (VarId(11), 8)]);
        let claim = AliasClaim::DisjointArrays {
            first: PtrStep::new(VarId(1), VarId(10), 1, 0, 8, 0),
            second: PtrStep::new(VarId(2), VarId(11), 1, 0, 8, 0),
        };
        assert!(check_alias_claim(&claim, &p, &AliasBasis::Reconstructed { arrays }).is_ok());
        // A wrong basis is rejected.
        assert!(check_alias_claim(&claim, &p, &points_to(0)).is_err());
    }

    #[test]
    fn disjoint_arrays_claim_is_rejected_for_overlapping_walks() {
        let p = assign_program();
        let arrays = ArrayTable::of(&[(VarId(10), 8)]);
        let claim = AliasClaim::DisjointArrays {
            first: PtrStep::new(VarId(1), VarId(10), 2, 0, 8, 0),
            second: PtrStep::new(VarId(2), VarId(10), 1, 0, 8, 0),
        };
        assert!(check_alias_claim(&claim, &p, &AliasBasis::Reconstructed { arrays }).is_err());
    }
}