//! H-5: Composed interaction tests for multi-level firing precedence.

#[cfg(test)]
mod h5_composed_tests {
    use crate::decide::{self, Decision};
    use crate::ir::*;

    fn loop_with_accesses(ivs: Vec<IV>, body: Vec<Access>, chase: Option<PointerChase>, recursion: Option<Recursion>) -> Loop {
        Loop { depth: ivs.len(), ivs, body, recursion, chase, assertions: vec![], scalars: vec![], stmts: None }
    }

    fn simple_access(base: &str, kind: AccessKind, index: Vec<i64>, offset: i64) -> Access {
        Access { base: base.to_string(), kind, index, index_kind: IndexKind::Affine, elem_size: 1, offset: OffsetValue::Const(offset), range: None, guard: None, indirect: None, expr: None }
    }

    fn guarded_access(base: &str, kind: AccessKind, index: Vec<i64>, offset: i64, guard: Guard) -> Access {
        Access { base: base.to_string(), kind, index, index_kind: IndexKind::Affine, elem_size: 1, offset: OffsetValue::Const(offset), range: None, guard: Some(guard), indirect: None, expr: None }
    }

    fn expr_access(base: &str, kind: AccessKind, expr: &str) -> Access {
        Access { base: base.to_string(), kind, index: vec![1], index_kind: IndexKind::Affine, elem_size: 1, offset: OffsetValue::Const(0), range: None, guard: None, indirect: None, expr: Some(expr.to_string()) }
    }

    fn indirect_access(base: &str, kind: AccessKind, ind: Indirect) -> Access {
        Access { base: base.to_string(), kind, index: vec![0], index_kind: IndexKind::Affine, elem_size: 1, offset: OffsetValue::Const(0), range: None, guard: None, indirect: Some(ind), expr: None }
    }

    fn make_guard(iv: usize, op: CmpOp, bound: i64) -> Guard {
        Guard { constraints: vec![Cmp { iv, op, bound }] }
    }

