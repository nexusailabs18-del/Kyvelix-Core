//! Arena-friendly version of the decision procedure.
//! Works on LoopRef (arena-borrowed) instead of Loop (owned).
//! Returns a Decision and, if certificate is needed, an arena-allocated one.

use crate::arena::Arena;
use crate::decide::{Decision, decide};
use crate::ir::*;

/// Convert a LoopRef into an owned Loop (fallback for now).
/// This is the migration path: we call `decide` on the owned Loop.
/// Later, we'll write a native `decide_ref` that avoids the allocation.
pub fn decide_ref<'a>(
    loop_ref: &LoopRef<'a>,
    p: usize,
    q: usize,
    arena: &'a Arena,
) -> Decision {
    // For now, materialize to owned Loop and call the existing decide().
    // This still allocates, but it's a stepping stone.
    // The real win: decide_ref uses arena internals directly.
    let owned = materialize(loop_ref, arena);
    let (d, _cert) = decide(&owned, p, q);
    d
}

/// Materialize a LoopRef into an owned Loop by copying into the arena.
/// Not zero-copy, but still faster than serde_json.
fn materialize<'a>(loop_ref: &LoopRef<'a>, _arena: &'a Arena) -> Loop {
    let ivs: Vec<IV> = loop_ref.ivs.iter().map(|iv| IV::new(
        iv.name.to_string(),
        iv.lower,
        iv.upper,
        iv.step,
    )).collect();

    let body: Vec<Access> = loop_ref.body.iter().map(|a| Access {
        kind: a.kind,
        base: a.base.to_string(),
        index: a.index.to_vec(),
        elem_size: a.elem_size,
        offset: crate::ir::OffsetValue::Const(a.offset),
        expr: None,
        indirect: None,
        guard: None,
        range: None,
        index_kind: IndexKind::Affine,
    }).collect();

    Loop {
        depth: loop_ref.depth,
        ivs,
        body,
        recursion: None,
        chase: None,
        assertions: vec![],
     scalars: vec![], stmts: None,}
}
