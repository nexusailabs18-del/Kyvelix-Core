//! M5 — bounded pointer chase.
//!
//! Scope (v1): a loop that walks a chase variable `p` through a literal
//! `next` table a bounded number of steps:
//!
//! ```text
//! for (i = 0; i < N; i++) { p = next[p]; A[p] = A[p+1]; }
//! ```
//!
//! with `next = [1, 2, 3, 4]`, `init = 0`, `max_steps = 4`.
//!
//! M5 computes the chain `p0 = init, p_{k+1} = table[p_k]` up to
//! `max_steps` steps, refuses if any step is out of bounds, then expands
//! every access on `chase.accessed_base` into one access per step, with
//! `index = [p_k]`. The expanded loop has no `chase` field and is
//! standard M0 input.
//!
//! Refuses (returns Unknown):
//!   - max_steps == 0
//!   - any step out of `table` bounds
//!   - accesses on `accessed_base` have placeholder index 0 and there
//!     are two or more such accesses on different bases (mixed chase
//!     and non-chase accesses are allowed as long as non-chase accesses
//!     do not use the chase variable)
//!
//! Certificates are emitted by M0 on the expanded loop, relabelled
//! "M5". The checker is unchanged.

use super::Decision;
use crate::certificate::{Certificate, Claim, Witness, RewriteRule};
use crate::ir::*;
use crate::ir::stmt::MemoryContext;

/// Compute the chase chain from `chase.init`, following `chase.table`
/// or `chase.table_expr` up to `chase.max_steps` steps.
/// Returns None if any step is out of bounds or if `max_steps == 0`.
/// If `table_expr` is `Some`, it is evaluated with the provided `MemoryContext`
/// (typically built from the loop's scalars).
fn chase_chain(chase: &PointerChase, memory: Option<MemoryContext>) -> Option<Vec<i64>> {
    // If table_expr is used but no memory provided, create an empty one.
    // This allows table_expr evaluation to proceed (though Load will return None
    // without actual array data, which is the correct conservative behavior).
    let memory = if chase.table_expr.is_some() && memory.is_none() {
        Some(MemoryContext::new())
    } else {
        memory
    };
    let memory_ref = memory.as_ref();
    
    if chase.max_steps == 0 {
        return None;
    }
    let mut chain = Vec::with_capacity(chase.max_steps);
    let mut p = chase.init;
    for step in 0..chase.max_steps {
        // Record the current position (this is the p used by the body
        // at chase step `step`).
        chain.push(p);
        // Only advance if another step follows.
        if step + 1 < chase.max_steps {
            let next_p = if let Some(expr) = &chase.table_expr {
                // Evaluate the table expression (e.g., Load) with current p as index
                // The index expression should use the chase variable value `p`
                let ivs = &[]; // No IVs needed for chase table eval
                // Create scalars with the chase variable value at the base index
                // The chase variable is typically scalar 0
                let mut scalars = Vec::new();
                scalars.push(p); // scalar 0 = current chase value
                // Add any additional scalars from the loop context
                // For now we just use the memory context for Load evaluation
                expr.eval(ivs, &scalars, memory_ref)?
            } else {
                // Fall back to literal table
                if p < 0 || (p as usize) >= chase.table.len() {
                    return None;
                }
                chase.table[p as usize]
            };
            p = next_p;
        }
    }
    Some(chain)
}

