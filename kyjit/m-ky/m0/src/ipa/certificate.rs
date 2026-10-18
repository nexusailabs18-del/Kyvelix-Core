//! Phase 8 §8.10 — the interprocedural certificate and its checker replay.
//!
//! A Phase 8 certificate names one interprocedural fact and carries the *whole
//! program* it was computed from. `m0-check` re-runs the relevant analysis and
//! re-derives the fact; nothing the analyzer concluded crosses the boundary.
//! This is the same discipline Phases 6 and 7 follow, and it is why the witness
//! carries a [`IpaProgram`] rather than a pre-computed summary.
//!
//! Besides the facts the spec's §8.5 considers (call graph, purity, MOD,
//! constant propagation), the module exposes a whole-procedure summary claim so
//! the read/write sets of [`crate::ipa::summaries`] are checkable too.

use serde::{Deserialize, Serialize};

use crate::ipa::callgraph::CallGraph;
use crate::ipa::constprop::{self, AbstractValue};
use crate::ipa::io_effects;
use crate::ipa::mod_analysis;
use crate::ipa::program::{IpaProgram, IpaStmt, ProcId, Slot};
use crate::ipa::pure;
use crate::ipa::summaries;
use crate::ir::purity::Purity;

/// What a Phase 8 certificate asserts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ipa", rename_all = "snake_case")]
pub enum IpaClaim {
    /// `proc` has this purity (Wolfe §8.4).
    Purity { proc: ProcId, purity: Purity },
    /// `proc` may modify exactly these symbols (Wolfe §8.5.2).
    ModSet { proc: ProcId, symbols: Vec<Slot> },
    /// The I/O statements at `first` and `second` in `proc` must keep their
    /// order (Wolfe §8.3).
    IoOrder { proc: ProcId, first: usize, second: usize },
    /// The formal `formal` of `proc` holds this abstract value (Wolfe §8.5.3).
    FormalValue { proc: ProcId, formal: usize, value: AbstractValue },
    /// `proc`'s bottom-up summary has exactly these read/write sets (Wolfe §8.5).
    Summary { proc: ProcId, read: Vec<Slot>, write: Vec<Slot> },
}

/// Sort a slot list so a set comparison is order-independent.
fn sorted(mut v: Vec<Slot>) -> Vec<Slot> {
    v.sort();
    v
}

/// Re-derive `claim` from `program`. `Ok(())` iff the program entails it.
pub fn check_ipa_claim(claim: &IpaClaim, program: &IpaProgram) -> Result<(), String> {
    program
        .validate()
        .map_err(|e| format!("carried program is malformed: {}", e))?;
    let cg = crate::ipa::callgraph::build(program);

    match claim {
        IpaClaim::Purity { proc, purity } => {
            let actual = pure::classify(program, &cg);
            let got = actual
                .get(proc.index())
                .cloned()
                .ok_or_else(|| format!("proc {} out of range", proc.index()))?;
            if got != *purity {
                return Err(format!(
                    "rebuilt purity {:?} disagrees with the claim {:?}",
                    got, purity
                ));
            }
            Ok(())
        }
        IpaClaim::ModSet { proc, symbols } => {
            let a = mod_analysis::analyze(program, &cg);
            let got = a
                .mod_sets
                .get(proc)
                .cloned()
                .ok_or_else(|| format!("proc {} out of range", proc.index()))?;
            if got != symbols.iter().copied().collect() {
                return Err(format!(
                    "rebuilt MOD set {:?} disagrees with the claim {:?}",
                    got, symbols
                ));
            }
            Ok(())
        }
        IpaClaim::IoOrder { proc, first, second } => {
            let ops = io_effects::io_ops(program, *proc);
            let a = ops.iter().find(|o| o.stmt == *first);
            let b = ops.iter().find(|o| o.stmt == *second);
            match (a, b) {
                (Some(a), Some(b)) if io_effects::must_serialize(a, b) => Ok(()),
                (Some(_), Some(_)) => {
                    Err("the two I/O statements are not on the same channel".into())
                }
                _ => Err("the claim names a statement that is not an I/O operation".into()),
            }
        }
        IpaClaim::FormalValue { proc, formal, value } => {
            let values = constprop::analyze(program, &cg, &[]);
            // `analyze` returns a dense map, so a missing key means the formal
            // does not exist rather than that it was never reached.
            let got = values
                .get(&(*proc, *formal))
                .cloned()
                .ok_or_else(|| format!("proc {} has no formal {}", proc.index(), formal))?;
            if got != *value {
                return Err(format!(
                    "rebuilt formal value {:?} disagrees with the claim {:?}",
                    got, value
                ));
            }
            Ok(())
        }
        IpaClaim::Summary { proc, read, write } => {
            let s = summaries::compute(program, &cg);
            let got = s
                .get(proc)
                .ok_or_else(|| format!("proc {} out of range", proc.index()))?;
            let read_ok = sorted(read.clone()) == got.read_set.iter().copied().collect::<Vec<_>>();
            let write_ok = sorted(write.clone()) == got.write_set.iter().copied().collect::<Vec<_>>();
            if !read_ok || !write_ok {
                return Err(format!(
                    "rebuilt summary (read {:?}, write {:?}) disagrees with the claim",
                    got.read_set, got.write_set
                ));
            }
            Ok(())
        }
    }
}

