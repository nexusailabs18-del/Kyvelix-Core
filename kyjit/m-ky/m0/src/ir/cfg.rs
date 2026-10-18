//! Control-flow graph over scalar statements (Phase 3).
//!
//! A [`CFG`] is a list of basic blocks plus an entry and an exit. Each block
//! holds a list of [`Stmt`]s (uses then definitions, in RHS-then-LHS order)
//! and optionally φ-terms / Υ-terms attached by placement.
//!
//! For the loops the current IR accepts there are no branches, so construction
//! (see [`crate::fud::cfg`]) yields the canonical three-block shape
//! `entry → body → exit`. The data model and every algorithm in [`crate::fud`]
//! are general over arbitrary reducible CFGs, because Phase 9's frontend will
//! emit branches and the machinery must already be correct for them.

use serde::{Deserialize, Serialize};

use super::scalar::{PhiTerm, ScalarDef, ScalarUse, ScalarVar, UpsilonTerm};
use super::scalar::{DefId, PhiId, UseId};

/// A single statement. `uses` are evaluated before `defs` (right-hand side
/// before left-hand side), so `x = x + 1` records the use of `x` before its
/// definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stmt {
    pub id: usize,
    pub uses: Vec<ScalarUse>,
    pub defs: Vec<ScalarDef>,
}

/// A basic block. `preds`/`succs` are block indices (into [`CFG::blocks`])
/// and must be symmetric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub id: usize,
    pub preds: Vec<usize>,
    pub succs: Vec<usize>,
    pub stmts: Vec<Stmt>,
    /// Φ-terms attached to this block (SSA merge points). Empty unless
    /// placement has run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phi: Vec<PhiTerm>,
    /// Υ-terms attached to this block (FRUD merge points). Empty unless
    /// Υ-placement has run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upsilon: Vec<UpsilonTerm>,
    /// Guard constraints emitted at this block (header). Populated by the
    /// CFG builder when lowering `StmtKind::Guard`. Used by M3 to recover
    /// loop bounds from the CFG instead of recursion metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guards: Vec<crate::ir::Cmp>,
}

/// The control-flow graph. `entry` has no predecessors, `exit` no successors;
/// every block is reachable from `entry` and reaches `exit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CFG {
    /// Variable table. Uses/defs/terms refer to variables by index.
    pub vars: Vec<ScalarVar>,
    pub blocks: Vec<Block>,
    pub entry: usize,
    pub exit: usize,
}

/// The canonical position of the i-th use. `UseId(i)` indexes into
/// `CFG::index().uses`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UseSite {
    pub var: usize,
    pub block: usize,
    /// Statement index within the block.
    pub stmt: usize,
    /// Position within the statement's use list.
    pub order: usize,
}

/// Site of the i-th def (`DefId(i)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DefSite {
    pub var: usize,
    pub block: usize,
    pub stmt: usize,
    pub order: usize,
}

/// Site of the i-th φ-term (`PhiId(i)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhiSite {
    pub var: usize,
    pub block: usize,
}

/// Site of the i-th Υ-term (`UpsilonId(i)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UpsilonSite {
    pub var: usize,
    pub block: usize,
}

/// Canonical enumeration of every use, def and term in a CFG.
/// The numbering is: blocks in id order; within a block, φ and Υ terms first
/// (in their stored order), then statements in order (uses then defs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CfgIndex {
    pub uses: Vec<UseSite>,
    pub defs: Vec<DefSite>,
    pub phis: Vec<PhiSite>,
    pub upsilons: Vec<UpsilonSite>,
}


