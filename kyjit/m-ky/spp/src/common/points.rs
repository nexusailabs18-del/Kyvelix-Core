//! SPP Local copy of points-to analysis types (from UDDP m0)

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

/// Points-to set: a set of abstract locations a pointer may point to
pub type PointsToSet = HashMap<String, HashSet<String>>;

/// Pointer program statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PtrStmt {
    Alloc { var: String, size: usize },
    Copy { dst: String, src: String },
    AddrOf { dst: String, src: String },
    Store { base: String, offset: usize, src: String },
    Load { dst: String, base: String, offset: usize },
    Null { var: String },
}

/// A pointer program (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtrProgram {
    pub stmts: Vec<PtrStmt>,
}

/// Points-to analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsToResult {
    pub points_to: PointsToSet,
}

/// Run a simple flow-insensitive points-to analysis
pub fn analyze_points_to(program: &PtrProgram) -> PointsToResult {
    let mut points_to: PointsToSet = HashMap::new();

    // Initialize all variables with empty points-to sets
    for stmt in &program.stmts {
        match stmt {
            PtrStmt::Alloc { var, .. } => {
                points_to.entry(var.clone()).or_default();
            }
            PtrStmt::Copy { dst, .. } => {
                points_to.entry(dst.clone()).or_default();
            }
            PtrStmt::AddrOf { dst, .. } => {
                points_to.entry(dst.clone()).or_default();
            }
            PtrStmt::Null { var } => {
                points_to.entry(var.clone()).or_default();
            }
            _ => {}
        }
    }

    // Iterate to fixed point (simple Andersen-style)
    let mut changed = true;
    let mut iterations = 0;
    while changed && iterations < 100 {
        changed = false;
        for stmt in &program.stmts {
            match stmt {
                PtrStmt::Copy { dst, src } => {
                    let src_pts = points_to.get(src).cloned().unwrap_or_default();
                    let dst_pts = points_to.entry(dst.clone()).or_default();
                    for loc in src_pts {
                        if dst_pts.insert(loc) {
                            changed = true;
                        }
                    }
                }
                PtrStmt::AddrOf { dst, src } => {
                    let dst_pts = points_to.entry(dst.clone()).or_default();
                    if dst_pts.insert(src.clone()) {
                        changed = true;
                    }
                }
                PtrStmt::Store { base, offset: _, src } => {
                    // base[offset] = src - for simplicity, treat as base points to src's points-to
                    let src_pts = points_to.get(src).cloned().unwrap_or_default();
                    let base_pts = points_to.entry(base.clone()).or_default();
                    for loc in src_pts {
                        if base_pts.insert(loc) {
                            changed = true;
                        }
                    }
                }
                PtrStmt::Load { dst, base, offset: _ } => {
                    // dst = base[offset] - dst gets base's points-to
                    let base_pts = points_to.get(base).cloned().unwrap_or_default();
                    let dst_pts = points_to.entry(dst.clone()).or_default();
                    for loc in base_pts {
                        if dst_pts.insert(loc) {
                            changed = true;
                        }
                    }
                }
                PtrStmt::Null { var } => {
                    // null points to nothing
                    points_to.entry(var.clone()).or_default();
                }
                _ => {}
            }
        }
        iterations += 1;
    }

    PointsToResult { points_to }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_points_to() {
        let mut program = PtrProgram { stmts: vec![] };
        program.stmts.push(PtrStmt::Alloc { var: "p".into(), size: 1 });
        program.stmts.push(PtrStmt::Alloc { var: "q".into(), size: 1 });
        program.stmts.push(PtrStmt::AddrOf { dst: "p".into(), src: "obj".into() });
        program.stmts.push(PtrStmt::Copy { dst: "q".into(), src: "p".into() });

        let result = analyze_points_to(&program);
        assert!(result.points_to.get("q").unwrap().contains("obj"));
    }
}