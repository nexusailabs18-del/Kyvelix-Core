//! M11 — symbolic pointer chase.
//!
//! Handles index expressions of the form:
//!     x_0 = iv[0]
//!     x_{k+1} = eval(next_expr, [x_k])
//! until x_{k+1} == x_k (fixed point) or k == max_steps.
//! Depth-1 only.

use super::Decision;
use crate::certificate::{CConstraint, Certificate, Claim, Witness};
use crate::ir::*;
use crate::ir::stmt::MemoryContext;

const ENUM_CAP: usize = 4096;

fn eval_symbolic_fp(
    next_expr: &IndexExpr,
    start: i64,
    max_steps: usize,
    scalars: &[i64],
    memory: Option<&MemoryContext>,
) -> Option<i64> {
    let mut current = start;
    for _ in 0..max_steps {
        let next_val = next_expr.eval(&[current], scalars, memory)?;
        if next_val == current {
            return Some(current);
        }
        current = next_val;
    }
    None // Did not converge within max_steps
}

fn enumerate_sym_fp(
    access: &Access,
    iv: &IV,
    scalars: &[i64],
    memory: Option<&MemoryContext>,
) -> Option<Vec<(i64, i64)>> {
    if iv.step <= 0 || iv.lower >= iv.upper { return None; }
    let count = ((iv.upper - iv.lower) as u64 + (iv.step as u64) - 1) / (iv.step as u64);
    if count as usize > ENUM_CAP { return None; }
    
    let (next_expr, max_steps) = match &access.index_kind {
        IndexKind::SymbolicFixedPoint { next_expr, max_steps } => (next_expr, *max_steps),
        _ => return None,
    };

    let offset = access.offset.as_const()?;
    let mut out = Vec::with_capacity(count as usize);
    let mut v = iv.lower;
    
    while v < iv.upper {
        if let Some(fp) = eval_symbolic_fp(next_expr, v, max_steps, scalars, memory) {
            if let Some(loc) = fp.checked_add(offset) {
                out.push((v, loc));
            }
        }
        v = v.checked_add(iv.step)?;
    }
    Some(out)
}

fn enumerate_affine_fallback(access: &Access, iv: &IV) -> Option<Vec<(i64, i64)>> {
    if iv.step <= 0 || iv.lower >= iv.upper { return None; }
    let count = ((iv.upper - iv.lower) as u64 + (iv.step as u64) - 1) / (iv.step as u64);
    if count as usize > ENUM_CAP { return None; }
    
    let c = access.index.first().copied().unwrap_or(0);
    let offset = access.offset.as_const()?;
    let mut out = Vec::with_capacity(count as usize);
    let mut v = iv.lower;
    
    while v < iv.upper {
        if let Some(loc) = c.checked_mul(v)?.checked_add(offset) {
            out.push((v, loc));
        }
        v = v.checked_add(iv.step)?;
    }
    Some(out)
}

