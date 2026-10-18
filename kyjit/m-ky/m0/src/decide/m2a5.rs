//! M2a.5 — table-literal injectivity.
//!
//! Extends M2a to `A[B[i]]` where `B` is a literal array
//! `B = [v0, v1, ..., vn-1]` and the values in the IV domain are
//! pairwise distinct. In that case, `B` is injective on the domain,
//! so `B[i] = B[j]` iff `i = j`.
//!
//! Injectivity constrains *which cell* an iteration names; it says nothing on
//! its own about *where* an access lands, because the location is
//! `B[i] + offset`. Two accesses therefore collide exactly when two cells of
//! the table differ by the difference of their offsets — so distinct offsets
//! neither prove nor refute a dependence, and the offset gap has to be checked
//! against the table itself (see [`colliding_cells`]).
//!
//! Scope (v1):
//!   - Both accesses have `indirect.table = Some(_)` with the SAME base
//!     array and the same table and domain.
//!   - Values in [domain_lo, domain_hi) are pairwise distinct.
//!   - Depth 1, unguarded, constant offsets, and an iteration space that stays
//!     inside the domain (an `UnsatProof` is only emitted when the checker can
//!     walk every cell itself).
//!   - Otherwise Unknown (mixed table + affine, mixed table + direct,
//!     different tables or domains, guarded, parametric offsets, etc.).
//!
//! The checker independently verifies the injectivity premise on every
//! `IntSolution` certificate and re-derives every absence claim from the loop,
//! so even if M2a.5 is buggy, unsound certificates are rejected downstream.

use super::Decision;
use crate::certificate::{Certificate, Claim, Witness};
use crate::ir::*;

/// Return the table of a table-indirect access, plus its domain.
///
/// An access without a literal table is not this level's to decide, and is
/// reported as such rather than defaulting to the empty table: a vacuous
/// injectivity claim there would let an access that merely *looks* tabled into
/// the reasoning below.
fn table_of<'a>(access: &'a Access) -> Option<(&'a str, &'a [i64], i64, i64)> {
    let ind = access.indirect.as_ref()?;
    let tbl = ind.table.as_ref()?;
    Some((ind.base.as_str(), tbl.as_slice(), ind.domain_lo, ind.domain_hi))
}

