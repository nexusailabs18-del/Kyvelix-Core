//! M2a — injectivity for `A[B[i]]`.
//!
//! Scope (v1): affine indirection only.
//!
//!   B[i] = c * i + d  with c != 0        → injective, flatten
//!   B[i] = d (c == 0)                    → NOT injective, Unknown
//!   B[i] = a literal table               → Unknown (M2a.5, needs checker support)
//!   mixed A[i] vs A[B[i]]                → Unknown (M5 territory)
//!
//! Flattening (Convention A — see `Access.index` doc comment in ir/mod.rs):
//!
//!   flat.index[k]  = access.index[k] + indirect.index[k]
//!   flat.offset    = access.offset   + indirect.offset
//!
//! Example: `A[B[i] + 2]` with `B[i] = 3i + 1`
//!   access.index = [0], access.offset = 2
//!   indirect.index = [3], indirect.offset = 1
//!   flat.index = [3], flat.offset = 3   →  A[3i + 3]
//!
//! Certificates are emitted against the FLATTENED loop (level = "M2a"),
//! exactly as M2g emits against the normalized loop. The checker is
//! unchanged: it reads `index` and `offset` from the flattened access,
//! which is standard M0 input.
//!
//! Soundness rule: every Exact is produced by M0 on the flattened loop,
//! which the checker independently verifies. If anything is unsure,
//! return Unknown.

use super::{m0, Decision};
use crate::certificate::{Certificate, Witness, RewriteRule};
use crate::ir::*;

// ============================================================================
// Injectivity
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Proven,
    Unknown(String),
}

fn is_injective(ind: &Indirect, ivs: &[IV]) -> Verdict {
    // v1: tables are out of scope.
    if ind.table.is_some() {
        return Verdict::Unknown("M2a: table indirection out of scope (v1)".into());
    }
    // Arity must match the loop depth.
    if ind.index.len() != ivs.len() {
        return Verdict::Unknown(format!(
            "M2a: indirect arity {} != depth {}",
            ind.index.len(),
            ivs.len()
        ));
    }
    // Non-empty domain.
    if ind.domain_lo >= ind.domain_hi {
        return Verdict::Unknown("M2a: empty indirect domain".into());
    }
    // Any non-zero coefficient ⇒ c * i + d is injective on all of Z,
    // hence on any non-empty subdomain.
    let all_zero = ind.index.iter().all(|&c| c == 0);
    if all_zero {
        return Verdict::Unknown("M2a: constant indirection is not injective".into());
    }
    Verdict::Proven
}

// ============================================================================
// Flattening
// ============================================================================

/// Rewrite every access that has an `indirect` field into a flat affine
/// access. Returns `Some(flat_loop)` only if **every** indirect in the
/// loop was provably injective and had a supported form. Returns `None`
/// if any indirect fails — M2a then answers Unknown.
pub fn flatten(loop_: &Loop) -> Option<Loop> {
    let mut changed = false;
    let mut body = Vec::with_capacity(loop_.body.len());

    for a in &loop_.body {
        match &a.indirect {
            None => body.push(a.clone()),
            Some(ind) => {
                match is_injective(ind, &loop_.ivs) {
                    Verdict::Proven => {
                        if ind.index.len() != a.index.len() {
                            return None;
                        }
                        let mut a2 = a.clone();
                        for k in 0..a2.index.len() {
                            a2.index[k] = a2.index[k].checked_add(ind.index[k])?;
                        }
                        let outer_const = a2.offset.as_const()?;
                        let new_const = outer_const.checked_add(ind.offset)?;
                        a2.offset = OffsetValue::Const(new_const);
                        a2.indirect = None;
                        body.push(a2);
                        changed = true;
                    }
                    Verdict::Unknown(_) => return None,
                }
            }
        }
    }

    if !changed {
        return None;
    }
    Some(Loop {
        depth: loop_.depth,
        ivs: loop_.ivs.clone(),
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,})
}

// ============================================================================
// Public API
// ============================================================================