/// Build a Phase 8 certificate for `claim` over `program`, anchored to the loop
/// file `loop_` (which supplies the hash and pair the format carries).
///
/// Returns `None` when the checker would reject the claim — the emitter runs
/// [`check_ipa_claim`] itself first.
pub fn interprocedural_certificate(
    loop_: &crate::ir::Loop,
    program: &IpaProgram,
    claim: IpaClaim,
    level: &str,
) -> Option<crate::certificate::Certificate> {
    if check_ipa_claim(&claim, program).is_err() {
        return None;
    }
    Some(crate::certificate::Certificate {
        level: level.to_string(),
        loop_hash: loop_.hash(),
        pair: crate::certificate::emit::evidence_pair(loop_),
        claim: crate::certificate::Claim::Interprocedural(claim),
        witness: crate::certificate::Witness::IpaEvidence {
            program: Box::new(program.clone()),
        },
        checker: "m0-check".to_string(),
    })
}

/// The call graph of a program, shared so a producer does not rebuild it twice.
pub fn callgraph_of(program: &IpaProgram) -> CallGraph {
    crate::ipa::callgraph::build(program)
}

/// True when the statement at `idx` in `proc` is an I/O statement.
pub fn is_io(program: &IpaProgram, proc: ProcId, idx: usize) -> bool {
    program
        .proc(proc)
        .and_then(|p| p.stmts.get(idx))
        .map(|s| matches!(s, IpaStmt::Io { .. }))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipa::program::{Call, Callee, IpaExpr, IoKind, Proc};

    fn proc_with(name: &str, params: Vec<&str>, stmts: Vec<IpaStmt>) -> Proc {
        // Give every test procedure one local so `Slot::Local(0)` is in range
        // and `IpaProgram::validate` accepts the carried program.
        let mut p = Proc::new(
            name,
            params.into_iter().map(String::from).collect(),
            vec!["l0".to_string()],
        );
        p.stmts = stmts;
        p
    }

    fn call1(callee: ProcId, args: Vec<IpaExpr>) -> IpaStmt {
        IpaStmt::Call(Call { callee: Callee::Direct(callee), args, ret: None })
    }

    #[test]
    fn a_purity_claim_is_re_derived() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("pure", vec![], vec![]),
                proc_with(
                    "writer",
                    vec![],
                    vec![IpaStmt::Io { channel: "o".into(), kind: IoKind::Write }],
                ),
            ],
            ProcId(0),
        );
        assert!(
            check_ipa_claim(&IpaClaim::Purity { proc: ProcId(0), purity: Purity::Pure }, &prog)
                .is_ok()
        );
        assert!(
            check_ipa_claim(&IpaClaim::Purity { proc: ProcId(0), purity: Purity::Impure }, &prog)
                .is_err()
        );
    }

    #[test]
    fn a_mod_set_claim_is_re_derived() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "p",
                vec![],
                vec![IpaStmt::Assign { target: Slot::Local(0), value: IpaExpr::Const(1) }],
            )],
            ProcId(0),
        );
        assert!(check_ipa_claim(
            &IpaClaim::ModSet { proc: ProcId(0), symbols: vec![Slot::Local(0)] },
            &prog
        )
        .is_ok());
        assert!(check_ipa_claim(&IpaClaim::ModSet { proc: ProcId(0), symbols: vec![] }, &prog).is_err());
    }

    #[test]
    fn an_io_order_claim_needs_the_same_channel() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "p",
                vec![],
                vec![
                    IpaStmt::Io { channel: "f".into(), kind: IoKind::Write },
                    IpaStmt::Io { channel: "g".into(), kind: IoKind::Read },
                ],
            )],
            ProcId(0),
        );
        assert!(check_ipa_claim(&IpaClaim::IoOrder { proc: ProcId(0), first: 0, second: 1 }, &prog).is_err());
    }

    #[test]
    fn a_formal_value_claim_is_re_derived() {
        let prog = IpaProgram::new(
            vec![],
            vec![
                proc_with("main", vec![], vec![call1(ProcId(1), vec![IpaExpr::Const(5)])]),
                proc_with("g", vec!["x"], vec![]),
            ],
            ProcId(0),
        );
        assert!(check_ipa_claim(
            &IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(5) },
            &prog
        )
        .is_ok());
        assert!(check_ipa_claim(
            &IpaClaim::FormalValue { proc: ProcId(1), formal: 0, value: AbstractValue::Const(6) },
            &prog
        )
        .is_err());
    }

    #[test]
    fn a_summary_claim_is_re_derived() {
        let prog = IpaProgram::new(
            vec![],
            vec![proc_with(
                "p",
                vec!["a"],
                vec![IpaStmt::Assign { target: Slot::Local(0), value: IpaExpr::Read(Slot::Param(0)) }],
            )],
            ProcId(0),
        );
        assert!(check_ipa_claim(
            &IpaClaim::Summary {
                proc: ProcId(0),
                read: vec![Slot::Param(0)],
                write: vec![Slot::Local(0)],
            },
            &prog
        )
        .is_ok());
    }
}