/// Verify the table is injective on [lo, hi).
fn table_injective(tbl: &[i64], lo: i64, hi: i64) -> bool {
    if lo < 0 || hi < lo || (hi as usize) > tbl.len() {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    for &v in &tbl[lo as usize..hi as usize] {
        if !seen.insert(v) {
            return false;
        }
    }
    true
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    let (base_p, tbl_p, lo_p, hi_p) = match table_of(ap) {
        Some(x) => x,
        None => return (Decision::Unknown("M2a.5: p is not table-indirect".into()), None),
    };
    let (base_q, tbl_q, lo_q, hi_q) = match table_of(aq) {
        Some(x) => x,
        None => return (Decision::Unknown("M2a.5: q is not table-indirect".into()), None),
    };

    // Both must reference the same table. Different tables -> unknown.
    if base_p != base_q || tbl_p != tbl_q {
        return (Decision::Unknown("M2a.5: tables differ".into()), None);
    }
    if (lo_p, hi_p) != (lo_q, hi_q) {
        return (Decision::Unknown("M2a.5: table domains differ".into()), None);
    }
    if !table_injective(tbl_p, lo_p, hi_p) {
        return (
            Decision::Unknown("M2a.5: table not injective on domain".into()),
            None,
        );
    }
    // An empty domain, and a loop whose own lattice never enters the domain,
    // are both handled below: neither leaves a cell for the accesses to name,
    // and the same reachability walk that settles collisions settles that.

    // Both accesses reference the SAME table on the SAME domain. The location
    // each names is `tbl[iv] + own_offset`, so distinct offsets neither prove
    // nor refute a dependence: the offset gap has to be checked against the
    // table itself.
    let off_p = match ap.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("M2a.5: parametric offset".into()), None),
    };
    let off_q = match aq.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("M2a.5: parametric offset".into()), None),
    };

    // A guard narrows the iterations on which an access runs, and this path
    // reasons over the declared table domain instead: leave guarded loops to M4.
    if ap.guard.is_some() || aq.guard.is_some() {
        return (Decision::Unknown("M2a.5: guarded access".into()), None);
    }
    // The table subscript is IV 0, so a witness for depth > 1 would need the
    // other levels pinned; that is out of scope here.
    if loop_.depth != 1 {
        return (Decision::Unknown("M2a.5: depth>1 unsupported".into()), None);
    }
    // Under M3 the `ivs` are placeholders, and an access whose index or range
    // moves the location is not the pure lookup this path models. Reading a
    // wider shape here would judge it against a model the checker does not
    // share, so both are refused.
    if loop_.recursion.is_some() || loop_.ivs.len() != loop_.depth {
        return (Decision::Unknown("M2a.5: recursion or parametric bounds".into()), None);
    }
    if !pure_lookup(ap) || !pure_lookup(aq) {
        return (Decision::Unknown("M2a.5: access is not a pure table lookup".into()), None);
    }

    // The cells the loop actually names. A cell the loop's own lattice skips is
    // executed by no iteration, so a collision found only there is a phantom
    // dependence between two runs of the program that never happen.
    let cells = match reached_cells(&loop_.ivs[0], lo_p, hi_p) {
        Some(v) => v,
        None => return (Decision::Unknown("M2a.5: iteration range overflow".into()), None),
    };
    match colliding_cells(tbl_p, &cells, off_p, off_q) {
        // A colliding pair of *executed* cells names the same location: a
        // dependence at distance `cj - ci`, which the checker re-derives from
        // the table.
        Cells::Collide(ci, cj) => {
            let d = match cj.checked_sub(ci) {
                Some(v) => vec![v],
                None => return (Decision::Unknown("M2a.5: distance overflow".into()), None),
            };
            let cert = Certificate {
                level: "M2a.5".to_string(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i: vec![ci], j: vec![cj] },
                checker: "m0-check".to_string(),
            };
            return (Decision::Exact(d), Some(cert));
        }
        // Overflowed arithmetic is not a proof of separation.
        Cells::Indecisive => {
            return (Decision::Unknown("M2a.5: offset gap overflows".into()), None)
        }
        // No pair of executed cells, shifted by the respective offsets, agrees:
        // the two accesses name disjoint location sets on every iteration the
        // loop performs, so the dependence is refuted. That covers the wide
        // loop whose lattice never enters the domain (no executed cell at all)
        // and the empty domain, where `tbl[i]` names nothing.
        //
        // The claim is only emitted where the checker can re-derive it: it
        // enumerates the iteration space of each access, so a loop longer than
        // its own budget is left Unknown rather than certified and rejected.
        Cells::Disjoint => {}
    }
    let walk = match lattice_len(&loop_.ivs[0]) {
        Some(v) => v,
        None => return (Decision::Unknown("M2a.5: iteration range overflow".into()), None),
    };
    if !absence_verifiable(walk, cells.len() as i64) {
        return (
            Decision::Unknown("M2a.5: iteration space beyond the checker's enumeration".into()),
            None,
        );
    }
    let cert = Certificate {
        level: "M2a.5".to_string(),
        loop_hash: loop_.hash(),
        pair: (p, q),
        claim: Claim::None,
        witness: Witness::UnsatProof {
            constraints: vec![crate::certificate::CConstraint {
                coeffs: vec![0; loop_.depth],
                constant: -1,
            }],
        },
        checker: "m0-check".to_string(),
    };
    (Decision::None, Some(cert))
}

/// Whether an access is exactly the `A[B[i]]` lookup M2a.5 models: an affine
/// subscript with no outer term and no range folding, so the location is the
/// table cell named by IV 0 plus the access offset and nothing else.
///
/// This mirrors the checker's own table branch: `witness_location` evaluates a
/// literal table as `tbl[iv_values[0]] + offset`, and `refutation_location`
/// refuses any access with a non-affine index kind, a range, or a nonzero outer
/// affine term. Anything wider would be judged here against a model the checker
/// does not share, so it is left to another level.
fn pure_lookup(access: &Access) -> bool {
    access.indirect.as_ref().map_or(false, |i| i.table.is_some())
        && matches!(access.index_kind, IndexKind::Affine)
        && access.range.is_none()
        && access.index.iter().all(|&c| c == 0)
}

