//! Phase 9a §4.2 — production emitters for the two Phase 4 claim kinds.
//!
//! Phase 4 introduced [`Claim::TripCount`] and [`Claim::ScalarDependence`]
//! together with the checker arms that verify them, but nothing in `m0-decide`
//! ever *constructed* one: the only certificates carrying those witnesses were
//! hand-built by the Phase 4 tests. That left the claims asserting facts no
//! producer could evidence.
//!
//! Phase 9a closed the gap by giving the analyses a CFG worth analysing —
//! [`crate::fud::build_cfg_from_loop`] now emits the five-block loop with the
//! back edge `latch(3) → header(1)`, so [`crate::fud::induct`] finds a real
//! induction variable and [`crate::fud::scalar_dd`] finds loop-carried
//! situations. This module turns those results into certificates.
//!
//! # What is and is not trusted across the boundary
//!
//! An emitted certificate carries the CFG itself, not the analysis's
//! conclusions. `m0-check` rebuilds the FUD chains from that CFG, re-runs
//! `induct` / `scalar_dd` itself, and requires its own result to agree with the
//! claim. A producer that mis-ran an analysis therefore produces a certificate
//! the checker rejects; the emitters below still run the same analyses up front,
//! so a well-formed loop only ever produces a certificate that checks.

use super::{Certificate, Claim, RuntimeCheckLeg, Witness};
use crate::fud::{
    build_cfg_from_loop, build_fud_chains, induct, scalar_dd, trip_count, InductionVariable,
};
use crate::fud::ScalarDistance as Sd;
use crate::ir::Loop;

/// The evidence recovered from one loop's CFG, kept together so a caller cannot
/// pair a claim with a CFG it was not computed from.
pub struct LoopEvidence {
    /// The φ-annotated CFG the analyses read — the CFG a certificate carries.
    pub cfg: crate::ir::cfg::CFG,
    /// The induction variables `induct` proved on that CFG.
    pub ivs: Vec<InductionVariable>,
}

impl LoopEvidence {
    /// Build the CFG for `loop_` and run the FUD analyses over it.
    ///
    /// Returns `None` when the loop has no induction variables: without an IV
    /// there is no guard and no latch, so there is no loop to count and nothing
    /// for `induct` to classify.
    pub fn from_loop(loop_: &Loop) -> Option<LoopEvidence> {
        let cfg = build_cfg_from_loop(loop_)?;
        let (annotated, chains) = build_fud_chains(&cfg);
        let ind = induct(&annotated, &chains);
        Some(LoopEvidence { cfg: annotated, ivs: ind.ivs })
    }

    /// The IV driving the loop header `header`, as classified by the same
    /// `induct` run that produced [`LoopEvidence::ivs`].
    pub fn iv_at(&self, header: usize) -> Option<&InductionVariable> {
        self.ivs.iter().find(|iv| iv.loop_ == header)
    }
}

/// A trip-count certificate for the loop of `loop_` whose header is `header`.
///
/// The trip count is computed from the IV's *derived* closed form: `a` and `b`
/// come from `induct` over the carried CFG, exactly as `m0-check` will recompute
/// them. `bound` is the guard's limit — the one input the CFG does not encode,
/// because the builder emits the guard as a predicate on the IV alone.
///
/// Returns `None` when the loop is not countable (a non-terminating or
/// overflowing trip count): [`Claim::TripCount`] speaks only for loops whose
/// count and exit value are concrete, and an uncountable claim would be a
/// checker rejection rather than a certificate worth emitting.
pub fn trip_count_certificate(
    loop_: &Loop,
    header: usize,
    bound: i64,
    bound_step: i64,
    pair: (usize, usize),
    level: &str,
    companion_step: Option<i64>,
) -> Option<Certificate> {
    let ev = LoopEvidence::from_loop(loop_)?;
    let iv = ev.iv_at(header)?;
    let tc = trip_count(iv.a, iv.b, bound, bound_step);
    let (count, exit_value) = match (tc.count, tc.exit_value) {
        (Some(c), Some(e)) => (c, e),
        _ => return None,
    };
    // Copy the IV's identity out before moving the CFG into the witness.
    let (iv_var, iv_loop) = (iv.var, iv.loop_);
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::TripCount { count, exit_value },
        witness: Witness::TripCountEvidence {
            cfg: Box::new(ev.cfg),
            iv_var,
            loop_: iv_loop,
            bound,
            bound_step,
            companion_step,
        },
        checker: "m0-check".to_string(),
    })
}