/// Expand every access on `chase.accessed_base` into one access per
/// chase step.
///
/// Semantics: the original loop's `for (i = 0; i < N; i++)` iterates N
/// times, and after `i` steps of the chase the variable `p` holds
/// `chain[i]`. So the write `A[p]` on iteration `i` accesses cell
/// `chain[i]`, and the read `A[p+1]` accesses cell `chain[i] + 1`.
///
/// The expanded loop models this by replacing the IV with the **step
/// index** `k`, and giving each expanded access a constant location:
///
///   - step-k write:  index = [0], offset = chain[k] + original_offset
///   - step-k read:   index = [0], offset = chain[k] + original_offset
///
/// so `loc(k) = chain[k] + offset` — a constant with respect to the
/// loop variable, but distinct per step. A dependence exists iff some
/// pair of steps `(k_p, k_q)` produces equal locations; the distance
/// is `k_q - k_p` (in step space, which is what "distance" means for
/// this loop).
///
/// Accesses not on `accessed_base` are left untouched; they keep the
/// original IV and the original `index`.
pub fn expand_with_memory(loop_: &Loop, memory: Option<&MemoryContext>) -> Option<Loop> {
    let chase = loop_.chase.as_ref()?;

    // Pass memory reference to chase_chain. If table_expr is used and no memory
    // is provided, chase_chain will create an empty one internally.
    let chain = chase_chain(chase, memory.cloned())?;

    let mut body = Vec::with_capacity(loop_.body.len() * chain.len());
    for a in &loop_.body {
        if a.base == chase.accessed_base {
            for &p in &chain {
                let mut a2 = a.clone();
                if a2.index.len() != 1 {
                    return None;
                }
                // The chase value is a constant array index, not an IV
                // coefficient. Set index = [0] (no IV dependence) and
                // fold the chain value into the offset.
                let orig_off = a2.offset.as_const()?;
                a2.index = vec![0];
                a2.offset = crate::ir::OffsetValue::Const(p.checked_add(orig_off)?);
                body.push(a2);
            }
        } else {
            body.push(a.clone());
        }
    }

    Some(Loop {
        depth: 1,
        ivs: vec![IV::new("k", 0, chain.len() as i64, 1)],
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
        scalars: vec![],
        stmts: None,
    })
}

/// Phase 5 §5.1.5 — symbolic pointer chase (Wolfe Ch. 7.5 extension).
///
/// The spec's Step 1:
///
/// ```text
/// if let Some(iv) = induct.ivs.find(|v| v.var == chase.var):
///     Class::LinearIv { a, b } => {
///         # Chain is arithmetic: p_k = a + b·k.
///         # Fixed point is computable in closed form.
///         return closed_form_fixed_point(a, b, bound)
///     }
/// ```
///
/// A chase you can write *in this IR* without a table is the one whose `next`
/// relation is arithmetic — `next[x] = x + b` — because then "the chase
/// variable advances by `b` each step" is exactly an induction variable, and
/// the access `A[p]` is already the affine `A[b·k + a]`. So the symbolically
/// decidable subset is precisely the case where the loop names the chase
/// variable as one of its own IVs: the closed form is the IV, no table lookup
/// remains, and (as the spec's own prose says) "no enumeration is needed — the
/// fixed point is computable directly."
///
/// Returns `Some((decision, cert))` when the chase is symbolically affine, by
/// delegating to the ordinary affine decision on a chase-less copy of the loop
/// — so `m0-check` verifies an ordinary `Exact`/`None` claim against an
/// ordinary affine loop, with the chase metadata removed and its removal
/// recorded in the loop hash the certificate carries. Returns `None` for a
/// genuine literal-table chase (the fallback the spec keeps for that case).
pub fn decide_symbolic_chase(
    loop_: &Loop,
    p: usize,
    q: usize,
) -> Option<(Decision, Option<Certificate>)> {
    let chase = loop_.chase.as_ref()?;
    if loop_.depth != 1 || chase.max_steps == 0 {
        return None;
    }
    // The chase variable must be an induction variable of the loop: that is
    // what makes the recurrence arithmetic and the closed form the IV itself.
    if !loop_.ivs.iter().any(|iv| iv.name == chase.var) {
        return None;
    }

    // Strip the chase metadata; the accesses are already affine in the IV.
    let mut affine = loop_.clone();
    affine.chase = None;
    // A symbolic chase loop has no table to expand, so its `body` accesses are
    // the real affine accesses and the pair indices carry over unchanged.
    let (d, cert) = super::decide(&affine, p, q);
    // Re-bind the certificate to the original file's hash: `m0-check` hashes
    // the loop on disk (which carries the chase field) and the `chain` here is
    // the affine loop the witness was computed for. `Witness::Rewrite` is
    // exactly this discipline.
    let cert = cert.map(|c| crate::certificate::Certificate {
        level: "M5".to_string(),
        loop_hash: loop_.hash(),
        witness: if matches!(c.witness, crate::certificate::Witness::Rewrite { rule: crate::certificate::RewriteRule::M5ChaseExpand, .. }) {
            c.witness
        } else {
            crate::certificate::Witness::Rewrite {
                rule: crate::certificate::RewriteRule::M5ChaseExpand,
                original: Box::new(loop_.clone()),
                rewritten: Box::new(affine.clone()),
                inner: Box::new(c.witness),
            }
        },
        ..c
    });
    Some((d, cert))
}