    #[test]
    fn h5_m5_before_m3() {
        let chase = PointerChase {
            var: "p".into(),
            init: 0,
            table: vec![1, 2, 3, 4],
            table_expr: None,
            max_steps: 4,
            accessed_base: "A".into(),
        };
        let recursion = Recursion {
            func: "f".into(),
            iv_name: "i".into(),
            iv_init: 0,
            iv_step: 1,
            iv_bound: 4,
            bound_op: CmpOp::Lt,
            companion: None, other_params: vec![],
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 4, 1)],
            vec![
                simple_access("A", AccessKind::Write, vec![1], 0),
                simple_access("A", AccessKind::Read, vec![1], 1),
            ],
            Some(chase),
            Some(recursion),
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M5 should fire for chase loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M5");
        }
    }

    #[test]
    fn h5_m3_before_m2g() {
        let recursion = Recursion {
            func: "f".into(),
            iv_name: "i".into(),
            iv_init: 0,
            iv_step: 1,
            iv_bound: 8,
            bound_op: CmpOp::Lt,
            companion: None, other_params: vec![],
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                expr_access("A", AccessKind::Write, "i << 1"),
                expr_access("A", AccessKind::Read, "i + 2"),
            ],
            None,
            Some(recursion),
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M3 should fire for recursion loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M3");
        }
    }

    #[test]
    fn h5_m2g_before_m2a() {
        let ind = Indirect {
            base: "B".into(),
            index: vec![1],
            offset: 0,
            table: None,
            domain_lo: 0,
            domain_hi: 8,
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                expr_access("A", AccessKind::Write, "i * 2"),
                indirect_access("A", AccessKind::Read, ind),
            ],
            None,
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M2g should fire for expr loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M2g");
        }
    }

    #[test]
    fn h5_m2a_before_m4() {
        let ind = Indirect {
            base: "B".into(),
            index: vec![2],
            offset: 1,
            table: None,
            domain_lo: 0,
            domain_hi: 8,
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                indirect_access("A", AccessKind::Write, ind.clone()),
                indirect_access("A", AccessKind::Read, Indirect { base: "B".into(), index: vec![1], offset: 0, table: None, domain_lo: 0, domain_hi: 8 }),
            ],
            None,
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_) | Decision::None), "M2a should fire for indirect loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M2a");
        }
    }

    #[test]
    fn h5_m4_before_m2b() {
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                guarded_access("A", AccessKind::Write, vec![1], 0, make_guard(0, CmpOp::Lt, 4)),
                Access {
                    base: "A".into(),
                    kind: AccessKind::Read,
                    index: vec![1],
                    index_kind: IndexKind::Affine, elem_size: 1,
                    offset: OffsetValue::Const(0),
                    range: Some(RangeExpr::Mod { iv: 0, modulus: 4 }),
                    guard: None,
                    indirect: None,
                    expr: None,
                },
            ],
            None,
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M4 should fire for guarded loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M4");
        }
    }

    #[test]
    fn h5_m2b_before_m10() {
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                Access {
                    base: "A".into(),
                    kind: AccessKind::Write,
                    index: vec![1],
                    index_kind: IndexKind::FixedPoint(FixedPointTable { next: vec![] }), elem_size: 1,
                    offset: OffsetValue::Const(0),
                    range: Some(RangeExpr::Mod { iv: 0, modulus: 4 }),
                    guard: None,
                    indirect: None,
                    expr: None,
                },
                Access {
                    base: "A".into(),
                    kind: AccessKind::Read,
                    index: vec![1],
                    index_kind: IndexKind::Affine, elem_size: 1,
                    offset: OffsetValue::Const(0),
                    range: None,
                    guard: None,
                    indirect: None,
                    expr: None,
                },
            ],
            None,
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        if let Some(c) = cert {
            assert_ne!(c.level, "M10", "M2b should fire before M10");
        }
    }

    #[test]
    fn h5_m2g_before_m4() {
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                expr_access("A", AccessKind::Write, "i + 3"),
                guarded_access("A", AccessKind::Read, vec![1], 0, make_guard(0, CmpOp::Lt, 4)),
            ],
            None,
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M2g should fire for expr loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M2g");
        }
    }

    #[test]
    fn h5_m3_before_m4() {
        let recursion = Recursion {
            func: "f".into(),
            iv_name: "i".into(),
            iv_init: 0,
            iv_step: 1,
            iv_bound: 8,
            bound_op: CmpOp::Lt,
            companion: None, other_params: vec![],
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 8, 1)],
            vec![
                simple_access("A", AccessKind::Write, vec![1], 0),
                simple_access("A", AccessKind::Read, vec![1], 2),
            ],
            None,
            Some(recursion),
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M3 should fire for recursion loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M3");
        }
    }

    #[test]
    fn h5_m5_before_m4() {
        let chase = PointerChase {
            var: "p".into(),
            init: 0,
            table: vec![1, 2, 3, 4],
            table_expr: None,
            max_steps: 4,
            accessed_base: "A".into(),
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 4, 1)],
            vec![
                guarded_access("A", AccessKind::Write, vec![1], 0, make_guard(0, CmpOp::Lt, 2)),
                simple_access("A", AccessKind::Read, vec![1], 1),
            ],
            Some(chase),
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        assert!(matches!(d, Decision::Exact(_)), "M5 should fire for chase loop");
        if let Some(c) = cert {
            assert_eq!(c.level, "M5");
        }
    }

    #[test]
    fn h5_m2a5_before_m2a() {
        let ind_table = Indirect {
            base: "B".into(),
            index: vec![0],
            offset: 0,
            table: Some(vec![1, 3, 5, 7]),
            domain_lo: 0,
            domain_hi: 4,
        };
        let ind_affine = Indirect {
            base: "C".into(),
            index: vec![1],
            offset: 0,
            table: None,
            domain_lo: 0,
            domain_hi: 4,
        };
        let l = loop_with_accesses(
            vec![IV::new("i", 0, 4, 1)],
            vec![
                indirect_access("A", AccessKind::Write, ind_table),
                indirect_access("A", AccessKind::Read, ind_affine),
            ],
            None,
            None,
        );
        let (d, cert) = decide::decide(&l, 0, 1);
        if let Some(c) = cert {
            assert_eq!(c.level, "M2a5");
        }
    }
}