/// The header block of the loop a `Loop`'s *first* induction variable drives.
///
/// Emitters need a header to name in the witness, and the convention throughout
/// the frontend is that IV `k` is registered at scalar slot `k`; the first IV
/// is the one a depth-1 loop's guard tests. Returns `None` if `induct` proves no
/// IV for that loop at all — a loop whose IV is not carried is not a loop the
/// trip-count claim can speak for.
pub fn primary_iv_header(loop_: &Loop) -> Option<usize> {
    let ev = LoopEvidence::from_loop(loop_)?;
    ev.ivs.iter().find(|iv| iv.var == 0).map(|iv| iv.loop_)
}

/// A trip-count certificate for a loop whose guards are the IR's own `ivs`.
///
/// [`trip_count_certificate`] needs a header and a bound, and both are already
/// in the loop for the ordinary case: the CFG builder tests IV `k` against
/// `ivs[k].upper` at header block `1 + k` (entry is block 0), so this is the
/// shape a M5-expanded or M8-recurrence loop presents. Only the loop's *first*
/// IV is claimed: a nested CFG's inner header is reached through the outer
/// latch, and proving the outer count is the outer claim.
///
/// Returns `None` when the loop has no IVs, when `induct` does not classify the
/// first IV as linear, or when the resulting count is not concrete.
pub fn loop_trip_count_certificate(
    loop_: &Loop,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    let bound = loop_.ivs.first()?.upper;
    let header = primary_iv_header(loop_)?;
    trip_count_certificate(loop_, header, bound, 0, pair, level, None)
}

/// A scalar-dependence certificate for the strongest loop-carried dependence
/// [`crate::fud::scalar_dd`] finds on `loop_`'s CFG.
///
/// Selection is deliberately mechanical: of the `(def, use)` situations the
/// analysis reports, take the first one the loop makes *carried* ([`Sd::One`],
/// [`Sd::AnyBefore`] or [`Sd::AnyAtOrBefore`]). An `Sd::Zero` situation is
/// intra-iteration and says nothing a plain data-flow fact would not, so those
/// are skipped; a loop with no carried dependence yields `None` rather than a
/// padded claim.
///
/// The checker re-derives the whole `situations` table from the carried CFG, so
/// the `def`/`use` ids here are positions the checker confirms, not positions it
/// takes on faith.
pub fn scalar_dependence_certificate(
    loop_: &Loop,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    let cfg = build_cfg_from_loop(loop_)?;
    let (annotated, chains) = build_fud_chains(&cfg);
    if annotated.blocks.len() <= 1 {
        return None;
    }
    let dd = scalar_dd(&annotated, &chains);
    // A carried situation requires a real header; no headers means the CFG is a
    // straight-line DAG and there is nothing loop-carried to claim.
    if dd.headers.is_empty() {
        return None;
    }
    let carried = |s: &Sd| matches!(s, Sd::One | Sd::AnyBefore | Sd::AnyAtOrBefore);
    let (def, use_, distance) = dd
        .situations
        .iter()
        .find(|(_, s)| carried(*s))
        .map(|((d, u), s)| (*d, *u, *s))?;
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::ScalarDependence { def, use_, distance },
        witness: Witness::ScalarDependenceEvidence {
            cfg: Box::new(annotated),
            asserted_def: def,
            asserted_use: use_,
        },
        checker: "m0-check".to_string(),
    })
}