/// Cells of the table the loop actually reaches, in iteration order.
///
/// `None` means the walk cannot be carried out in `i64`, or is longer than the
/// checker is willing to enumerate ([`CHECKER_DOMAIN_CAP`]), so no conclusion
/// may be drawn from it.
///
/// The loop runs `lower, lower + step, lower + 2 * step, ...` while below
/// `upper`, and a table cell outside that lattice is named by no iteration the
/// program performs. Intersecting the lattice with the domain is what keeps a
/// stepped loop from reasoning about phantom iterations, and mirrors
/// `executing_iterations` in the checker.
fn reached_cells(iv: &IV, lo: i64, hi: i64) -> Option<Vec<i64>> {
    if iv.step <= 0 || lo >= hi {
        // A non-positive step runs no iteration; an empty domain names no cell.
        return Some(Vec::new());
    }
    let mut start = iv.lower.max(lo);
    let end = iv.upper.min(hi);
    if start >= end {
        return Some(Vec::new());
    }
    // Snap the bottom of the range up onto the loop's own lattice.
    let rem = match start.checked_sub(iv.lower) {
        Some(v) => v.rem_euclid(iv.step),
        None => return None,
    };
    if rem != 0 {
        let step_minus_rem = match iv.step.checked_sub(rem) {
            Some(v) => v,
            None => return None,
        };
        start = match start.checked_add(step_minus_rem) {
            Some(v) => v,
            None => return None,
        };
        if start >= end {
            return Some(Vec::new());
        }
    }
    let count = match end.checked_sub(1).and_then(|e| e.checked_sub(start)) {
        Some(v) => match v.checked_div(iv.step) {
            Some(q) => q + 1,
            None => return None,
        },
        None => return None,
    };
    if count > CHECKER_DOMAIN_CAP {
        return None;
    }
    Some((0..count).filter_map(|n| {
        let n_step = iv.step.checked_mul(n)?;
        start.checked_add(n_step)
    }).collect())
}

/// How many iterations the loop performs, ignoring guards. `None` when that
/// many steps cannot be counted in `i64`.
fn lattice_len(iv: &IV) -> Option<i64> {
    if iv.step <= 0 || iv.lower >= iv.upper {
        return Some(0);
    }
    let span = iv.upper.checked_sub(1)?.checked_sub(iv.lower)?;
    Some(span / iv.step + 1)
}

/// The checker's own enumeration budget, mirrored so M2a.5 only claims absence
/// where the checker can re-derive it. `executing_iterations` refuses a domain
/// above `CHECKER_DOMAIN_CAP` before filtering, and `no_dependence_by_enumeration`
/// refuses a pair walk above `CHECKER_PAIR_CAP`; a certificate past either is
/// rejected rather than accepted, so emitting one would only turn a sound
/// `None` into a checker failure.
const CHECKER_DOMAIN_CAP: i64 = 4096;
const CHECKER_PAIR_CAP: i64 = 1 << 22;

/// Whether the checker can re-derive an absence claim by enumeration. Both of
/// its budgets apply: each access's domain is counted before guards and the
/// table filter, so the loop's full walk must fit `CHECKER_DOMAIN_CAP`, and the
/// pair walk over the cells each access names must fit `CHECKER_PAIR_CAP`.
fn absence_verifiable(walk: i64, cells: i64) -> bool {
    walk <= CHECKER_DOMAIN_CAP
        && cells.checked_mul(cells).map_or(false, |n| n <= CHECKER_PAIR_CAP)
}

/// Find two *reached* cells of `tbl` that, shifted by their access's offset,
/// name the same location: `tbl[ci] + off_p == tbl[cj] + off_q`.
///
/// Equal offsets make every cell such a partner, so a non-empty cell set always
/// yields its first cell against itself. Different offsets need two cells whose
/// values differ by exactly `off_p - off_q`; injectivity on the domain makes
/// each value's cell unique, so one hash of the domain settles it.
enum Cells {
    /// Two reached cells shifted by their offsets name the same location.
    Collide(i64, i64),
    /// No such pair exists -- a *proof* that the accesses cannot share a
    /// location on any iteration the loop performs, and so the basis of the
    /// absence certificate. Only ever returned for a domain `table_injective`
    /// has already accepted.
    Disjoint,
    /// The comparison could not be carried out in `i64`. Arithmetic that
    /// overflows says nothing about disjointness, so the caller must stay
    /// silent rather than claim absence.
    Indecisive,
}

