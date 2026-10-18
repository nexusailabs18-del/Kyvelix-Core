//! Dispatcher: tries each level in order.
//! First definitive answer (EXACT or NONE) wins.
//! If all levels return UNKNOWN, the loop is UNKNOWN.

mod m0;
pub mod m2a;
pub mod m2a5;
pub mod m2b;
pub mod m3;
pub mod m5;
pub mod m6;
pub mod m7;
pub mod m8;
pub mod m10;
pub mod m11;
pub mod m12;
pub mod m13;
pub mod m2g;
pub mod m1;
pub mod m4;

use crate::certificate::Certificate;
use crate::fud::{build_cfg_from_loop, build_fud_chains, constant_prop};
use crate::ir::*;

#[derive(Debug, Clone)]
pub enum Decision {
    Exact(Vec<i64>),
    None,
    Unknown(String),
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    // Fast rejects shared by all levels
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    // Stage 1: same base
    if ap.base != aq.base {
        return (Decision::None, None);
    }

    // Stage 2: both reads
    if ap.kind == AccessKind::Read && aq.kind == AccessKind::Read {
        return (Decision::None, None);
    }

    // If the loop carries chase metadata, M5 expands it first.
    if loop_.chase.is_some() {
        return m5::decide(loop_, p, q);
    }

    // If the loop carries recursion metadata, M3 transforms it into a
    // real loop first (its `ivs` are placeholders until then).
    if loop_.recursion.is_some() {
        return m3::decide(loop_, p, q);
    }

    // If any access carries a surface `expr`, the loop is not in M0's
    // input language. Route to M2g, which normalizes then calls M0.
    if loop_.body.iter().any(|a| a.expr.is_some()) {
        return m2g::decide(loop_, p, q);
    }

    // M2a.5: table-literal injectivity fires before M2a when either
    // access has a table-form indirect.
    if loop_.body.iter().any(|a| {
        a.indirect.as_ref().and_then(|i| i.table.as_ref()).is_some()
    }) {
        return m2a5::decide(loop_, p, q);
    }

    // If any access carries an `indirect`, `index`/`offset` are stale
    // placeholders under Convention A. Route to M2a, which flattens
    // and then calls M0 on the flattened loop.
    if loop_.body.iter().any(|a| a.indirect.is_some()) {
        return m2a::decide(loop_, p, q);
    }

    // If the loop has scalar statements, run constant propagation.
    // The results can be used by M4 for guard folding (e.g., discovering
    // that a guard bound involving a scalar is actually constant).
    let const_prop = if loop_.stmts.is_some() {
        build_cfg_from_loop(loop_)
            .map(|cfg| {
                let (annotated, chains) = build_fud_chains(&cfg);
                constant_prop(&annotated, &chains)
            })
    } else {
        None
    };

    // If either access carries a guard, M4 narrows the effective IV
    // domain before M0 sees the loop.
    if loop_.body[p].guard.is_some() || loop_.body[q].guard.is_some() {
        return m4::decide_with_const_prop(loop_, p, q, const_prop);
    }

    // If either access carries a bounded-range expression, M2b decides
    // on location intervals directly.
    if loop_.body[p].range.is_some() || loop_.body[q].range.is_some() {
        return m2b::decide(loop_, p, q);
    }

    // M10: fixed-point index expressions
    {
        use crate::ir::IndexKind;
        let uses_fp = |a: &crate::ir::Access|
            matches!(&a.index_kind, IndexKind::FixedPoint(_));
        if uses_fp(&loop_.body[p]) || uses_fp(&loop_.body[q]) {
            return m10::decide(loop_, p, q);
        }
    }

    // M8: recurrence index expressions
    {
        use crate::ir::IndexKind;
        let uses_rec = |a: &crate::ir::Access|
            matches!(&a.index_kind, IndexKind::Recurrence(_));
        if uses_rec(&loop_.body[p]) || uses_rec(&loop_.body[q]) {
            return m8::decide(loop_, p, q);
        }
    }

    // M7: hash index expressions
    {
        use crate::ir::IndexKind;
        let uses_hash = |a: &crate::ir::Access| matches!(&a.index_kind, IndexKind::Hash(_));
        if uses_hash(&loop_.body[p]) || uses_hash(&loop_.body[q]) {
            return m7::decide(loop_, p, q);
        }
    }

    // M13: assertions on the loop's IV domain.
    if !loop_.assertions.is_empty() {
        return m13::decide(loop_, p, q);
    }