/// The `pair` field for an evidence certificate: the loop's first two accesses
/// when it has them, else `(0, 0)`.
///
/// A trip-count or scalar-dependence claim is about the *loop* rather than about
/// an access pair, but `Certificate::pair` is part of the serialised format and
/// the checker's other arms index `loop_.body` with it, so evidence
/// certificates carry a pair that is at least in range for the loop that
/// produced them.
pub fn evidence_pair(loop_: &Loop) -> (usize, usize) {
    if loop_.body.len() >= 2 {
        (0, 1)
    } else {
        (0, 0)
    }
}

// ============================================================================
// Phase 5 §5.9 (Ch. 7.6) — run-time dependence testing
// ============================================================================

/// The index coefficient shared by both accesses of a candidate anti-dependence,
/// provided it is the only thing that can make them collide.
///
/// The run-time test exists for exactly one shape: a depth-1, step-1, affine
/// loop where a write and a read of the same base use indices that differ by a
/// constant multiple of the IV. `A[i] = A[i-1]` is the canonical member — the
/// loop is not independent (`A[i]` reads what the previous iteration wrote), so
/// a plain "assume independent" version would be wrong, and only a run-time test
/// can license the vectorized version.
///
/// Returns `(coeff, k)` where `coeff` is the (equal) index coefficient of both
/// accesses and `k = |coeff|` is the distance in *lattice* elements. `None` when
/// the pair is outside this shape — a different base, a non-affine index, a
/// guard, an indirection, a step that is not 1 — in which case no run-time test
/// is emitted rather than a guessed one.
fn runtime_check_shape(loop_: &Loop, p: usize, q: usize) -> Option<(i64, i64)> {
    if loop_.depth != 1 || loop_.ivs.len() != 1 || loop_.ivs[0].step != 1 {
        return None;
    }
    let ap = loop_.body.get(p)?;
    let aq = loop_.body.get(q)?;
    if ap.base != aq.base
        || ap.index_kind != crate::ir::IndexKind::Affine
        || aq.index_kind != crate::ir::IndexKind::Affine
    {
        return None;
    }
    // A guard, range, indirection or surface expression means the true index is
    // not the affine `index · iv + offset`, so the shape is not one this test
    // speaks for.
    for a in [ap, aq] {
        if a.guard.is_some() || a.range.is_some() || a.indirect.is_some() || a.expr.is_some() {
            return None;
        }
    }
    let (cp, cq) = (ap.index.first().copied()?, aq.index.first().copied()?);
    if cp == 0 || cp != cq {
        return None;
    }
    Some((cp, cp.unsigned_abs() as i64))
}

/// The *canonical* run-time check for an access pair, or `None` when the pair is
/// outside the §5.9 shape.
///
/// The predicate is `NonStraddling || ByteEqual` — the two conjunctive legs the
/// checker also reconstructs. This function exists so a caller can ask "does
/// this pair have a run-time test, and what is it?" without building a
/// certificate; [`runtime_check_certificate`] is the certificate form, and both
/// compute the legs the same way so they cannot drift.
pub fn runtime_check_legs(loop_: &Loop, p: usize, q: usize) -> Option<Vec<RuntimeCheckLeg>> {
    runtime_check_shape(loop_, p, q)?;
    Some(vec![
        RuntimeCheckLeg::NonStraddling,
        RuntimeCheckLeg::ByteEqual,
    ])
}

/// A [`Claim::RuntimeCheck`] certificate for the loop's access pair, or `None`
/// when the pair admits no run-time test (see [`runtime_check_shape`]).
///
/// This is the §5.9 output type the reference calls for: it does **not** claim
/// the accesses are independent — it claims that the emitter identified this
/// pair as a run-time versioning candidate and that `legs` is the analysis's
/// answer. `m0-check` re-derives `legs` from the loop's exact index coefficients
/// and element sizes, so a certificate asserting the wrong check is rejected,
/// and there is no arm that reads the claim as independence.
pub fn runtime_check_certificate(
    loop_: &Loop,
    p: usize,
    q: usize,
    level: &str,
) -> Option<Certificate> {
    let legs = runtime_check_legs(loop_, p, q)?;
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair: (p, q),
        claim: Claim::RuntimeCheck { legs },
        witness: Witness::RuntimeCheckEvidence { pair: (p, q) },
        checker: "m0-check".to_string(),
    })
}