impl CFG {
    /// The canonical three-block CFG `entry → body → exit` with the given
    /// statements in one body block. Rewrites each statement's `id`, each
    /// use's and def's `stmt`/`order` fields to their canonical values.
    pub fn new_single_block(vars: Vec<ScalarVar>, mut stmts: Vec<Stmt>) -> CFG {
        for (i, st) in stmts.iter_mut().enumerate() {
            st.id = i;
            for (j, u) in st.uses.iter_mut().enumerate() {
                u.stmt = i;
                u.order = j;
            }
            for (j, d) in st.defs.iter_mut().enumerate() {
                d.stmt = i;
                d.order = j;
            }
        }
        let body = Block {
            id: 1,
            preds: vec![0],
            succs: vec![2],
            stmts,
            phi: vec![],
            upsilon: vec![],
            guards: vec![],
        };
        CFG {
            vars,
            blocks: vec![
                Block { id: 0, preds: vec![], succs: vec![1], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
                body,
                Block { id: 2, preds: vec![1], succs: vec![], stmts: vec![], phi: vec![], upsilon: vec![], guards: vec![] },
            ],
            entry: 0,
            exit: 2,
        }
    }

    /// Enumerate every use/def/term in canonical order.
    pub fn index(&self) -> CfgIndex {
        let mut ix = CfgIndex::default();
        for (b, blk) in self.blocks.iter().enumerate() {
            for p in &blk.phi {
                ix.phis.push(PhiSite { var: p.var, block: b });
            }
            for up in &blk.upsilon {
                ix.upsilons.push(UpsilonSite { var: up.var, block: b });
            }
            for (s, st) in blk.stmts.iter().enumerate() {
                for (o, u) in st.uses.iter().enumerate() {
                    ix.uses.push(UseSite { var: u.var, block: b, stmt: s, order: o });
                }
                for (o, d) in st.defs.iter().enumerate() {
                    ix.defs.push(DefSite { var: d.var, block: b, stmt: s, order: o });
                }
            }
        }
        ix
    }

    /// The block containing use `id`.
    pub fn use_block(&self, id: UseId) -> usize {
        self.index().uses[id.0].block
    }

    /// The variable of use `id`.
    pub fn use_var(&self, id: UseId) -> usize {
        self.index().uses[id.0].var
    }

    /// The variable of def `id`.
    pub fn def_var(&self, id: DefId) -> usize {
        self.index().defs[id.0].var
    }

    /// The block containing def `id`.
    pub fn def_block(&self, id: DefId) -> usize {
        self.index().defs[id.0].block
    }

    /// The variable of φ-term `id`.
    pub fn phi_var(&self, id: PhiId) -> usize {
        self.index().phis[id.0].var
    }

    /// Reverse the CFG: swap predecessors and successors and exchange
    /// entry and exit. Block ids, statements and variables are unchanged.
    /// Φ/Υ terms are dropped — placement runs on the reversed graph freshly.
    pub fn reversed(&self) -> CFG {
        let blocks = self
            .blocks
            .iter()
            .map(|b| Block {
                id: b.id,
                preds: b.succs.clone(),
                succs: b.preds.clone(),
                stmts: b.stmts.clone(),
                phi: vec![],
                upsilon: vec![],
                guards: vec![],
            })
            .collect();
        CFG {
            vars: self.vars.clone(),
            blocks,
            entry: self.exit,
            exit: self.entry,
        }
    }
}

impl CFG {
    /// Structural invariants. `Err` describes the first violation.
    ///
    /// Checked: block ids are positional; preds/succs are symmetric and
    /// in range; entry has no preds; exit has no succs; every block is
    /// reachable from entry and reaches exit; statement ids are positional;
    /// recorded `stmt`/`order` fields agree with position; every var index is
    /// in range; at most one φ (`Υ`) term per (block, variable).
    pub fn validate(&self) -> Result<(), String> {
        let n = self.blocks.len();
        if n == 0 {
            return Err("CFG has no blocks".into());
        }
        if self.entry >= n {
            return Err(format!("entry {} out of range", self.entry));
        }
        if self.exit >= n {
            return Err(format!("exit {} out of range", self.exit));
        }
        let nv = self.vars.len();
        for (b, blk) in self.blocks.iter().enumerate() {
            if blk.id != b {
                return Err(format!("block {} has id {}", b, blk.id));
            }
            for &p in &blk.preds {
                if p >= n {
                    return Err(format!("block {} pred {} out of range", b, p));
                }
                if !self.blocks[p].succs.contains(&b) {
                    return Err(format!("edge {} -> {} missing reverse", p, b));
                }
            }
            for &s in &blk.succs {
                if s >= n {
                    return Err(format!("block {} succ {} out of range", b, s));
                }
                if !self.blocks[s].preds.contains(&b) {
                    return Err(format!("edge {} -> {} missing reverse", b, s));
                }
            }
            if b == self.entry && !blk.preds.is_empty() {
                return Err("entry has predecessors".into());
            }
            if b == self.exit && !blk.succs.is_empty() {
                return Err("exit has successors".into());
            }
            let mut phi_seen: Vec<usize> = vec![];
            for p in &blk.phi {
                if p.var >= nv {
                    return Err(format!("phi var {} out of range", p.var));
                }
                if p.block != b {
                    return Err(format!("phi at block {} claims block {}", b, p.block));
                }
                if phi_seen.contains(&p.var) {
                    return Err(format!("duplicate phi for var {} at block {}", p.var, b));
                }
                phi_seen.push(p.var);
            }
            let mut up_seen: Vec<usize> = vec![];
            for up in &blk.upsilon {
                if up.var >= nv {
                    return Err(format!("upsilon var {} out of range", up.var));
                }
                if up.block != b {
                    return Err(format!("upsilon at block {} claims block {}", b, up.block));
                }
                if up_seen.contains(&up.var) {
                    return Err(format!("duplicate upsilon for var {} at block {}", up.var, b));
                }
                up_seen.push(up.var);
            }
            for (s, st) in blk.stmts.iter().enumerate() {
                if st.id != s {
                    return Err(format!("block {} stmt {} has id {}", b, s, st.id));
                }
                for (o, u) in st.uses.iter().enumerate() {
                    if u.var >= nv {
                        return Err(format!("use var {} out of range", u.var));
                    }
                    if u.stmt != s || u.order != o {
                        return Err(format!(
                            "use position mismatch at block {} stmt {} order {}",
                            b, s, o
                        ));
                    }
                }
                for (o, d) in st.defs.iter().enumerate() {
                    if d.var >= nv {
                        return Err(format!("def var {} out of range", d.var));
                    }
                    if d.stmt != s || d.order != o {
                        return Err(format!(
                            "def position mismatch at block {} stmt {} order {}",
                            b, s, o
                        ));
                    }
                }
            }
        }
        // Reachability from entry.
        let mut seen = vec![false; n];
        let mut stack = vec![self.entry];
        seen[self.entry] = true;
        while let Some(x) = stack.pop() {
            for &s in &self.blocks[x].succs {
                if !seen[s] {
                    seen[s] = true;
                    stack.push(s);
                }
            }
        }
        for (b, ok) in seen.iter().enumerate() {
            if !ok {
                return Err(format!("block {} not reachable from entry", b));
            }
        }
        // Reachability to exit (backward).
        let mut seen = vec![false; n];
        let mut stack = vec![self.exit];
        seen[self.exit] = true;
        while let Some(x) = stack.pop() {
            for &p in &self.blocks[x].preds {
                if !seen[p] {
                    seen[p] = true;
                    stack.push(p);
                }
            }
        }
        for (b, ok) in seen.iter().enumerate() {
            if !ok {
                return Err(format!("block {} cannot reach exit", b));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::scalar::{ScalarDef, ScalarRhs, ScalarTy, ScalarUse};
    use super::*;

    fn var(name: &str) -> ScalarVar {
        ScalarVar { name: name.into(), ty: ScalarTy::I64 }
    }

    fn stmt(id: usize, uses: Vec<usize>, defs: Vec<usize>) -> Stmt {
        Stmt {
            id,
            uses: uses.into_iter().map(|var| ScalarUse { var, stmt: 0, order: 0 }).collect(),
            defs: defs
                .into_iter()
                .map(|var| ScalarDef { var, stmt: 0, order: 0, rhs: ScalarRhs::Unknown })
                .collect(),
        }
    }

    #[test]
    fn single_block_shape_is_canonical() {
        let cfg = CFG::new_single_block(
            vec![var("x")],
            vec![stmt(99, vec![0], vec![0])],
        );
        assert_eq!(cfg.entry, 0);
        assert_eq!(cfg.exit, 2);
        assert_eq!(cfg.blocks.len(), 3);
        assert_eq!(cfg.blocks[0].succs, vec![1]);
        assert_eq!(cfg.blocks[1].preds, vec![0]);
        assert_eq!(cfg.blocks[1].succs, vec![2]);
        assert_eq!(cfg.blocks[2].preds, vec![1]);
        assert_eq!(cfg.blocks[1].stmts.len(), 1);
        cfg.validate().expect("shape is valid");
        // Recorded ids/positions were rewritten to canonical values.
        assert_eq!(cfg.blocks[1].stmts[0].id, 0);
        assert_eq!(cfg.blocks[1].stmts[0].uses[0].stmt, 0);
        assert_eq!(cfg.blocks[1].stmts[0].uses[0].order, 0);
        assert_eq!(cfg.blocks[1].stmts[0].defs[0].order, 0);
    }

    #[test]
    fn index_numbering_is_canonical() {
        let cfg = CFG::new_single_block(
            vec![var("x"), var("y")],
            vec![stmt(0, vec![0, 1], vec![1]), stmt(1, vec![1], vec![0])],
        );
        let ix = cfg.index();
        assert_eq!(ix.uses.len(), 3);
        assert_eq!(ix.uses[0], UseSite { var: 0, block: 1, stmt: 0, order: 0 });
        assert_eq!(ix.uses[1], UseSite { var: 1, block: 1, stmt: 0, order: 1 });
        assert_eq!(ix.uses[2], UseSite { var: 1, block: 1, stmt: 1, order: 0 });
        assert_eq!(ix.defs.len(), 2);
        assert_eq!(ix.defs[0], DefSite { var: 1, block: 1, stmt: 0, order: 0 });
        assert_eq!(ix.defs[1], DefSite { var: 0, block: 1, stmt: 1, order: 0 });
        assert_eq!(cfg.use_var(UseId(2)), 1);
        assert_eq!(cfg.def_var(DefId(1)), 0);
        assert_eq!(cfg.def_block(DefId(0)), 1);
    }

    #[test]
    fn validate_catches_asymmetric_edges() {
        let mut cfg = CFG::new_single_block(vec![], vec![]);
        cfg.blocks[0].succs.push(2);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("missing reverse"), "got: {}", err);
    }

    #[test]
    fn validate_catches_var_out_of_range() {
        let mut cfg = CFG::new_single_block(vec![var("x")], vec![stmt(0, vec![0], vec![0])]);
        cfg.blocks[1].stmts[0].uses[0].var = 5;
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("out of range"), "got: {}", err);
    }

    #[test]
    fn reversed_swaps_edges_and_entry_exit() {
        let cfg = CFG::new_single_block(vec![var("x")], vec![stmt(0, vec![0], vec![0])]);
        let rev = cfg.reversed();
        assert_eq!(rev.entry, 2);
        assert_eq!(rev.exit, 0);
        assert_eq!(rev.blocks[0].succs, Vec::<usize>::new());
        assert_eq!(rev.blocks[0].preds, vec![1]);
        assert_eq!(rev.blocks[1].preds, vec![2]);
        assert_eq!(rev.blocks[1].succs, vec![0]);
        assert_eq!(rev.blocks[2].preds, Vec::<usize>::new());
        assert_eq!(rev.blocks[2].succs, vec![1]);
        rev.validate().expect("reversed CFG is valid");
        // Statements survive reversal.
        assert_eq!(rev.blocks[1].stmts, cfg.blocks[1].stmts);
    }

    #[test]
    fn cfg_round_trips_through_json() {
        let cfg = CFG::new_single_block(
            vec![var("x"), var("y")],
            vec![stmt(0, vec![0, 1], vec![1])],
        );
        let s = serde_json::to_string(&cfg).unwrap();
        let back: CFG = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn phi_and_upsilon_round_trip_and_validate() {
        let mut cfg = CFG::new_single_block(vec![var("x")], vec![stmt(0, vec![0], vec![0])]);
        cfg.blocks[1].phi.push(PhiTerm { var: 0, block: 1 });
        cfg.blocks[1].upsilon.push(UpsilonTerm { var: 0, block: 1 });
        cfg.validate().unwrap();
        let s = serde_json::to_string(&cfg).unwrap();
        let back: CFG = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cfg);
        // A second phi for the same variable at the same block is rejected.
        cfg.blocks[1].phi.push(PhiTerm { var: 0, block: 1 });
        assert!(cfg.validate().unwrap_err().contains("duplicate phi"));
    }
}