fn colliding_cells(tbl: &[i64], cells: &[i64], off_p: i64, off_q: i64) -> Cells {
    let (&first, _) = match cells.split_first() {
        Some(x) => x,
        // No iteration reaches a cell, so neither access runs and there is no
        // pair of executions to collide.
        None => return Cells::Disjoint,
    };
    if off_p == off_q {
        return Cells::Collide(first, first);
    }
    let gap = match off_p.checked_sub(off_q) {
        Some(v) => v,
        None => return Cells::Indecisive,
    };
    let mut cell_of: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    for &k in cells {
        cell_of.insert(tbl[k as usize], k);
    }
    for &k in cells {
        let target = match tbl[k as usize].checked_add(gap) {
            Some(v) => v,
            None => return Cells::Indecisive,
        };
        if let Some(&j) = cell_of.get(&target) {
            return Cells::Collide(k, j);
        }
    }
    Cells::Disjoint
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc_table(kind: AccessKind, tbl: Vec<i64>, offset: i64, lo: i64, hi: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![0],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: Some(Indirect {
                base: "B".into(),
                index: vec![1],
                offset: 0,
                table: Some(tbl),
                domain_lo: lo,
                domain_hi: hi,
            }),
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn make_loop_1d(body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 64, 1)],
            body,
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    /// A loop whose iteration space is exactly `[lo, hi)`, i.e. one that stays
    /// inside the table domain it indexes. Outside the domain `B[i]` names no
    /// cell, which is why M2a.5 can only refute a dependence here.
    fn make_loop_domain(body: Vec<Access>, lo: i64, hi: i64) -> Loop {
        Loop {
            ivs: vec![IV::new("i", lo, hi, 1)],
            scalars: vec![],
            stmts: None,
            ..make_loop_1d(body)
            }
    }

    #[test]
    fn t001_same_table_same_offset() {
        let tbl = vec![3, 7, 11, 15];
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl.clone(), 0, 0, 4),
            acc_table(AccessKind::Read,  tbl,           0, 0, 4),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M2a.5");
    }

    #[test]
    fn t002_same_table_offset_diff() {
        // B = [3, 7, 11, 15] with offsets 0 and 1 names {3,7,11,15} against
        // {4,8,12,16}: no cell of one set is a cell of the other, so the two
        // accesses cannot collide -- provided the loop stays in the domain.
        let tbl = vec![3, 7, 11, 15];
        let l = make_loop_domain(
            vec![
                acc_table(AccessKind::Write, tbl.clone(), 0, 0, 4),
                acc_table(AccessKind::Read,  tbl,           1, 0, 4),
            ],
            0,
            4,
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
    }

    #[test]
    fn offset_gap_that_matches_cell_gap_is_a_dependence() {
        // The old M2a.5 read "different offsets, therefore no dependence". It
        // is wrong: `A[B[i]]` at offset 0 and `A[B[i]]` at offset 4 collide
        // whenever two cells of B differ by 4, which `[3,7,11,15]` does at
        // every step. B[i] + 0 == B[i+1] - ... -> here loc(i=1, p) = 7 equals
        // loc(j=0, q) = 3 + 4.
        let tbl = vec![3, 7, 11, 15];
        let l = make_loop_domain(
            vec![
                acc_table(AccessKind::Write, tbl.clone(), 0, 0, 4),
                acc_table(AccessKind::Read,  tbl.clone(),   4, 0, 4),
            ],
            0,
            4,
        );
        let (d, cert) = decide(&l, 0, 1);
        let dist = match &d {
            Decision::Exact(v) => v.clone(),
            other => panic!("expected Exact, got {:?}", other),
        };
        let cert = cert.unwrap();
        if let Witness::IntSolution { i, j } = cert.witness {
            // The witness must really be a shared location.
            assert_eq!(tbl[i[0] as usize] + 0, tbl[j[0] as usize] + 4);
            assert_eq!(dist, vec![j[0] - i[0]]);
        } else {
            panic!("expected an IntSolution witness");
        }
    }

    #[test]
    fn absence_claim_survives_a_loop_wider_than_the_domain() {
        // Iterations past the table's domain name no cell, and the checker now
        // models that: it filters the iteration space down to the executions
        // that reach a cell before comparing locations. So the refutation is
        // re-derivable, and M2a.5 may claim absence.
        let tbl = vec![3, 7, 11, 15];
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl.clone(), 0, 0, 4),
            acc_table(AccessKind::Read,  tbl,           1, 0, 4),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None), "got {:?}", d);
        let cert = cert.expect("absence certificate expected");
        assert!(matches!(cert.claim, Claim::None));
    }

    #[test]
    fn no_absence_claim_when_the_loop_is_too_long_to_enumerate() {
        // The refutation is only worth emitting where the checker can re-derive
        // it, and its enumeration is budgeted. A loop longer than that budget is
        // left unknown rather than certified and then rejected.
        let tbl: Vec<i64> = (0..4096).map(|k| k as i64 * 2 + 1).collect();
        let l = Loop {
            ivs: vec![IV::new("i", 0, 8192, 1)],
            scalars: vec![],
            stmts: None,
            ..make_loop_1d(vec![
                acc_table(AccessKind::Write, tbl.clone(), 0, 0, 4096),
                acc_table(AccessKind::Read, tbl, 1, 0, 4096),
            ])
            };
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)), "got {:?}", d);
        assert!(cert.is_none(), "no certificate may accompany an unknown");
    }

    #[test]
    fn collision_only_on_skipped_iterations_is_not_a_dependence() {
        // for (i = 0; i < 5; i += 2) performs i in {0, 2, 4}; cells 1 and 3 are
        // named by no iteration. B = [2,10,4,5,8]: `A[B[i]]` at offset 0 and at
        // offset 5 collide exactly when B[ci] == B[cj] + 5, which holds at
        // (ci, cj) = (1, 3) -- both phantom. The executed cells {0,2,4} give
        // {2,4,8} against {7,9,13}: disjoint, so there is no dependence, and no
        // witness may name an iteration the loop never runs.
        let tbl = vec![2, 10, 4, 5, 8];
        let l = Loop {
            ivs: vec![IV::new("i", 0, 5, 2)],
            scalars: vec![],
            stmts: None,
            ..make_loop_1d(vec![
                acc_table(AccessKind::Write, tbl.clone(), 0, 0, 5),
                acc_table(AccessKind::Read, tbl, 5, 0, 5),
            ])
            };
        let (d, cert) = decide(&l, 0, 1);
        match &d {
            Decision::Exact(v) => panic!("phantom collision claimed at {:?}", v),
            Decision::Unknown(_) => {}
            Decision::None => {
                // Absence is fine, provided the witness it rests on is real.
                if let Some(c) = &cert {
                    assert!(matches!(c.claim, Claim::None));
                }
            }
        }
    }

    #[test]
    fn witness_collides_only_on_iterations_the_loop_reaches() {
        // Same table and offsets, but every cell is reachable: the collision at
        // (1, 3) is a genuine dependence now.
        let tbl = vec![2, 10, 4, 5, 8];
        let l = make_loop_domain(
            vec![
                acc_table(AccessKind::Write, tbl.clone(), 0, 0, 5),
                acc_table(AccessKind::Read, tbl.clone(), 5, 0, 5),
            ],
            0,
            5,
        );
        let (d, cert) = decide(&l, 0, 1);
        let dist = match &d {
            Decision::Exact(v) => v.clone(),
            other => panic!("expected Exact, got {:?}", other),
        };
        let cert = cert.unwrap();
        if let Witness::IntSolution { i, j } = cert.witness {
            assert_eq!(tbl[i[0] as usize] + 0, tbl[j[0] as usize] + 5);
            assert_eq!(dist, vec![j[0] - i[0]]);
            // Both witness iterations must be values the loop assigns.
            let iv = &l.ivs[0];
            for &v in &[i[0], j[0]] {
                assert!(v >= iv.lower && v < iv.upper && (v - iv.lower) % iv.step == 0);
            }
        } else {
            panic!("expected an IntSolution witness");
        }
    }

    #[test]
    fn t003_duplicate_table_rejected() {
        // B = [1, 1, 2, 3] has a duplicate -> not injective.
        let tbl = vec![1, 1, 2, 3];
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl.clone(), 0, 0, 4),
            acc_table(AccessKind::Read,  tbl,           0, 0, 4),
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn t004_mixed_table_and_affine_indirect() {
        let tbl = vec![3, 7, 11, 15];
        let mut affine_acc = acc_table(AccessKind::Read, tbl.clone(), 0, 0, 4);
        // Convert q's indirect to affine (no table).
        if let Some(ind) = affine_acc.indirect.as_mut() {
            ind.table = None;
            ind.index = vec![2];
        }
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl, 0, 0, 4),
            affine_acc,
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn t005_mixed_table_and_no_indirect() {
        let tbl = vec![3, 7, 11, 15];
        let mut direct = acc_table(AccessKind::Read, tbl.clone(), 0, 0, 4);
        direct.indirect = None;
        direct.index = vec![1];
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl, 0, 0, 4),
            direct,
        ]);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn t006_empty_domain() {
        let tbl = vec![3, 7, 11, 15];
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl.clone(), 0, 2, 2),
            acc_table(AccessKind::Read,  tbl,           0, 2, 2),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::None));
        assert_eq!(cert.unwrap().level, "M2a.5");
    }

    #[test]
    fn t007_different_tables_unknown() {
        let tbl_a = vec![3, 7, 11, 15];
        let tbl_b = vec![3, 7, 11, 15];
        let l = make_loop_1d(vec![
            acc_table(AccessKind::Write, tbl_a, 0, 0, 4),
            acc_table(AccessKind::Read,  tbl_b, 0, 0, 4),
        ]);
        // Same values but different Vec instances -> tables differ by ptr identity?
        // Our check is value equality (tbl_p != tbl_q), so identical values
        // are treated as same table -> Exact.
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)));
    }
    // ================================================================
    // Property fuzz: random tables and offsets. Ground truth computed
    // directly from the injectivity + offset rules.
    // ================================================================

    fn xorshift25(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    #[derive(Debug)]
    enum Expected25 { Exact, None, Unknown }

    /// The positions the loop actually assigns and that lie in the table's
    /// declared domain, in iteration order.
    ///
    /// Outside the domain `B[i]` names no cell and the access does not run, so
    /// both filters apply — the loop's own step lattice first, then the domain.
    /// This walks `lower`, `lower + step`, ... directly rather than reproducing
    /// `reached_cells`' snap-and-count arithmetic, so the two are independent
    /// implementations of the same set and can be cross-checked.
    fn executed_positions(iv: &IV, lo: i64, hi: i64) -> Vec<i64> {
        let mut out = Vec::new();
        if iv.step <= 0 || iv.lower >= iv.upper {
            return out;
        }
        let mut p = iv.lower;
        while p < iv.upper {
            if p >= lo && p < hi {
                out.push(p);
            }
            p += iv.step;
        }
        out
    }

    /// The ground truth for `A[B[i] + op]` against `A[B[j] + oq]`, walking the
    /// iteration pairs the loop performs.
    ///
    /// Distinct offsets do *not* entail absence: the two location sets overlap
    /// exactly when two cells differ by `op - oq`. Absence is also not
    /// establishable while the table is non-injective on the domain, so that
    /// shape is reported as unknown.
    fn brute25(tbl: &[i64], iv: &IV, lo: i64, hi: i64, op: i64, oq: i64) -> Expected25 {
        // Injectivity check on [lo, hi), as the checker performs it.
        if lo < 0 || hi < lo || (hi as usize) > tbl.len() {
            return Expected25::Unknown;
        }
        let mut seen = std::collections::HashSet::new();
        for &v in &tbl[lo as usize..hi as usize] {
            if !seen.insert(v) { return Expected25::Unknown; }
        }
        let ran = executed_positions(iv, lo, hi);
        for &ci in &ran {
            for &cj in &ran {
                if tbl[ci as usize] + op == tbl[cj as usize] + oq {
                    return Expected25::Exact;
                }
            }
        }
        Expected25::None
    }

    #[test]
    fn fuzz_m2a5_property() {
        let mut state: u64 = 0x25a_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut witnessed = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..4000 {
            // The table's declared domain.
            let dom_lo = (xorshift25(&mut state) % 4) as i64;
            let dom_hi = dom_lo + (xorshift25(&mut state) % 6) as i64 + 1;
            // Random table — sometimes injective, sometimes not.
            let tbl: Vec<i64> = if xorshift25(&mut state) % 2 == 0 {
                (0..dom_hi).map(|_| (xorshift25(&mut state) % 100) as i64).collect()
            } else {
                (0..dom_hi).map(|_| (xorshift25(&mut state) % 5) as i64).collect()
            };
            let off_p = (xorshift25(&mut state) % 7) as i64 - 3;
            let off_q = (xorshift25(&mut state) % 7) as i64 - 3;

            // The loop's own lattice, deliberately unaligned with the domain: it
            // may start below or inside it, stop before or past it, and step by
            // anything in 1..=4. A collision that shows up only on a position the
            // step skips is exactly the bug this shape exists to catch.
            let iv = IV::new(
                "i",
                (xorshift25(&mut state) % 6) as i64 - 1,
                (xorshift25(&mut state) % 8) as i64 + 3,
                (xorshift25(&mut state) % 4) as i64 + 1,
            );

            let l = Loop {
                ivs: vec![iv.clone()],
                scalars: vec![],
                stmts: None,
                ..make_loop_1d(vec![
                    acc_table(AccessKind::Write, tbl.clone(), off_p, dom_lo, dom_hi),
                    acc_table(AccessKind::Read,  tbl.clone(), off_q, dom_lo, dom_hi),
                ])
                };

            let expected = brute25(&tbl, &iv, dom_lo, dom_hi, off_p, off_q);
            let (d, cert) = decide(&l, 0, 1);
            tested += 1;

            let ok = match (&expected, &d) {
                (Expected25::Unknown, Decision::Unknown(_)) => true,
                (Expected25::Unknown, _) => false,
                (Expected25::None, Decision::None) => true,
                (Expected25::None, Decision::Unknown(_)) => { conservative += 1; true }
                (Expected25::None, _) => false,
                (Expected25::Exact, Decision::Exact(_)) => true,
                (Expected25::Exact, Decision::Unknown(_)) => { conservative += 1; true }
                (Expected25::Exact, _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "tbl={:?} iv=[{},{}) step={} dom=[{}, {}) op={} oq={} expected={:?} got={:?}",
                        tbl, iv.lower, iv.upper, iv.step, dom_lo, dom_hi, off_p, off_q, expected, d
                    ));
                }
                continue;
            }

            // A positive verdict must be witnessed by two iterations the loop
            // really performs, and at those two iterations the locations must
            // genuinely agree. This is the assertion the phantom-iteration bug
            // violated: it certified a collision at positions the step never
            // reaches.
            if let (Expected25::Exact, Decision::Exact(_)) = (&expected, &d) {
                let c = cert.as_ref().expect("an Exact verdict must carry a certificate");
                let (wi, wj) = match &c.witness {
                    Witness::IntSolution { i, j } => (i[0], j[0]),
                    other => panic!("M2a.5 Exact used a non-IntSolution witness: {:?}", other),
                };
                let ran = executed_positions(&iv, dom_lo, dom_hi);
                let bad = if !(wi >= dom_lo && wi < dom_hi && wj >= dom_lo && wj < dom_hi) {
                    Some("witness outside the table domain")
                } else if !ran.contains(&wi) || !ran.contains(&wj) {
                    Some("witness on an iteration the loop never runs")
                } else if tbl[wi as usize] + off_p != tbl[wj as usize] + off_q {
                    Some("witness locations do not collide")
                } else if !matches!(&c.claim, Claim::Exact(v) if v == &vec![wj - wi]) {
                    Some("claim does not match the witness distance")
                } else {
                    None
                };
                match bad {
                    None => witnessed += 1,
                    Some(why) => {
                        unsound += 1;
                        if examples.len() < 10 {
                            examples.push(format!(
                                "witness i={} j={} {}: tbl={:?} iv=[{},{}) step={} dom=[{}, {}) op={} oq={}",
                                wi, wj, why, tbl, iv.lower, iv.upper, iv.step,
                                dom_lo, dom_hi, off_p, off_q
                            ));
                        }
                    }
                }
            }
        }

        println!("M2a.5 fuzz: tested={} unsound={} conservative={} witnessed={}",
                 tested, unsound, conservative, witnessed);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M2a.5 fuzz found {} unsound", unsound);
        // The shape has to be exercised, not merely tolerated.
        assert!(witnessed > 100,
                "fuzz produced too few positive witnesses ({}); the Exact path is untested",
                witnessed);
    }

}