    // M6: product index expressions
    {
        use crate::ir::IndexKind;
        let uses_product = |a: &crate::ir::Access| matches!(&a.index_kind, IndexKind::Product(_));
        if uses_product(&loop_.body[p]) || uses_product(&loop_.body[q]) {
            return m6::decide(loop_, p, q);
        }
    }

        // M11 — symbolic pointer chase
    if loop_.body.iter().any(|a| matches!(&a.index_kind, crate::ir::IndexKind::SymbolicFixedPoint { .. })) {
        return m11::decide(loop_, p, q);
    }

// Try M0
    let (d, c) = m0::decide(loop_, p, q);
    if !matches!(d, Decision::Unknown(_)) {
        return (d, c);
    }

    // Try M1 (parametric affine)
    // M1 works on LoopParam. For now, the dispatcher only sees Loop.
    // M1 will be reached via the LoopParam entry point (see decide_param).
    // When a caller has a LoopParam, it calls m1::decide_param directly.

    // M2b, M3, M4, M5 go here as they are added.

    (Decision::Unknown("all levels exhausted".into()), None)
}

pub fn gcd_i64(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// M9 entry point: dispatch a loop with a runtime-modulus environment.
/// Routes to `m2b::decide_param` if any access uses `ModParam`.
/// Otherwise falls back to plain `decide`.
pub fn decide_with_env(
    loop_: &Loop,
    p: usize,
    q: usize,
    env: &[(String, i64)],
) -> (Decision, Option<Certificate>) {
    let has_modparam = loop_.body.iter().any(|a| {
        matches!(&a.range, Some(RangeExpr::ModParam { .. }))
    });
    if !has_modparam {
        return decide(loop_, p, q);
    }
    m2b::decide_param(loop_, p, q, env)
}

/// M1 entry point: decide a parametric loop with an environment.
/// Delegates to m1::decide_param.
/// M12 entry point. Takes a `DynamicLoop` and an environment; resolves
/// the bounds; delegates to `decide` on the resolved `Loop`. The
/// certificate is wrapped in `Witness::WithEnv`.
pub fn decide_dynamic(
    dyn_loop: &DynamicLoop,
    env: &[(String, i64)],
    p: usize,
    q: usize,
) -> (Decision, Option<Certificate>) {
    m12::decide_with_env(dyn_loop, env, p, q)
}

/// Phase 5 §5.1.9–§5.1.11 — the dependence set of a loop, as the input the
/// alignment / loop-kind / cross-loop subsystems consume.
///
/// Every ordered access pair `(p, q)` that shares a base is decided by the
/// dispatcher:
///
/// - `Decision::Exact(distance)` → a [`Dependence`] of the direction implied by
///   the two access kinds (`Write→Read` flow, `Read→Write` anti, `Write→Write`
///   output) with that distance vector;
/// - `Decision::None` → no dependence, so no edge;
/// - `Decision::Unknown` → a conservative edge ([`Dependence::unknown`]) with
///   the same direction. An undecided pair *might* be dependent, so a
///   classifier that dropped it would be unsound; keeping it is what makes
///   `classify_loop_kind` answer `Sequential` rather than over-claiming
///   parallelism.
///
/// Pairs on different bases contribute nothing (they cannot depend), matching
/// the dispatcher's own first-stage reject.
pub fn loop_dependences(loop_: &Loop) -> Vec<crate::ir::dependence::Dependence> {
    use crate::ir::dependence::{DepKind, Dependence};
    let mut out = Vec::new();
    let n = loop_.body.len();
    for p in 0..n {
        for q in 0..n {
            if p == q {
                continue;
            }
            let ap = &loop_.body[p];
            let aq = &loop_.body[q];
            if ap.base != aq.base {
                continue;
            }
            let kind = match (ap.kind, aq.kind) {
                (AccessKind::Write, AccessKind::Read) => DepKind::Flow,
                (AccessKind::Read, AccessKind::Write) => DepKind::Anti,
                (AccessKind::Write, AccessKind::Write) => DepKind::Output,
                // Both reads never depend; `decide` also rejects them.
                (AccessKind::Read, AccessKind::Read) => continue,
            };
            match decide(loop_, p, q).0 {
                Decision::Exact(distance) => {
                    out.push(Dependence::new(p, q, kind, distance))
                }
                Decision::None => {}
                Decision::Unknown(_) => out.push(Dependence::unknown(p, q, kind)),
            }
        }
    }
    out
}

pub fn decide_param(
    loop_: &LoopParam,
    env: &[(String, i64)],
    p: usize,
    q: usize,
) -> (Decision, Option<Certificate>) {
    let env_vec: Vec<(String, i64)> = env.to_vec();
    m1::decide_param(loop_, &env_vec, p, q)
}

/// Phase 9a §4.2 — the loop-level evidence a decided loop also warrants.
///
/// Every level above answers an *access-pair* question: does statement `p`
/// depend on statement `q`. The two Phase 4 claim kinds answer a *loop*
/// question — how many times does this loop run, and which scalar value flows
/// across the back edge — so they are not the decision's witness and cannot
/// replace it. `m0-decide` therefore asks this function alongside the decision
/// and writes the answers as additional certificates.
///
/// Selection is by what the loop carries, mirroring the dispatcher's own order
/// so the evidence is attributed to the level that understands the loop:
///
/// - a `recursion` is M3's, and its trip count comes from the loop the
///   recursion denotes (a recursion loop's JSON `ivs` are placeholders — the
///   recursion *is* the range);
/// - a `chase` is M5's, and the same holds: the evidence is computed on the
///   expanded loop and relabelled with the file's hash;
/// - a loop with a `Recurrence` index is M8's and has real `ivs`, so it is
///   counted directly;
/// - any other loop with at least one IV is counted as M0's.
///
/// A loop whose guard is not an ascending `x < bound` comparison (a descending
/// recursion) and a loop with no IVs have no trip count to claim: the result is
/// a missing entry, not a padded one.
pub fn decide_evidence(loop_: &Loop) -> Vec<Certificate> {
    use crate::certificate::emit;
    let mut out = Vec::new();

    // ---- Trip count. ----
    let tc = if let Some(cert) = m3::trip_count_certificate(loop_) {
        Some(cert)
    } else if loop_.chase.is_some() {
        // M5's loop range is the *expanded* loop's: a chase loop's JSON `ivs`
        // are placeholders, exactly as a recursion loop's are, so the evidence
        // is computed on the expansion and relabelled with the file's hash.
        match m5::expand(loop_) {
            Some(expanded) => emit::loop_trip_count_certificate(
                &expanded,
                emit::evidence_pair(&expanded),
                "M5",
            )
            .map(|mut c| {
                c.loop_hash = loop_.hash();
                c
            }),
            None => None,
        }
    } else if !loop_.ivs.is_empty() {
        let uses_recurrence = loop_
            .body
            .iter()
            .any(|a| matches!(&a.index_kind, IndexKind::Recurrence(_)));
        let level = if uses_recurrence { "M8" } else { "M0" };
        emit::loop_trip_count_certificate(loop_, emit::evidence_pair(loop_), level)
    } else {
        None
    };
    out.extend(tc);

    // ---- Scalar dependence. ----
    // Carried across the back edge by whatever variable the analyses find
    // first; the checker re-derives the whole table from the carried CFG.
    if let Some(dd) =
        emit::scalar_dependence_certificate(loop_, emit::evidence_pair(loop_), "SCALARDD")
    {
        out.push(dd);
    }

    // ---- Phase 6: array regions. ----
    // One `ArrayRegion` claim per access that has a derivable region, and one
    // `FunctionSummary` claim for the whole loop. Both carry the loop as
    // evidence; the checker re-derives the region/summary from it.
    for access in 0..loop_.body.len() {
        if let Some(c) =
            emit::array_region_certificate(loop_, access, emit::evidence_pair(loop_), "REGION")
        {
            out.push(c);
        }
    }
    if let Some(c) = emit::function_summary_certificate(loop_, emit::evidence_pair(loop_), "REGION")
    {
        out.push(c);
    }

    // ---- Phase 5: reductions, loop kind, alignment. ----
    // Each carries the loop; the checker re-derives the claim from it. These
    // are only produced when the loop has the scalar structure the analyses
    // need (a reduction claim needs a CFG with real definitions).
    if let Some(c) = emit::reduction_certificate(loop_, emit::evidence_pair(loop_), "REDUCTION") {
        out.push(c);
    }
    if let Some(c) = emit::loop_kind_certificate(loop_, emit::evidence_pair(loop_), "LOOPKIND") {
        out.push(c);
    }
    if let Some(c) = emit::alignment_certificate(loop_, emit::evidence_pair(loop_), "ALIGN") {
        out.push(c);
    }

    out
}
mod h5_composed_tests;