/// M2a entry point — same signature as `m0::decide`.
pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    // If no access has an indirect, M2a has nothing to do.
    if loop_.body.iter().all(|a| a.indirect.is_none()) {
        return (Decision::Unknown("M2a: no indirect access".into()), None);
    }
    // Mixed direct/indirect (one access with indirect, one without) is
    // out of scope — it needs M5-style reasoning about the base pointers.
    let ap_has = loop_.body[p].indirect.is_some();
    let aq_has = loop_.body[q].indirect.is_some();
    if ap_has != aq_has {
        return (
            Decision::Unknown("M2a: mixed direct/indirect access".into()),
            None,
        );
    }

    match flatten(loop_) {
        Some(flat) => {
            let (d, c) = m0::decide(&flat, p, q);
            (d, c.map(|mut cert| {
                cert.level = "M2a".to_string();
                cert.loop_hash = loop_.hash();
                cert.witness = Witness::Rewrite {
                    rule: RewriteRule::M2aFlatten,
                    original: Box::new(loop_.clone()),
                    rewritten: Box::new(flat),
                    inner: Box::new(cert.witness),
                };
                cert
            }))
        }
        None => (
            Decision::Unknown("M2a: no injective affine indirection".into()),
            None,
        ),
    }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(name: &str) -> IV {
        IV::new(name, 0, 64, 1)
    }

    fn acc(kind: AccessKind, ind: Option<Indirect>) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![0],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: ind,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn make_loop(body: Vec<Access>) -> Loop {
        Loop { depth: 1, ivs: vec![iv("i")], body, recursion: None, chase: None, assertions: vec![] , scalars: vec![], stmts: None,}
    }

    fn ind(c: i64, d: i64) -> Indirect {
        Indirect {
            base: "B".into(),
            index: vec![c],
            offset: d,
            table: None,
            domain_lo: 0,
            domain_hi: 64,
        }
    }

    #[test]
    fn injective_affine_c_nonzero() {
        let ivs = vec![iv("i")];
        assert_eq!(is_injective(&ind(3, 1), &ivs), Verdict::Proven);
        assert_eq!(is_injective(&ind(-2, 0), &ivs), Verdict::Proven);
        assert_eq!(is_injective(&ind(1, 99), &ivs), Verdict::Proven);
    }

    #[test]
    fn constant_indirection_not_injective() {
        let ivs = vec![iv("i")];
        match is_injective(&ind(0, 5), &ivs) {
            Verdict::Unknown(msg) => assert!(msg.contains("constant")),
            v => panic!("expected Unknown, got {:?}", v),
        }
    }

    #[test]
    fn empty_domain_not_injective() {
        let ivs = vec![iv("i")];
        let mut i = ind(3, 0);
        i.domain_lo = 10;
        i.domain_hi = 10;
        assert!(matches!(is_injective(&i, &ivs), Verdict::Unknown(_)));
    }

    #[test]
    fn table_out_of_scope_v1() {
        let ivs = vec![iv("i")];
        let mut i = ind(3, 0);
        i.table = Some(vec![1, 2, 3]);
        match is_injective(&i, &ivs) {
            Verdict::Unknown(msg) => assert!(msg.contains("table")),
            v => panic!("expected Unknown, got {:?}", v),
        }
    }

    #[test]
    fn flatten_a001() {
        // A[B[i] + 1] with B[i] = 3i + 1  →  A[3i + 2]
        let body = vec![
            acc(AccessKind::Write, None),
            {
                let mut a = acc(AccessKind::Read, Some(ind(3, 1)));
                a.offset = OffsetValue::Const(1);
                a
            },
        ];
        let l = make_loop(body);
        let f = flatten(&l).expect("should flatten");
        assert_eq!(f.body[0].index, vec![0]);
        assert_eq!(f.body[1].index, vec![3]);
        assert_eq!(f.body[1].offset, OffsetValue::Const(2));
        assert!(f.body[1].indirect.is_none());
    }

    #[test]
    fn flatten_identity_a003() {
        // A[B[i]] with B[i] = 2i  →  A[2i]
        let body = vec![
            acc(AccessKind::Write, Some(ind(2, 0))),
            acc(AccessKind::Read,  Some(ind(2, 0))),
        ];
        let l = make_loop(body);
        let f = flatten(&l).expect("should flatten");
        assert_eq!(f.body[0].index, vec![2]);
        assert_eq!(f.body[1].index, vec![2]);
        assert_eq!(f.body[0].offset, OffsetValue::Const(0));
        assert_eq!(f.body[1].offset, OffsetValue::Const(0));
    }

    #[test]
    fn flatten_rejects_constant() {
        let body = vec![
            acc(AccessKind::Write, Some(ind(0, 0))),
            acc(AccessKind::Read,  Some(ind(0, 0))),
        ];
        let l = make_loop(body);
        assert!(flatten(&l).is_none());
    }

    #[test]
    fn flatten_rejects_table() {
        let mut i = ind(3, 0);
        i.table = Some(vec![1, 2, 3]);
        let body = vec![
            acc(AccessKind::Write, Some(i.clone())),
            acc(AccessKind::Read,  Some(i)),
        ];
        let l = make_loop(body);
        assert!(flatten(&l).is_none());
    }

    #[test]
    fn flatten_rejects_empty_domain() {
        let mut i = ind(3, 0);
        i.domain_lo = 5;
        i.domain_hi = 5;
        let body = vec![
            acc(AccessKind::Write, Some(i.clone())),
            acc(AccessKind::Read,  Some(i)),
        ];
        let l = make_loop(body);
        assert!(flatten(&l).is_none());
    }

    #[test]
    fn flatten_no_indirect_returns_none() {
        let body = vec![
            acc(AccessKind::Write, None),
            acc(AccessKind::Read,  None),
        ];
        let l = make_loop(body);
        assert!(flatten(&l).is_none());
    }

    #[test]
    fn flatten_mixed_returns_none_from_decide() {
        // one with indirect, one without → decide must return Unknown
        let body = vec![
            acc(AccessKind::Write, None),
            acc(AccessKind::Read,  Some(ind(3, 0))),
        ];
        let l = make_loop(body);
        let (d, _) = decide(&l, 0, 1);
        match d {
            Decision::Unknown(msg) => assert!(msg.contains("mixed")),
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn flatten_no_indirect_returns_unknown_from_decide() {
        let body = vec![
            acc(AccessKind::Write, None),
            acc(AccessKind::Read,  None),
        ];
        let l = make_loop(body);
        let (d, _) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Unknown(_)));
    }
    // ================================================================
    // Property fuzz: random Indirect flattening. Verifies that the
    // flattened access produces the same location as the raw access
    // for every IV in the range.
    // ================================================================

    fn xorshift2a(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    #[test]
    fn fuzz_m2a_flatten_property() {
        let mut state: u64 = 0x2a_12345678;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Random c != 0 (M2a requires c != 0 for injectivity).
            let c = match xorshift2a(&mut state) % 6 { 0 => 2, 1 => 1, 2 => 3, 3 => -1, 4 => -2, _ => 4 };
            let d = (xorshift2a(&mut state) % 9) as i64 - 4;
            let outer_off = (xorshift2a(&mut state) % 9) as i64 - 4;
            let dom_lo = 0i64;
            let dom_hi = 8i64;

            let indirect = Indirect {
                base: "B".into(),
                index: vec![c],
                offset: d,
                table: None,
                domain_lo: dom_lo,
                domain_hi: dom_hi,
            };
            let mut a = acc(AccessKind::Write, Some(indirect.clone()));
            a.offset = OffsetValue::Const(outer_off);
            let a2 = acc(AccessKind::Read, Some(indirect.clone()));

            let l = make_loop(vec![a.clone(), a2.clone()]);

            // Expected: flatten should produce index = c, offset = outer_off + d.
            let flat = flatten(&l);
            tested += 1;
            match flat {
                Some(fl) => {
                    // Check that flattened access at every IV matches the raw form:
                    //   raw: A[c*i + d + outer_off]
                    //   flat: A[c' * i + off']
                    let fa = &fl.body[0];
                    let fc = fa.index[0];
                    let fo = fa.offset.as_const().expect("test ensures constant offset");
                    // Verify over the IV range: fc*i + fo == c*i + d + outer_off for all i.
                    let iv_lo = l.ivs[0].lower;
                    let iv_hi = l.ivs[0].upper;
                    let mut ok = true;
                    for i in iv_lo..iv_hi {
                        let raw = c * i + d + outer_off;
                        let norm = fc * i + fo;
                        if raw != norm {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        unsound += 1;
                        if examples.len() < 10 {
                            examples.push(format!(
                                "c={} d={} outer_off={} fc={} fo={}",
                                c, d, outer_off, fc, fo
                            ));
                        }
                    }
                }
                None => {
                    // M2a refused — that's fine, not unsound.
                }
            }
        }

        println!("M2a fuzz: tested={} unsound={}", tested, unsound);
        for ex in &examples { println!("  UNSOUND {}", ex); }
        assert_eq!(unsound, 0, "M2a flatten found {} unsound", unsound);
    }

}