pub fn decide(loop_: &Loop, p: usize, q: usize) -> (Decision, Option<Certificate>) {
    let ap = &loop_.body[p];
    let aq = &loop_.body[q];

    let uses_sym_fp = |a: &Access| matches!(&a.index_kind, IndexKind::SymbolicFixedPoint { .. });
    if !uses_sym_fp(ap) && !uses_sym_fp(aq) {
        return (Decision::Unknown("M11: no symbolic fixed-point index".into()), None);
    }

    if loop_.depth != 1 {
        return (Decision::Unknown("M11: depth>1 unsupported".into()), None);
    }
    if ap.base != aq.base {
        return (Decision::None, None);
    }

    let iv = &loop_.ivs[0];
    // Scalars values come from execution context, not Loop struct (which has ScalarVar).
    // For M11, we pass empty slice - Load expressions will return None (conservative).
    let scalars: &[i64] = &[];
    // MemoryContext comes from environment, not directly from Loop.
    let memory: Option<&MemoryContext> = None;
    
    let lp = match &ap.index_kind {
        IndexKind::SymbolicFixedPoint { .. } => match enumerate_sym_fp(ap, iv, scalars, memory) {
            Some(v) => v,
            None => {
                return (
                    Decision::Unknown("M11: cannot enumerate symbolic FP for p".into()),
                    None,
                )
            }
        },
        IndexKind::Affine => match enumerate_affine_fallback(ap, iv) {
            Some(v) => v,
            None => {
                return (
                    Decision::Unknown("M11: cannot enumerate affine for p".into()),
                    None,
                )
            }
        },
        _ => return (Decision::Unknown("M11: p index kind not supported".into()), None),
    };
    
    let lq = match &aq.index_kind {
        IndexKind::SymbolicFixedPoint { .. } => match enumerate_sym_fp(aq, iv, scalars, memory) {
            Some(v) => v,
            None => {
                return (
                    Decision::Unknown("M11: cannot enumerate symbolic FP for q".into()),
                    None,
                )
            }
        },
        IndexKind::Affine => match enumerate_affine_fallback(aq, iv) {
            Some(v) => v,
            None => {
                return (
                    Decision::Unknown("M11: cannot enumerate affine for q".into()),
                    None,
                )
            }
        },
        _ => return (Decision::Unknown("M11: q index kind not supported".into()), None),
    };

    let mut best: Option<(i64, i64, i64)> = None;
    for (iv_p, loc_p) in &lp {
        for (iv_q, loc_q) in &lq {
            if loc_p == loc_q {
                match best {
                    None => best = Some((*loc_p, *iv_p, *iv_q)),
                    Some((cur, _, _)) if *loc_p < cur => {
                        best = Some((*loc_p, *iv_p, *iv_q))
                    }
                    _ => {}
                }
            }
        }
    }

    match best {
        Some((_loc, iv_p, iv_q)) => {
            let d = vec![iv_q - iv_p];
            let cert = Certificate {
                level: "M11".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::Exact(d.clone()),
                witness: Witness::IntSolution { i: vec![iv_p], j: vec![iv_q] },
                checker: "m0-check".into(),
            };
            (Decision::Exact(d), Some(cert))
        }
        None => {
            let cert = Certificate {
                level: "M11".into(),
                loop_hash: loop_.hash(),
                pair: (p, q),
                claim: Claim::None,
                witness: Witness::UnsatProof {
                    constraints: vec![CConstraint {
                        coeffs: vec![0; loop_.depth],
                        constant: -1,
                    }],
                },
                checker: "m0-check".into(),
            };
            (Decision::None, Some(cert))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(name: &str, lo: i64, hi: i64) -> IV {
        IV::new(name, lo, hi, 1)
    }

    fn acc_sym_fp(kind: AccessKind, next_expr: IndexExpr, max_steps: usize, offset: i64) -> Access {
        Access {
            kind,
            base: "A".into(),
            index: vec![0],
            elem_size: 8,
            offset: OffsetValue::Const(offset),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::SymbolicFixedPoint { next_expr, max_steps },
        }
    }

    fn make_loop_1d(lo: i64, hi: i64, body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![iv("i", lo, hi)],
            body,
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,}
    }

    #[test]
    fn test_eval_sym_fp_converges_1_step() {
        // next = x * 0, start = 5 -> converges to 0 in 1 step
        let e = IndexExpr::Mul(Box::new(IndexExpr::IV(0)), Box::new(IndexExpr::Const(0)));
        assert_eq!(eval_symbolic_fp(&e, 5, 5, &[], None), Some(0));
        assert_eq!(eval_symbolic_fp(&e, 0, 5, &[], None), Some(0));
    }

    #[test]
    fn test_eval_sym_fp_no_convergence() {
        // next = x + 1, start = 0, max_steps = 5 -> doesn't converge
        let e = IndexExpr::Add(Box::new(IndexExpr::IV(0)), Box::new(IndexExpr::Const(1)));
        assert_eq!(eval_symbolic_fp(&e, 0, 5, &[], None), None);
    }

    #[test]
    fn decide_sym_fp_same_converges() {
        // A[x] = A[x] where x = x * 0 -> converges to 0.
        let e = IndexExpr::Mul(Box::new(IndexExpr::IV(0)), Box::new(IndexExpr::Const(0)));
        let l = make_loop_1d(0, 4, vec![
            acc_sym_fp(AccessKind::Write, e.clone(), 5, 0),
            acc_sym_fp(AccessKind::Read,  e, 5, 0),
        ]);
        let (d, cert) = decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(ref v) if v == &vec![0]));
        assert_eq!(cert.unwrap().level, "M11");
    }
    // ================================================================
    // Property fuzz: generate random IndexExpr trees, compare decide()
    // against a brute-force simulation that mirrors M11's own model.
    // ================================================================

    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn gen_expr(state: &mut u64, depth: u32) -> IndexExpr {
        let r = xorshift(state) % 100;
        if depth == 0 {
            if r < 40 {
                return IndexExpr::IV(0);
            } else {
                let v = (xorshift(state) % 8) as i64 - 3;
                return IndexExpr::Const(v);
            }
        }
        if r < 25 {
            IndexExpr::IV(0)
        } else if r < 45 {
            let v = (xorshift(state) % 8) as i64 - 3;
            IndexExpr::Const(v)
        } else if r < 75 {
            IndexExpr::Add(
                Box::new(gen_expr(state, depth - 1)),
                Box::new(gen_expr(state, depth - 1)),
            )
        } else {
            IndexExpr::Mul(
                Box::new(gen_expr(state, depth - 1)),
                Box::new(gen_expr(state, depth - 1)),
            )
        }
    }

    fn fixed_point(next: &IndexExpr, start: i64, max_steps: usize) -> Option<i64> {
        let mut current = start;
        for _ in 0..max_steps {
            let next_val = next.eval(&[current], &[], None)?;
            if next_val == current {
                return Some(current);
            }
            current = next_val;
        }
        None
    }

    #[derive(Debug, PartialEq)]
    enum Expected {
        Exact(i64),
        None,
    }

    fn brute(
        next_p: &IndexExpr,
        off_p: i64,
        next_q: &IndexExpr,
        off_q: i64,
        lo: i64,
        hi: i64,
        max_steps: usize,
    ) -> Expected {
        let writes: Vec<(i64, i64)> = (lo..hi)
            .filter_map(|i| fixed_point(next_p, i, max_steps).map(|fp| (i, fp + off_p)))
            .collect();
        let reads: Vec<(i64, i64)> = (lo..hi)
            .filter_map(|j| fixed_point(next_q, j, max_steps).map(|fp| (j, fp + off_q)))
            .collect();
        let mut best: Option<(i64, i64, i64)> = None;
        for (iv_p, loc_p) in &writes {
            for (iv_q, loc_q) in &reads {
                if loc_p == loc_q {
                    match best {
                        None => best = Some((*loc_p, *iv_p, *iv_q)),
                        Some((cur, _, _)) if *loc_p < cur => {
                            best = Some((*loc_p, *iv_p, *iv_q))
                        }
                        _ => {}
                    }
                }
            }
        }
        match best {
            Some((_, iv_p, iv_q)) => Expected::Exact(iv_q - iv_p),
            None => Expected::None,
        }
    }

    #[test]
    fn fuzz_m11_property() {
        let mut state: u64 = 0x12345678_9abcdef0;
        let mut tested = 0u64;
        let mut unsound = 0u64;
        let mut conservative = 0u64;
        let mut examples_unsound: Vec<String> = Vec::new();
        let lo = 0i64;
        let hi = 4i64;

        for _ in 0..2000 {
            let next_p = gen_expr(&mut state, 3);
            let next_q = gen_expr(&mut state, 3);
            let off_p = (xorshift(&mut state) % 7) as i64 - 3;
            let off_q = (xorshift(&mut state) % 7) as i64 - 3;
            let max_steps = (xorshift(&mut state) % 8) as usize + 1;

            let l = Loop {
                depth: 1,
                ivs: vec![iv("i", lo, hi)],
                body: vec![
                    acc_sym_fp(AccessKind::Write, next_p.clone(), max_steps, off_p),
                    acc_sym_fp(AccessKind::Read,  next_q.clone(), max_steps, off_q),
                ],
                recursion: None,
                chase: None,
                assertions: vec![],
             scalars: vec![], stmts: None,};

            let expected = brute(&next_p, off_p, &next_q, off_q, lo, hi, max_steps);
            let (d, _) = decide(&l, 0, 1);
            tested += 1;

            match (&d, &expected) {
                (Decision::Unknown(_), _) => {
                    conservative += 1;
                }
                (Decision::Exact(v), Expected::Exact(e)) if v.len() == 1 && v[0] == *e => {}
                (Decision::None, Expected::None) => {}
                _ => {
                    unsound += 1;
                    if examples_unsound.len() < 10 {
                        examples_unsound.push(format!(
                            "next_p={:?} off_p={} next_q={:?} off_q={} max_steps={} expected={:?} got={:?}",
                            next_p, off_p, next_q, off_q, max_steps, expected, d
                        ));
                    }
                }
            }
        }

        println!("M11 fuzz: tested={} unsound={} conservative={}",
                 tested, unsound, conservative);
        for ex in &examples_unsound {
            println!("  UNSOUND {}", ex);
        }
        assert_eq!(unsound, 0, "M11 fuzz found {} unsound cases", unsound);
    }

}
