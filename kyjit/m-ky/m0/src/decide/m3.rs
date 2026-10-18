//! M3 — bounded recursion.
//!
//! Scope (v1): a self-recursive function of the form
//!     fn f(iv, ...) { if (iv <op> bound) return; ...; return f(iv + step, ...); }
//! with a single recursive call, a non-zero constant step, and a guard
//! that strictly bounds the IV. This is behaviourally a `for` loop, so
//! M3 rewrites it into a standard Loop and delegates to M0.
//!
//! Refuses (returns Unknown):
//!   - step == 0 (infinite loop)
//!   - sign of step incompatible with bound_op (would never terminate)
//!   - bound already exceeded at iv_init (0 iterations)  -> returns None
//!   - multiple recursive calls, mutual recursion, non-tail recursion
//!
//! Certificates emitted by M0 on the transformed loop are relabelled
//! "M3". The checker sees a standard affine loop and needs no changes.

use super::{m0, Decision};
use crate::certificate::{CConstraint, Certificate, Claim, Witness, RewriteRule};
use crate::fud::{build_cfg_from_loop, build_fud_chains, induct, Class};
use crate::ir::*;

/// The IV range a recursion's *guard* denotes, as a function of the initial
/// value and step the caller already derived.
///
/// This is the one piece of recursion handling `induct` cannot supply: the CFG
/// builder emits the loop guard as [`crate::ir::StmtKind::Guard`] over the IV
/// alone, so the comparison's other operand (`iv_bound`) and its relation
/// (`bound_op`) are not present as scalar uses any analysis can read. The
/// initial value and the step used to come from here too — they now come from
/// [`derive_iv`], and this function only turns a derived `(init, step)` pair
/// plus the guard into a half-open `[lower, upper)` range.
///
/// Returns `None` for a step that never terminates against its guard.
fn guard_range(init: i64, step: i64, bound: i64, op: CmpOp) -> Option<(i64, i64)> {
    if step == 0 {
        return None;
    }
    let (lower, upper) = match (step.signum(), op) {
        // Positive step, iv < bound     -> [init, bound)
        (1, CmpOp::Lt) => (init, bound),
        // Positive step, iv <= bound    -> [init, bound + 1)
        (1, CmpOp::Le) => {
            let ub = bound.checked_add(1)?;
            (init, ub)
        }
        // Negative step, iv > bound     -> (bound, init]  => [bound + 1, init + 1)
        (-1, CmpOp::Gt) => {
            let lb = bound.checked_add(1)?;
            let ub = init.checked_add(1)?;
            (lb, ub)
        }
        // Negative step, iv >= bound    -> [bound, init]  => [bound, init + 1)
        (-1, CmpOp::Ge) => {
            let ub = init.checked_add(1)?;
            (bound, ub)
        }
        // Incompatible combinations would never terminate.
        _ => return None,
    };
    Some((lower, upper))
}

/// The step one full round of the recursion applies to its variable: the
/// companion (a mutual `f`-then-`g` pair) contributes its own step, so the
/// round advances by the sum. Both steps must point the same way or the IV
/// oscillates and never terminates.
fn effective_step(r: &Recursion) -> Option<i64> {
    if r.iv_step == 0 {
        return None;
    }
    match &r.companion {
        Some(c) => {
            if c.iv_step == 0 {
                return None;
            }
            if (r.iv_step > 0) != (c.iv_step > 0) {
                return None;
            }
            r.iv_step.checked_add(c.iv_step)
        }
        None => Some(r.iv_step),
    }
}

/// An induction variable recovered from the loop's own CFG.
///
/// `init` and `step` are `induct`'s `a` and `b` for the IV's header φ — the
/// closed form `a + b·k` over the iteration index — not fields copied out of
/// `Recursion`. `cfg` is the φ-annotated five-block CFG they were derived from,
/// which is what a certificate carries so `m0-check` can re-derive them.
pub struct DerivedIv {
    pub init: i64,
    pub step: i64,
    pub var: usize,
    pub header: usize,
    pub cfg: CFG,
}

