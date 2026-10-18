//! Phase 8 — Interprocedural analysis (Wolfe Chapter 8.5, with §8.3 and §8.4).

pub mod callgraph;
pub mod certificate;
pub mod compose;
pub mod constprop;
pub mod context;
pub mod io_effects;
pub mod mod_analysis;
pub mod program;
pub mod pure;
pub mod summaries;

pub use callgraph::{build as build_call_graph, tarjan_scc, ArgFlow, CallGraph};
pub use certificate::{check_ipa_claim, interprocedural_certificate, IpaClaim};
pub use compose::{compose_into, map_symbol};
pub use constprop::{analyze as analyze_constprop, AbstractValue, JumpFn};
pub use context::{refine as refine_context, ArgShape, ContextKey, MAX_CONTEXTS};
pub use io_effects::{has_io, io_ops, must_serialize, summaries as io_summaries, IoOp, IoSummary};
pub use mod_analysis::{analyze as analyze_mod, local_writes, ModAnalysis, ModVertex};
pub use program::{Call, Callee, IpaExpr, IpaProgram, IpaStmt, IoKind, Proc, ProcId, Slot};
pub use pure::classify as classify_purity;
pub use summaries::{compute as compute_summaries, ProcSummary};