/// Decide with an optional MemoryContext for evaluating `table_expr`.
/// This enables the fast chase path for symbolic (non-literal) chase tables.
pub fn decide_with_memory(
    loop_: &Loop,
    p: usize,
    q: usize,
    memory: Option<MemoryContext>,
) -> (Decision, Option<Certificate>) {
    let chase = match &loop_.chase {
        Some(c) => c,
        None => return (Decision::Unknown("M5: no chase metadata".into()), None),
    };

    // Phase 5 §5.1.5: if the chase variable's recurrence is an induction
    // variable the Phase 4 analyses can prove linear, the chain is arithmetic
    // and its fixed point is computable in closed form — no table, no
    // enumeration. This is tried first; the literal-table path below is the
    // fallback when the loop has no scalar CFG to read (a bare `chase` loop
    // with no `stmts`/`scalars`), which is the case the whole of M5 handled
    // before Phase 5.
    if let Some(result) = decide_symbolic_chase(loop_, p, q) {
        return result;
    }

    let expanded = match expand_with_memory(loop_, memory.as_ref()) {
        Some(l) => l,
        None => {
            return (
                Decision::Unknown(format!(
                    "M5: chase on {} is not expandable (init={}, max_steps={})",
                    chase.var, chase.init, chase.max_steps
                )),
                None,
            );
        }
    };

    // After expansion, the pair indices (p, q) may no longer be valid
    // if the accesses we care about were expanded into a different
    // number of copies. We rely on the caller to have selected a pair
    // whose bases are both on `accessed_base` or both not, and whose
    // expansion is deterministic. For simplicity, only support depth-1
    // loops where both accesses are on the same base.
    if loop_.depth != 1 {
        return (Decision::Unknown("M5: depth>1 unsupported".into()), None);
    }
    if p >= loop_.body.len() || q >= loop_.body.len() {
        return (Decision::Unknown("M5: pair index out of range".into()), None);
    }

    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    // The pair must be on the same base to be meaningful in the expanded
    // form. If both are on `chase.accessed_base`, expansion multiplies
    // them. If neither, M5 should not have been routed to.
    if ap.base != aq.base {
        // Different arrays cannot have a data dependence.
        return (Decision::None, None);
    }
    if ap.base != chase.accessed_base {
        return (
            Decision::Unknown("M5: neither access is on the chase base".into()),
            None,
        );
    }

    // Recompute the chain (the expand() call above already validated it).
    let chain = match chase_chain(chase, memory) {
        Some(c) => c,
        None => {
            return (
                Decision::Unknown("M5: chase chain not computable".into()),
                None,
            );
        }
    };

    // The expanded loop has K write copies (indices 0..K) and K read
    // copies (indices K..2K). Each write copy k has location
    // chain[k] + off_p; each read copy k has location chain[k] + off_q.
    //
    // A dependence exists iff some (k_p, k_q) with
    //     chain[k_p] + off_p == chain[k_q] + off_q.
    //
    // M0's Stage 5b enumerates over the IV range, so calling
    // m0::decide on every (write_copy, read_copy) pair would be
    // O(K^2) calls. Instead, we build the location set of the read
    // side once and walk the write side.
    let k = expanded.ivs[0].upper as usize;
    if k == 0 || k != chain.len() {
        return (Decision::Unknown("M5: inconsistent expanded loop".into()), None);
    }

    // Offsets of the expanded write copies: chain[0..K] + original off_p.
    // Offsets of the expanded read copies:  chain[0..K] + original off_q.
    //
    // We stored them as `offset = Const(p + orig_off)` during expansion,
    // so read them back directly.
    let off_p_orig = match ap.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("M5: symbolic offset in expanded access".into()), None),
    };
    let off_q_orig = match aq.offset.as_const() {
        Some(v) => v,
        None => return (Decision::Unknown("M5: symbolic offset in expanded access".into()), None),
    };

    // Build the map: read-side location -> smallest step q that produces it.
    use std::collections::HashMap;
    let mut read_by_loc: HashMap<i64, usize> = HashMap::new();
    for (q_step, &chain_q) in chain.iter().enumerate() {
        let loc_q = chain_q.saturating_add(off_q_orig);
        read_by_loc.entry(loc_q).or_insert(q_step);
    }

    // Where each original access landed in the expanded body. `expand` walks
    // `loop_.body` in order and replaces every on-base access with `chain.len()`
    // copies, so an access's first copy sits after all the copies the earlier
    // accesses contributed. Both `p` and `q` are on `accessed_base` here (the
    // guards above reject anything else), so each occupies a contiguous run of
    // `k` copies — but the runs start at `base(p)` and `base(q)`, *not* at 0 and
    // `k`. The old code assumed `p` was the body's first access, which made the
    // reverse pair `(q, p)` name a write copy against a read copy that never
    // collide, and the checker rejected it.
    let base = |idx: usize| -> usize {
        loop_.body[..idx]
            .iter()
            .map(|a| if a.base == chase.accessed_base { k } else { 1 })
            .sum()
    };
    let (base_p, base_q) = (base(p), base(q));

    // Walk the write side, looking for the first match.
    for (p_step, &chain_p) in chain.iter().enumerate() {
        let loc_p = chain_p.saturating_add(off_p_orig);
        if let Some(&q_step) = read_by_loc.get(&loc_p) {
            // Found a collision at chase steps (p_step, q_step).
            // Distance in step space: q_step - p_step.
            let d = q_step as i64 - p_step as i64;

            // The witness IVs are the chase-step indices p_step and q_step. The
            // checker's WithLoop arm computes witness_location on
            // `loop_.body[cert.pair.0]` and `loop_.body[cert.pair.1]`, so the
            // pair must be the *expanded* indices of the two copies that
            // actually collide: step p_step of access p, step q_step of access q.
            let ep = base_p + p_step;
            let eq = base_q + q_step;

            let cert = Some(Certificate {
                level: "M5".to_string(),
                loop_hash: loop_.hash(),
                pair: (ep, eq),
                claim: Claim::Exact(vec![d]),
                witness: Witness::Rewrite {
                    rule: RewriteRule::M5ChaseExpand,
                    original: Box::new(loop_.clone()),
                    rewritten: Box::new(expanded),
                    inner: Box::new(Witness::IntSolution {
                        i: vec![p_step as i64],
                        j: vec![q_step as i64],
                    }),
                },
                checker: "m0-check-v1".to_string(),
            });
            return (Decision::Exact(vec![d]), cert);
        }
    }

    // Exhaustive search found no collision: no dependence.
    (Decision::None, None)
}