/// Derive the recursion variable's initial value and step by running the Phase
/// 4 analyses over the loop's CFG (Phase 9a §4.2).
///
/// This replaces the body `recursion_to_iv` used to carry, which read
/// `iv_init` and `iv_step` straight out of the metadata and so never consulted
/// the CFG builder at all. The rewired path:
///
/// 1. seeds a provisional loop whose IV carries the recursion's starting point
///    and per-round step (the JSON `ivs` on a recursion loop are placeholders —
///    the recursion *is* the loop's range);
/// 2. builds the five-block CFG ([`crate::fud::build_cfg_from_loop`]), whose
///    back edge `latch(3) → header(1)` is what makes the IV loop-carried;
/// 3. rebuilds the FUD chains over it ([`crate::fud::build_fud_chains`]);
/// 4. runs [`crate::fud::induct`], which finds the header φ and solves its
///    recurrences, and requires the variable to classify as
///    [`Class::LinearIv`] — a variable `induct` cannot prove linear is refused
///    here rather than assumed affine;
/// 5. requires the derived `a`/`b` to equal the metadata's `iv_init`/step and
///    the scalar's name to be the recursion's variable.
///
/// Step 5 is what makes this a check and not a re-derivation from thin air: the
/// closed form comes from the CFG, and any disagreement with the declared
/// recursion means M3 declines the loop.
fn derive_iv(loop_: &Loop, r: &Recursion) -> Option<DerivedIv> {
    let step = effective_step(r)?;
    let name = r.iv_name.clone();

    // The provisional loop: the recursion's range expressed as an IV so the CFG
    // builder can lower it. `upper` only sizes the guard, which is the part of
    // the range this function does *not* claim to derive.
    let provisional = Loop {
        depth: 1,
        ivs: vec![IV::with_bound_op(name.clone(), r.iv_init, r.iv_bound, step, r.bound_op)],
        body: loop_.body.clone(),
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    };

    let cfg = build_cfg_from_loop(&provisional)?;
    let (annotated, chains) = build_fud_chains(&cfg);
    let ind = induct(&annotated, &chains);

    // The recursion drives the loop's first IV, which the builder registers at
    // scalar slot 0.
    let iv = ind.ivs.iter().find(|v| v.var == 0)?;
    let (a, b) = match ind.class.get(&iv.op)? {
        Class::LinearIv { a, b } => (*a, *b),
        _ => return None,
    };

    // The derived closed form must agree with the declared recursion, and the
    // variable `induct` found must really be the recursion's own.
    if a != r.iv_init || b != step {
        return None;
    }
    let declared = annotated.vars.get(iv.var)?;
    if declared.name.as_str() != name.as_str() {
        return None;
    }

    Some(DerivedIv { init: a, step: b, var: iv.var, header: iv.loop_, cfg: annotated })
}

/// Compute the loop IV range from a `Loop`'s recursion descriptor: the initial
/// value and step come from [`derive_iv`] (the CFG analyses), the bounds from
/// [`guard_range`] (the guard metadata).
///
/// Extract the loop bound from the CFG's header block guards.
/// Returns the bound and comparison operator from the first guard on the loop IV.
fn cfg_guard_bound(cfg: &CFG) -> Option<(i64, CmpOp)> {
    // Header is block 1 (after entry block 0)
    if cfg.blocks.len() < 2 {
        return None;
    }
    let header = &cfg.blocks[1];
    if header.guards.is_empty() {
        return None;
    }
    // Find the guard for IV 0 (the recursion's IV)
    for c in &header.guards {
        if c.iv == 0 {
            return Some((c.bound, c.op));
        }
    }
    None
}

/// Returns `None` if the recursion is not transformable, or if `induct` does not
/// classify the recursion variable as a linear induction variable of the loop
/// the CFG builder produced.
fn recursion_to_iv(loop_: &Loop) -> Option<IV> {
    let r = loop_.recursion.as_ref()?;
    let derived = derive_iv(loop_, r)?;
    // Use the bound from the CFG's guard instead of recursion metadata
    let (bound, op) = cfg_guard_bound(&derived.cfg)?;
    let (lower, upper) = guard_range(derived.init, derived.step, bound, op)?;
    Some(IV::with_bound_op(r.iv_name.clone(), lower, upper, derived.step.abs(), op))
}

/// Extract the ascending bound from the CFG's header block guards.
/// Returns the bound converted to the `x < bound` form that `trip_count` consumes.
fn cfg_ascending_bound(cfg: &CFG, step: i64) -> Option<i64> {
    if step <= 0 {
        return None;
    }
    let (bound, op) = cfg_guard_bound(cfg)?;
    match op {
        CmpOp::Lt => Some(bound),
        CmpOp::Le => bound.checked_add(1),
        _ => None,
    }
}