/// Phase 6 §6.2 — an array-region certificate for `loop_`'s access `access`.
///
/// The region is derived by [`crate::region::region_of_access`] over the loop's
/// own IV domain. Returns `None` when the access has no derivable region (a
/// non-affine index the analysis refuses): a claim the checker would reject is
/// not worth emitting.
///
/// The certificate carries the loop itself in
/// [`Witness::ArrayRegionEvidence`]; `m0-check` re-derives the region from that
/// loop and requires the derived box to be contained in the claimed one.
pub fn array_region_certificate(
    loop_: &Loop,
    access: usize,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    if access >= loop_.body.len() {
        return None;
    }
    let region = crate::region::region_of_access(&loop_.body[access], loop_)?;
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::ArrayRegion { access, region },
        witness: Witness::ArrayRegionEvidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".to_string(),
    })
}

/// Phase 6 §6.2 — the whole-array `MayUse` / `MayMod` / `Kill` summary of
/// `loop_`.
///
/// Built by [`crate::region::loop_summary`], which reads the loop's accesses
/// directly (the scalar CFG does not model array accesses — see
/// [`crate::region`]'s module docs). The checker rebuilds the same summary from
/// the carried loop and requires exact agreement.
pub fn function_summary_certificate(
    loop_: &Loop,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    if loop_.body.is_empty() {
        return None;
    }
    let s = crate::region::loop_summary(loop_);
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::FunctionSummary {
            func: format!("loop:{}", loop_.hash()),
            may_use: s.may_use,
            may_mod: s.may_mod,
            kill: s.kill,
        },
        witness: Witness::ArrayRegionEvidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".to_string(),
    })
}

/// Phase 5 §5.1.8 — a reduction certificate for the first reduction
/// [`crate::ir::reduction::detect_reductions`] finds on `loop_`.
///
/// Returns `None` when the loop has no scalar statement body (the analyses need
/// a CFG with real definitions) or when no reduction is detected: a loop with
/// nothing to claim produces no claim.
pub fn reduction_certificate(
    loop_: &Loop,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    let cfg = crate::fud::build_cfg_from_loop(loop_)?;
    let (_annotated, chains) = crate::fud::build_fud_chains(&cfg);
    let r = crate::ir::reduction::detect_reductions(&cfg, &chains)
        .into_iter()
        .next()?;
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::Reduction { var: r.var, loop_: r.loop_, kind: r.kind },
        witness: Witness::Phase5Evidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".to_string(),
    })
}

/// Phase 5 §5.1.11 — a loop-kind certificate for `loop_`.
///
/// The dependence set is rebuilt with [`crate::decide::loop_dependences`] and
/// classified by [`crate::ir::loopkind::classify_loop_kind`]. The checker does
/// exactly the same rebuild, so the claim is accepted iff the classification is
/// reproducible.
pub fn loop_kind_certificate(
    loop_: &Loop,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    let deps = crate::decide::loop_dependences(loop_);
    let kind = crate::ir::loopkind::classify_loop_kind(loop_, &deps);
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::LoopKind(kind),
        witness: Witness::Phase5Evidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".to_string(),
    })
}

/// Phase 5 §5.1.9 — an alignment certificate for `loop_`.
///
/// Returns `None` when the loop's dependences admit no alignment (a cycle with
/// nonzero total distance); asserting an alignment the loop does not admit would
/// be a checker rejection, not a certificate worth writing.
pub fn alignment_certificate(
    loop_: &Loop,
    pair: (usize, usize),
    level: &str,
) -> Option<Certificate> {
    let deps = crate::decide::loop_dependences(loop_);
    let align = crate::fud::compute_alignment(loop_, &deps)?;
    Some(Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair,
        claim: Claim::Aligned(align),
        witness: Witness::Phase5Evidence { loop_: Box::new(loop_.clone()) },
        checker: "m0-check".to_string(),
    })
}