/// Backward-compatible entry point that uses no MemoryContext.
/// Prefer `decide_with_memory` when the caller has array data for `table_expr`.
pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    decide_with_memory(loop_, p, q, None)
}

/// Backward-compatible entry point that uses no MemoryContext.
/// Prefer `expand_with_memory` when the caller has array data for `table_expr`.
pub fn expand(loop_: &Loop) -> Option<Loop> {
    expand_with_memory(loop_, None)
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(kind: AccessKind, base: &str, index: i64, offset: i64) -> Access {
        Access {
            kind,
            base: base.into(),
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

    fn make_chase_loop(table: Vec<i64>, init: i64, max_steps: usize, body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 64, 1)],
            body,
            recursion: None,
            chase: Some(PointerChase {
                var: "p".into(),
                table,
                table_expr: None,
                init,
                max_steps,
                accessed_base: "A".into(),
            }),
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    #[test]
    fn chain_linear() {
        let chase = PointerChase {
            var: "p".into(),
            table: vec![1, 2, 3, 4],
            table_expr: None,
            init: 0,
            max_steps: 4,
            accessed_base: "A".into(),
        };
        assert_eq!(chase_chain(&chase, None), Some(vec![0, 1, 2, 3]));
    }

    #[test]
    fn chain_cycle() {
        let chase = PointerChase {
            var: "p".into(),
            table: vec![1, 2, 0, 0],
            table_expr: None,
            init: 0,
            max_steps: 5,
            accessed_base: "A".into(),
        };
        // 0 -> 1 -> 2 -> 0 -> 1 -> 2
        assert_eq!(chase_chain(&chase, None), Some(vec![0, 1, 2, 0, 1]));
    }

    #[test]
    fn chain_out_of_bounds() {
        let chase = PointerChase {
            var: "p".into(),
            table: vec![1, 5, 3],
            table_expr: None,
            init: 0,
            max_steps: 4,
            accessed_base: "A".into(),
        };
        // Step 0: record 0, advance to 1
        // Step 1: record 1, advance to 5
        // Step 2: record 5, advance to table[5] -> out of bounds (len 3)
        assert_eq!(chase_chain(&chase, None), None);
    }

    #[test]
    fn chain_zero_steps() {
        let chase = PointerChase {
            var: "p".into(),
            table: vec![1, 2],
            table_expr: None,
            init: 0,
            max_steps: 0,
            accessed_base: "A".into(),
        };
        assert_eq!(chase_chain(&chase, None), None);
    }

    /// Phase 5 §5.1.5: a chase whose variable *is* the loop's IV is
    /// symbolically affine, so `decide_symbolic_chase` handles it (and the
    /// literal-table fallback is not reached).
    #[test]
    fn symbolic_chase_handles_an_iv_chase_variable() {
        let l = Loop {
            depth: 1,
            ivs: vec![IV::new("p", 0, 8, 1)],
            body: vec![acc(AccessKind::Write, "A", 1, 0), acc(AccessKind::Read, "A", 1, 1)],
            recursion: None,
            chase: Some(PointerChase {
                var: "p".into(),
                table: vec![],
                table_expr: None,
                init: 0,
                max_steps: 8,
                accessed_base: "A".into(),
            }),
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        };
        let handled = decide_symbolic_chase(&l, 0, 1);
        assert!(handled.is_some(), "IV chase must be handled symbolically");
        // A[p] = A[p+1] over p in 0..8: locations {0..7} vs {1..8} overlap.
        assert!(matches!(handled.unwrap().0, Decision::Exact(_)));
    }

    /// A literal-table chase (chase var is not an IV) must fall through.
    #[test]
    fn symbolic_chase_falls_through_for_a_table_chase() {
        let l = make_chase_loop(
            vec![1, 2, 3, 4],
            0,
            4,
            vec![acc(AccessKind::Write, "A", 0, 0), acc(AccessKind::Read, "A", 0, 1)],
        );
        assert!(decide_symbolic_chase(&l, 0, 1).is_none());
    }

    #[test]
    fn expand_two_accesses() {
        // A[p] = A[p+1] with 4-step chain over A
        let l = make_chase_loop(
            vec![1, 2, 3, 4],
            0,
            4,
            vec![
                acc(AccessKind::Write, "A", 0, 0),
                acc(AccessKind::Read,  "A", 0, 1),
            ],
        );
        let e = expand(&l).expect("should expand");
        // 2 accesses * 4 steps = 8 accesses in the expanded loop
        assert_eq!(e.body.len(), 8);
        assert!(e.chase.is_none());
        // First write's index is chain[0]=0
        assert_eq!(e.body[0].index, vec![0]);
        // First read's index is chain[0]=0, offset 1
        assert_eq!(e.body[4].index, vec![0]);
        assert_eq!(e.body[4].offset, OffsetValue::Const(1));
    }

    #[test]
    fn decide_exact_same_shape() {
        // A[p] = A[p]; both on chase base A, distance 0
        let l = make_chase_loop(
            vec![1, 2, 3, 4],
            0,
            4,
            vec![
                acc(AccessKind::Write, "A", 0, 0),
                acc(AccessKind::Read,  "A", 0, 0),
            ],
        );
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M5");
    }

    #[test]
    fn decide_exact_offset_1() {
        // A[p] = A[p+1]; distance -1 on each pair
        let l = make_chase_loop(
            vec![1, 2, 3, 4],
            0,
            4,
            vec![
                acc(AccessKind::Write, "A", 0, 0),
                acc(AccessKind::Read,  "A", 0, 1),
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![-1]));
    }

    #[test]
    fn decide_unknown_no_chase() {
        let l = Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 64, 1)],
            body: vec![
                acc(AccessKind::Write, "A", 1, 0),
                acc(AccessKind::Read,  "A", 1, 1),
            ],
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,};
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn decide_unknown_out_of_bounds_chain() {
        // max_steps 4 forces the chain to advance FROM position 5,
        // which is out of table bounds.
        let l = make_chase_loop(
            vec![1, 5, 3],
            0,
            4,
            vec![
                acc(AccessKind::Write, "A", 0, 0),
                acc(AccessKind::Read,  "A", 0, 1),
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }

    #[test]
    fn decide_none_different_bases() {
        let l = make_chase_loop(
            vec![1, 2, 3, 4],
            0,
            4,
            vec![
                acc(AccessKind::Write, "A", 0, 0),
                acc(AccessKind::Read,  "B", 0, 0),
            ],
        );
        let (d, _) = decide(&l, 0, 1);
        // Different arrays cannot have a data dependence.
        assert!(matches!(d, Decision::None));
    }
    // ================================================================
    // Property fuzz: random chase tables, chains, offsets. Compares
    // decide() against brute-force over the chain's diagonal steps.
    // ================================================================

    fn xorshift5(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    #[derive(Debug)]
    enum Expected5 { Exact, None, Unknown }

    fn brute5(table: &[i64], init: i64, max_steps: usize, off_p: i64, off_q: i64) -> Expected5 {
        if max_steps == 0 { return Expected5::Unknown; }
        let mut chain = Vec::with_capacity(max_steps);
        let mut p = init;
        for step in 0..max_steps {
            chain.push(p);
            if step + 1 < max_steps {
                if p < 0 || (p as usize) >= table.len() { return Expected5::Unknown; }
                p = table[p as usize];
            }
        }
        // Write: {chain[k] + off_p}; read: {chain[k] + off_q}.
        let writes: std::collections::HashSet<i64> = chain.iter().map(|&c| c + off_p).collect();
        for &c in &chain {
            if writes.contains(&(c + off_q)) { return Expected5::Exact; }
        }
        Expected5::None
    }

    #[test]
    fn fuzz_m5_property() {
        let mut state: u64 = 0x5_55555555;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            let table_len = (xorshift5(&mut state) % 6) as usize + 1;
            let table: Vec<i64> = (0..table_len)
                .map(|_| (xorshift5(&mut state) % (table_len as u64 + 3)) as i64 - 2)
                .collect();
            let init = (xorshift5(&mut state) % (table_len as u64 + 3)) as i64 - 2;
            let max_steps = (xorshift5(&mut state) % 6) as usize;
            let off_p = (xorshift5(&mut state) % 7) as i64 - 3;
            let off_q = (xorshift5(&mut state) % 7) as i64 - 3;

            let l = make_chase_loop(table.clone(), init, max_steps, vec![
                acc(AccessKind::Write, "A", 0, off_p),
                acc(AccessKind::Read,  "A", 0, off_q),
            ]);

            let expected = brute5(&table, init, max_steps, off_p, off_q);
            let (d, _) = decide(&l, 0, 1);
            tested += 1;

            let ok = match (&expected, &d) {
                (Expected5::Unknown, Decision::Unknown(_)) => true,
                (Expected5::Unknown, _) => false,
                (Expected5::None, Decision::None) => true,
                (Expected5::None, Decision::Unknown(_)) => { conservative += 1; true }
                (Expected5::None, _) => false,
                (Expected5::Exact, Decision::Exact(_)) => true,
                (Expected5::Exact, Decision::Unknown(_)) => { conservative += 1; true }
                (Expected5::Exact, _) => false,
            };
            if !ok {
                unsound += 1;
                if examples.len() < 10 {
                    examples.push(format!(
                        "table={:?} init={} max_steps={} op={} oq={} expected={:?} got={:?}",
                        table, init, max_steps, off_p, off_q, expected, d
                    ));
                }
            }
        }

        println!("M5 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M5 fuzz found {} unsound", unsound);
    }

}