/// The loop a recursion *denotes*, written in the IR's own terms.
///
/// On a recursion-bearing loop the JSON `ivs` are placeholders — the recursion
/// is the loop's range. Running the scalar analyses on the file's `ivs` would
/// therefore analyse a loop the program does not contain. This is the loop they
/// must see instead: IV 0 seeded with the recursion's initial value and
/// advanced by its per-round step, with the guard's bound as the upper limit.
///
/// Returns `None` when the recursion has no non-zero step, or when its guard is
/// not the ascending `x < bound` form.
pub fn recursion_analysis(loop_: &Loop) -> Option<(Loop, i64)> {
    let r = loop_.recursion.as_ref()?;
    let step = effective_step(r)?;

    // Build provisional loop and derive IV to get the CFG with guards
    let derived = derive_iv(loop_, r)?;
    let bound = cfg_ascending_bound(&derived.cfg, step)?;

    let analysis = Loop {
        depth: 1,
        ivs: vec![IV::new(r.iv_name.clone(), r.iv_init, bound, step)],
        body: loop_.body.clone(),
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: loop_.scalars.clone(),
        stmts: loop_.stmts.clone(),
    };
    Some((analysis, bound))
}

/// A §4.2 trip-count certificate for a recursion-bearing loop.
///
/// The claim is computed over [`recursion_analysis`] — the loop the recursion
/// denotes — and then relabelled with the original loop's hash, exactly as M3's
/// access-pair certificates are: `m0-check` reads the file, so the certificate
/// must speak for the file's hash, while the CFG it carries is the analysis
/// loop's. The `count`/`exit_value` are recomputed by the checker from that CFG
/// and the IV's derived closed form, so the only fact taken on faith here is
/// the guard's bound.
pub fn trip_count_certificate(loop_: &Loop) -> Option<Certificate> {
    let r = loop_.recursion.as_ref()?;
    let companion_step = r.companion.as_ref().map(|c| c.iv_step);
    let (analysis, bound) = recursion_analysis(loop_)?;
    let header = crate::certificate::emit::primary_iv_header(&analysis)?;
    let mut cert = crate::certificate::emit::trip_count_certificate(
        &analysis,
        header,
        bound,
        0,
        crate::certificate::emit::evidence_pair(&analysis),
        "M3",
        companion_step,
    )?;
    cert.loop_hash = loop_.hash();
    Some(cert)
}

