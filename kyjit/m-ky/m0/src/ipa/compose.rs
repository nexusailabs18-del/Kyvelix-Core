//! Phase 8 §8.9 — composing summaries at call sites (Wolfe §8.5).
//!
//! A callee's summary is written in the callee's own symbol space: its formals
//! and the globals. To fold it into the caller's summary those symbols must be
//! translated to the caller's: a callee formal becomes whatever expression the
//! caller passed as that argument, and a global stays a global (globals are
//! shared; that is the whole reason this analysis is interprocedural).
//!
//! A callee *local* never translates — it cannot escape — and an argument the
//! model cannot name (a computed value, a pointer) translates to nothing, so no
//! effect is claimed through it. That is the same conservative choice Phase 7
//! and [`crate::ipa::mod_analysis`] make.

use std::collections::BTreeSet;

use crate::ipa::program::{Call, IpaExpr, IpaProgram, Slot};

/// Translate a callee symbol into the caller's symbol space at one call site.
///
/// Returns `None` for a symbol that has no caller-side image (a callee local),
/// or for a formal whose actual the model cannot name.
pub fn map_symbol(_prog: &IpaProgram, call: &Call, sym: Slot) -> Option<Slot> {
    match sym {
        Slot::Global(k) => Some(Slot::Global(k)),
        Slot::Param(j) => match call.args.get(j) {
            Some(IpaExpr::Read(s)) | Some(IpaExpr::AddrOf(s)) => Some(*s),
            _ => None,
        },
        Slot::Local(_) => None,
    }
}

/// Fold a callee's read/write sets into a caller's at one call site.
///
/// Returns `true` when either output set grew. `prog` is accepted for symmetry
/// with the rest of the module's API and for future shape checks; the mapping
/// itself needs only the call.
pub fn compose_into(
    _prog: &IpaProgram,
    call: &Call,
    callee_reads: &BTreeSet<Slot>,
    callee_writes: &BTreeSet<Slot>,
    caller_reads: &mut BTreeSet<Slot>,
    caller_writes: &mut BTreeSet<Slot>,
) -> bool {
    let mut changed = false;
    for s in callee_reads {
        if let Some(mapped) = map_symbol(_prog, call, *s) {
            changed |= caller_reads.insert(mapped);
        }
    }
    for s in callee_writes {
        if let Some(mapped) = map_symbol(_prog, call, *s) {
            changed |= caller_writes.insert(mapped);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipa::program::{Callee, Proc, ProcId};

    fn one_proc_program() -> IpaProgram {
        IpaProgram::new(vec!["g".into()], vec![Proc::new("main", vec![], vec![])], ProcId(0))
    }

    #[test]
    fn a_global_maps_to_itself() {
        let prog = one_proc_program();
        let call = Call { callee: Callee::Direct(ProcId(0)), args: vec![], ret: None };
        assert_eq!(map_symbol(&prog, &call, Slot::Global(0)), Some(Slot::Global(0)));
    }

    #[test]
    fn a_formal_maps_to_the_argument() {
        let prog = one_proc_program();
        let call = Call {
            callee: Callee::Direct(ProcId(0)),
            args: vec![IpaExpr::Read(Slot::Local(3))],
            ret: None,
        };
        assert_eq!(map_symbol(&prog, &call, Slot::Param(0)), Some(Slot::Local(3)));
    }

    #[test]
    fn a_local_does_not_map_and_a_computed_arg_does_not() {
        let prog = one_proc_program();
        let call = Call {
            callee: Callee::Direct(ProcId(0)),
            args: vec![IpaExpr::Const(5)],
            ret: None,
        };
        assert_eq!(map_symbol(&prog, &call, Slot::Local(0)), None);
        assert_eq!(map_symbol(&prog, &call, Slot::Param(0)), None);
    }

    #[test]
    fn compose_folds_the_callees_sets_into_the_callers() {
        let prog = one_proc_program();
        let call = Call {
            callee: Callee::Direct(ProcId(0)),
            args: vec![IpaExpr::Read(Slot::Param(1))],
            ret: None,
        };
        let mut reads = BTreeSet::new();
        let mut writes = BTreeSet::new();
        let mut cr = BTreeSet::new();
        let mut cw = BTreeSet::new();
        // An empty callee summary contributes nothing.
        assert!(!compose_into(&prog, &call, &reads, &writes, &mut cr, &mut cw));
        reads.insert(Slot::Param(0));
        writes.insert(Slot::Param(0));
        let mut cr2 = BTreeSet::new();
        let mut cw2 = BTreeSet::new();
        assert!(compose_into(&prog, &call, &reads, &writes, &mut cr2, &mut cw2));
        assert!(cr2.contains(&Slot::Param(1)));
        assert!(cw2.contains(&Slot::Param(1)));
    }
}