/// Rewrite a recursion-bearing Loop into a standard affine Loop.
/// Returns None if the recursion is not transformable.
pub fn transform(loop_: &Loop) -> Option<Loop> {
    let iv = recursion_to_iv(loop_)?;

    // Refuse if the loop carries features M3 is not designed to
    // normalize alongside recursion. (The dispatcher routes such
    // loops elsewhere anyway.)
    if !loop_.assertions.is_empty() { return None; }
    for a in &loop_.body {
        if a.indirect.is_some() || a.range.is_some() || a.guard.is_some() {
            return None;
        }
        if a.offset.as_const().is_none() { return None; }
    }

    // Empty domain: recursion never executes.
    if iv.lower >= iv.upper {
        return Some(Loop {
            depth: 1,
            ivs: vec![iv],
            body: loop_.body.clone(),
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,});
    }

    // If the effective step is 1, no normalization is needed.
    if iv.step == 1 {
        return Some(Loop {
            depth: 1,
            ivs: vec![iv],
            body: loop_.body.clone(),
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,});
    }

    // Normalize a step-s IV range to a unit-step IV range.
    //
    // The original recursion visits IVs `iv.lower, iv.lower + s,
    // iv.lower + 2s, ...` up to `iv.upper`. Equivalently, unit-step
    // IV `k ∈ [0, ceil((hi - lo) / s))` with the mapping
    //     real_iv = iv.lower + k * s.
    //
    // For each access, `loc = index * real_iv + offset`
    //                        = index * (iv.lower + k * s) + offset
    //                        = (index * s) * k + (index * iv.lower + offset).
    //
    // So: multiply every index coefficient by `s`, and add
    // `index * iv.lower` to the offset.
    let step = iv.step;
    let count = (iv.upper - iv.lower + step - 1) / step; // ceil

    let normalized_iv = IV::new(iv.name.clone(), 0, count, 1);

    let mut body = loop_.body.clone();
    for a in &mut body {
        let old_offset = a.offset.as_const()?;
        let mut base_shift: i64 = 0;
        for c in a.index.iter_mut() {
            base_shift = base_shift.checked_add(c.checked_mul(iv.lower)?)?;
            *c = c.checked_mul(step)?;
        }
        a.offset = OffsetValue::Const(old_offset.checked_add(base_shift)?);
    }

    Some(Loop {
        depth: 1,
        ivs: vec![normalized_iv],
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,})
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let r = match &loop_.recursion {
        Some(r) => r,
        None => return (Decision::Unknown("M3: no recursion metadata".into()), None),
    };

    let transformed = match transform(loop_) {
        Some(l) => l,
        None => {
            return (
                Decision::Unknown(format!(
                    "M3: recursion on {} step={} op={:?} is not transformable",
                    r.iv_name, r.iv_step, r.bound_op
                )),
                None,
            );
        }
    };

    // Empty domain: 0 iterations.
    if transformed.ivs[0].lower >= transformed.ivs[0].upper {
        let cert = Certificate {
            level: "M3".to_string(),
            loop_hash: loop_.hash(),
            pair: (p, q),
            claim: Claim::None,
            witness: Witness::UnsatProof {
                constraints: vec![CConstraint {
                    coeffs: vec![0; loop_.depth],
                    constant: -1,
                }],
            },
            checker: "m0-check".to_string(),
        };
        return (Decision::None, Some(cert));
    }

    let (d, c) = m0::decide(&transformed, p, q);
    (d, c.map(|mut cert| {
        cert.level = "M3".to_string();
        cert.loop_hash = loop_.hash();
        cert.witness = Witness::Rewrite {
            rule: RewriteRule::M3Recursion,
            original: Box::new(loop_.clone()),
            rewritten: Box::new(transformed),
            inner: Box::new(cert.witness),
        };
        cert
    }))
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(step: i64, bound: i64, op: CmpOp, init: i64) -> Recursion {
        Recursion {
            func: "f".into(),
            iv_name: "i".into(),
            iv_init: init,
            iv_step: step,
            iv_bound: bound,
            bound_op: op,
            other_params: vec![],
            companion: None,
        }
    }

    fn acc(kind: AccessKind, index: i64, offset: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![index],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn make_rec_loop(r: Recursion, body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 0, 1)],
            body,
            recursion: Some(r),
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    /// The two-access body (`A[i] = A[i+1]`) most M3 tests share.
    fn test_body() -> Vec<Access> {
        vec![acc(AccessKind::Write, 1, 0), acc(AccessKind::Read, 1, 1)]
    }

    /// A recursion-bearing loop with the shared body. `recursion_to_iv` takes a
    /// whole `Loop` from Phase 9a onward: the IV it derives comes from the CFG
    /// the loop produces, not from the metadata alone.
    fn rec_loop(r: Recursion) -> Loop {
        make_rec_loop(r, test_body())
    }

    #[test]
    fn transform_positive_lt() {
        // step 1, i < 64, init 0 -> [0, 64)
        let iv = recursion_to_iv(&rec_loop(rec(1, 64, CmpOp::Lt, 0))).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (0, 64, 1));
    }

    #[test]
    fn transform_positive_le() {
        // step 1, i <= 63, init 0 -> [0, 64)
        let iv = recursion_to_iv(&rec_loop(rec(1, 63, CmpOp::Le, 0))).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (0, 64, 1));
    }

    #[test]
    fn transform_negative_gt() {
        // step -1, i > 0, init 64 -> [1, 65)  i.e. i in {64, 63, ..., 1}
        let iv = recursion_to_iv(&rec_loop(rec(-1, 0, CmpOp::Gt, 64))).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (1, 65, 1));
    }

    #[test]
    fn transform_negative_ge() {
        // step -1, i >= 0, init 64 -> [0, 65)
        let iv = recursion_to_iv(&rec_loop(rec(-1, 0, CmpOp::Ge, 64))).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (0, 65, 1));
    }

    #[test]
    fn transform_rejects_zero_step() {
        assert!(recursion_to_iv(&rec_loop(rec(0, 64, CmpOp::Lt, 0))).is_none());
    }

    #[test]
    fn transform_rejects_incompatible_op() {
        // positive step, i > bound -> would never terminate
        assert!(recursion_to_iv(&rec_loop(rec(1, 64, CmpOp::Gt, 0))).is_none());
        // negative step, i < bound -> would never terminate
        assert!(recursion_to_iv(&rec_loop(rec(-1, 0, CmpOp::Lt, 64))).is_none());
    }

    #[test]
    fn decide_a001_exact() {
        // sum(i, n): if (i>=n) return; A[i] = A[i+1]; sum(i+1, n)
        // init 0, bound 64, step +1, op Lt
        let r = rec(1, 64, CmpOp::Lt, 0);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),   // A[i]
                acc(AccessKind::Read,  1, 1),   // A[i+1]
            ],
        );
        let (d, cert) = decide(&l, 0, 1);
        match d {
            Decision::Exact(v) => assert_eq!(v, vec![-1]),
            other => panic!("expected Exact, got {:?}", other),
        }
        assert_eq!(cert.unwrap().level, "M3");
    }

    #[test]
    fn decide_a002_zero_distance() {
        let r = rec(1, 64, CmpOp::Lt, 0);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 0),
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
    }

    #[test]
    fn decide_a003_none_gcd() {
        // A[i] = A[i+2]: gcd 1 divides 2 -> EXACT -2, not NONE.
        // Actually stride same (1), diff = -2, so distance = 2. Exact.
        let r = rec(1, 64, CmpOp::Lt, 0);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 2),
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)));
    }

    #[test]
    fn decide_a004_empty_domain() {
        // init 0, bound 0, i < 0 -> empty
        let r = rec(1, 0, CmpOp::Lt, 0);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
        );
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
        assert_eq!(cert.unwrap().level, "M3");
    }

    #[test]
    fn decide_a005_negative_step() {
        // step -1, i > 0, init 64 -> i in [1, 65)
        let r = rec(-1, 0, CmpOp::Gt, 64);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
        );
        let (d, cert) = decide(&l, 0, 1);
        // i in [1, 65), write A[i], read A[i+1]
        // Distance -1 (still EXACT, same shape)
        assert!(matches!(d, Decision::Exact(_)));
        assert_eq!(cert.unwrap().level, "M3");
    }

    #[test]
    fn decide_a006_zero_step() {
        let r = rec(0, 64, CmpOp::Lt, 0);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn decide_a007_no_recursion() {
        let l = Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 64, 1)],
            body: vec![
                acc(AccessKind::Write, 1, 0),
                acc(AccessKind::Read,  1, 1),
            ],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    // ----- M3.5: mutual recursion -----

    fn rec_with_companion(
        step: i64,
        c_step: i64,
        bound: i64,
        op: CmpOp,
        init: i64,
    ) -> Recursion {
        Recursion {
            func: "f".into(),
            iv_name: "i".into(),
            iv_init: init,
            iv_step: step,
            iv_bound: bound,
            bound_op: op,
            other_params: vec![],
            companion: Some(Box::new(Recursion {
                func: "g".into(),
                iv_name: "i".into(),
                iv_init: init,
                iv_step: c_step,
                iv_bound: bound,
                bound_op: op,
                other_params: vec![],
                companion: None,
            })),
        }
    }

    #[test]
    fn companion_sums_steps() {
        // f: i += 1, g: i += 2 → effective step 3.
        // Init 0, bound 15, i < 15 → IV = [0, 15) step 3.
        let r = rec_with_companion(1, 2, 15, CmpOp::Lt, 0);
        let iv = recursion_to_iv(&make_rec_loop(r.clone(), test_body())).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (0, 15, 3));
    }

    #[test]
    fn companion_negative_steps() {
        // f: i -= 1, g: i -= 2 → effective step 3.
        // Init 30, bound 0, i > 0 → IV = [1, 31) step 3.
        let r = rec_with_companion(-1, -2, 0, CmpOp::Gt, 30);
        let iv = recursion_to_iv(&make_rec_loop(r.clone(), test_body())).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (1, 31, 3));
    }

    #[test]
    fn companion_incompatible_directions_refused() {
        // f: i += 1, g: i -= 1 → net 0. Refuse.
        let r = rec_with_companion(1, -1, 15, CmpOp::Lt, 0);
        assert!(recursion_to_iv(&make_rec_loop(r.clone(), test_body())).is_none());
    }

    #[test]
    fn companion_zero_step_refused() {
        // g: i += 0 → not a valid companion.
        let r = rec_with_companion(1, 0, 15, CmpOp::Lt, 0);
        assert!(recursion_to_iv(&make_rec_loop(r.clone(), test_body())).is_none());
    }

    #[test]
    fn companion_decide_exact() {
        // Mutual recursion f<->g with iv += 1 on f, iv += 2 on g.
        // Effective step 3, so the real IV sequence is {0, 3, 6, 9, 12}.
        // Body has A[i] = A[i+1]: write locations {0,3,6,9,12},
        // read locations {1,4,7,10,13}. Disjoint → None.
        //
        // The previous version of this test asserted Exact([-1]),
        // which was a false positive: M0 iterated the transformed
        // loop with step 1 instead of step 3, producing spurious
        // collisions at i=0,j=1 etc. Fixed by normalizing the step-s
        // range to a unit-step range with scaled coefficients in
        // M3::transform (see fuzz_m35_property for the guard).
        let r = rec_with_companion(1, 2, 15, CmpOp::Lt, 0);
        let l = make_rec_loop(
            r,
            vec![
                acc(AccessKind::Write, 1, 0),   // A[i]
                acc(AccessKind::Read,  1, 1),   // A[i+1]
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(
            matches!(d, Decision::None),
            "expected None, got {:?}",
            d
        );
    }

    #[test]
    fn companion_missing_inner_field_not_refused() {
        // Regression: recursion without companion still works (M3 path).
        let r = rec(1, 64, CmpOp::Lt, 0);
        let iv = recursion_to_iv(&make_rec_loop(r.clone(), test_body())).unwrap();
        assert_eq!((iv.lower, iv.upper, iv.step), (0, 64, 1));
    }
    // ================================================================
    // M35 property fuzz: mutual recursion (M3 with a companion).
    // Mirrors recursion_to_iv's companion handling: effective step
    // is iv_step + companion.iv_step, both same sign. The IV range
    // is [lo, hi) with step = |effective_step|.
    // ================================================================

    fn xorshift35(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn rec_with_companion_35(
        iv_init: i64, iv_step: i64, iv_bound: i64, op: CmpOp,
        comp_step: i64,
    ) -> Recursion {
        Recursion {
            func: "f".into(),
            iv_name: "i".into(),
            iv_init,
            iv_step,
            iv_bound,
            bound_op: op,
            other_params: vec![],
            companion: Some(Box::new(Recursion {
                func: "g".into(),
                iv_name: "i".into(),
                iv_init,
                iv_step: comp_step,
                iv_bound,
                bound_op: op,
                other_params: vec![],
                companion: None,
            })),
        }
    }

    fn effective_range_35(
        iv_init: i64, iv_step: i64, iv_bound: i64, op: CmpOp, comp_step: i64,
    ) -> Option<(i64, i64, i64)> {
        // Returns (lower, upper, step) or None if not transformable.
        if iv_step == 0 || comp_step == 0 { return None; }
        if (iv_step > 0) != (comp_step > 0) { return None; }
        let effective_step = iv_step.checked_add(comp_step)?;
        let (lo, hi) = match (effective_step.signum(), op) {
            (1, CmpOp::Lt) => (iv_init, iv_bound),
            (1, CmpOp::Le) => (iv_init, iv_bound.checked_add(1)?),
            (-1, CmpOp::Gt) => (iv_bound.checked_add(1)?, iv_init.checked_add(1)?),
            (-1, CmpOp::Ge) => (iv_bound, iv_init.checked_add(1)?),
            _ => return None,
        };
        Some((lo, hi, effective_step.abs()))
    }

    fn brute_35(
        ip: i64, off_p: i64, iq: i64, off_q: i64,
        lo: i64, hi: i64, step: i64,
    ) -> Option<Option<i64>> {
        // Returns:
        //   None            -> empty domain
        //   Some(None)      -> no collision
        //   Some(Some(d))   -> collision at some (i, j)
        if lo >= hi || step <= 0 { return None; }
        let mut writes: Vec<(i64, i64)> = Vec::new();
        let mut v = lo;
        while v < hi {
            writes.push((ip * v + off_p, v));
            v += step;
        }
        let mut v = lo;
        while v < hi {
            let loc_q = iq * v + off_q;
            for (loc_p, _) in &writes {
                if *loc_p == loc_q {
                    return Some(Some(0));
                }
            }
            v += step;
        }
        Some(None)
    }

    #[test]
    fn fuzz_m35_property() {
        let mut state: u64 = 0x3535_3535_dead_beef;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut errors = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Same-sign step pair. Effective step between 1 and 8.
            let base_step = ((xorshift35(&mut state) % 4) as i64) + 1;   // 1..4
            let comp_step = ((xorshift35(&mut state) % 4) as i64) + 1;   // 1..4
            let sign = if xorshift35(&mut state) % 2 == 0 { 1 } else { -1 };
            let iv_step = sign * base_step;
            let comp_step = sign * comp_step;

            let iv_init = (xorshift35(&mut state) % 40) as i64;
            let iv_bound = (xorshift35(&mut state) % 40) as i64;

            // Same-sign step + op compatibility: use lt/le for positive,
            // gt/ge for negative.
            let op = if sign > 0 {
                if xorshift35(&mut state) % 2 == 0 { CmpOp::Lt } else { CmpOp::Le }
            } else {
                if xorshift35(&mut state) % 2 == 0 { CmpOp::Gt } else { CmpOp::Ge }
            };

            let range = match effective_range_35(iv_init, iv_step, iv_bound, op, comp_step) {
                Some(r) => r,
                None => continue,
            };
            let (lo, hi, step) = range;
            if lo >= hi { continue; }
            // Cap the iteration count to keep brute force fast.
            let count = ((hi - lo) / step) as usize;
            if count == 0 || count > 256 { continue; }

            let ip = match xorshift35(&mut state) % 3 { 0 => 0, 1 => 1, _ => 2 };
            let iq = match xorshift35(&mut state) % 3 { 0 => 0, 1 => 1, _ => 2 };
            let off_p = (xorshift35(&mut state) % 5) as i64 - 2;
            let off_q = (xorshift35(&mut state) % 5) as i64 - 2;

            let r = rec_with_companion_35(iv_init, iv_step, iv_bound, op, comp_step);
            let l = make_rec_loop(
                r,
                vec![
                    acc(AccessKind::Write, ip, off_p),
                    acc(AccessKind::Read,  iq, off_q),
                ],
            );

            let expected = brute_35(ip, off_p, iq, off_q, lo, hi, step);

            let (d, _) = decide(&l, 0, 1);
            tested += 1;

            let ok = match (expected, &d) {
                (None, Decision::Unknown(_)) => true,
                (None, _) => false,
                (Some(None), Decision::None) => true,
                (Some(None), Decision::Unknown(_)) => { conservative += 1; true }
                (Some(None), _) => false,
                (Some(Some(_)), Decision::Exact(_)) => true,
                (Some(Some(_)), Decision::Unknown(_)) => { conservative += 1; true }
                (Some(Some(_)), _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "iv_init={} iv_step={} iv_bound={} op={:?} comp_step={} \
                         ip={} off_p={} iq={} off_q={} range=({},{},{}) expected={:?} got={:?}",
                        iv_init, iv_step, iv_bound, op, comp_step,
                        ip, off_p, iq, off_q, lo, hi, step, expected, d
                    ));
                }
            }
        }

        println!("M35 fuzz: tested={} unsound={} conservative={} errors={}",
                 tested, unsound, conservative, errors);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M35 fuzz found {} unsound cases", unsound);
    }


    // ---- Phase 9a §4.2: the analysis loop a recursion denotes. ----

    #[test]
    fn recursion_analysis_uses_the_recursion_not_the_placeholder_ivs() {
        // A recursion loop's JSON `ivs` are `upper: 0` placeholders — the
        // recursion *is* the range. The analysis loop must carry the recursion's
        // own init/step/bound, or the scalar analyses would look at a loop that
        // runs zero times.
        let l = rec_loop(rec(2, 20, CmpOp::Lt, 4));
        let (analysis, bound) = recursion_analysis(&l).unwrap();
        assert_eq!(bound, 20);
        assert_eq!(analysis.ivs.len(), 1);
        assert_eq!(analysis.ivs[0].name, "i");
        assert_eq!(analysis.ivs[0].lower, 4);
        assert_eq!(analysis.ivs[0].upper, 20);
        assert_eq!(analysis.ivs[0].step, 2);
        // The recursion is gone: the analysis loop is an ordinary affine loop,
        // which is what lets `build_cfg_from_loop` give it a back edge.
        assert!(analysis.recursion.is_none());
    }

    #[test]
    fn recursion_analysis_lifts_le_to_one_past_the_bound() {
        // `i <= 20` is the guard `i < 21`.
        let (_, bound) = recursion_analysis(&rec_loop(rec(1, 20, CmpOp::Le, 0))).unwrap();
        assert_eq!(bound, 21);
    }

    #[test]
    fn recursion_analysis_refuses_descending_and_unsuitable_guards() {
        // A descending recursion has no ascending bound, and the trip-count
        // recomputation in the checker is defined on `x < bound` only.
        assert!(recursion_analysis(&rec_loop(rec(-1, 0, CmpOp::Gt, 64))).is_none());
        assert!(recursion_analysis(&rec_loop(rec(-1, 0, CmpOp::Ge, 64))).is_none());
        // No step: not a loop.
        assert!(recursion_analysis(&rec_loop(rec(0, 64, CmpOp::Lt, 0))).is_none());
        // No recursion at all.
        let mut plain = rec_loop(rec(1, 64, CmpOp::Lt, 0));
        plain.recursion = None;
        assert!(recursion_analysis(&plain).is_none());
    }

    #[test]
    fn trip_count_certificate_claims_the_recursion_count() {
        // 0,1,...,63 -> 64 trips, exit value 64.
        let l = rec_loop(rec(1, 64, CmpOp::Lt, 0));
        let cert = trip_count_certificate(&l).unwrap();
        match &cert.claim {
            Claim::TripCount { count, exit_value } => assert_eq!(
                (*count, *exit_value),
                (64, 64),
                "0<=i<64 step 1 is 64 trips exiting at 64"
            ),
            other => panic!("expected a TripCount claim, got {other:?}"),
        }
        // The certificate speaks for the *file's* loop, not the analysis loop.
        assert_eq!(cert.loop_hash, l.hash());
        assert_eq!(cert.level, "M3");
    }

    #[test]
    fn trip_count_certificate_steps_and_offsets() {
        // i = 4; i < 20; i += 2  ->  i = 4,6,...,18  ->  8 trips, exit 20.
        let l = rec_loop(rec(2, 20, CmpOp::Lt, 4));
        let cert = trip_count_certificate(&l).unwrap();
        match &cert.claim {
            // 4,6,...,18 -> 8 trips, exit 20.
            Claim::TripCount { count, exit_value } => {
                assert_eq!((*count, *exit_value), (8, 20))
            }
            other => panic!("expected a TripCount claim, got {other:?}"),
        }
    }

    #[test]
    fn trip_count_certificate_absent_when_the_loop_is_not_countable() {
        // A descending recursion has no trip count to claim; the emitter says so
        // by returning None rather than by asserting a count it cannot defend.
        assert!(trip_count_certificate(&rec_loop(rec(-1, 0, CmpOp::Gt, 64))).is_none());
        let mut plain = rec_loop(rec(1, 64, CmpOp::Lt, 0));
        plain.recursion = None;
        assert!(trip_count_certificate(&plain).is_none());
    }

    #[test]
    fn trip_count_certificate_derives_its_bound_through_the_cfg() {
        // The witness must name the IV that `induct` proved over the carried
        // CFG — the checker's arm looks the pair (var, loop) up again and
        // rejects a certificate that nominates anything else.
        let l = rec_loop(rec(1, 64, CmpOp::Lt, 0));
        let cert = trip_count_certificate(&l).unwrap();
        let (var, header, bound) = match &cert.witness {
            Witness::TripCountEvidence { cfg, iv_var, loop_, bound, bound_step, companion_step: _ } => {
                assert_eq!(cfg.blocks.len(), 5, "expected the five-block loop CFG");
                // The back edge is what makes this a loop the analyses can read.
                assert!(cfg.blocks[3].succs.contains(&1), "latch must feed the header");
                (*iv_var, *loop_, *bound)
            }
            other => panic!("expected TripCountEvidence, got {other:?}"),
        };
        assert_eq!((var, header, bound), (0, 1, 64));
    }

}
