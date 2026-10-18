//! SPP Layer 1 — Effect Summaries (stub for independent build)
//!
//! Every operation carries an effect summary:
//! READ, WRITE, ALLOCATE, THROW, BLOCK, ATOMIC, IO, UNKNOWN
//!
//! Parallelization legality incorporates them.

use crate::layer1::types::EffectSummary;
use crate::common::LoopStub;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, HashMap};

/// Effect kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    Read,
    Write,
    Allocate,
    Throw,
    Block,
    Atomic,
    Io,
    Unknown,
}

/// Effect with location information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub location: String,
    pub loop_level: Option<usize>,
    pub is_conditional: bool,
}

/// Effect summary for a loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEffectSummary {
    pub loop_hash: String,
    pub effects: Vec<Effect>,
    pub reads: HashSet<String>,
    pub writes: HashSet<String>,
    pub allocates: HashSet<String>,
    pub throws: bool,
    pub blocks: bool,
    pub atomic: bool,
    pub io: bool,
    pub unknown: bool,
}

impl LoopEffectSummary {
    pub fn new(loop_hash: String) -> Self {
        Self {
            loop_hash,
            effects: Vec::new(),
            reads: HashSet::new(),
            writes: HashSet::new(),
            allocates: HashSet::new(),
            throws: false,
            blocks: false,
            atomic: false,
            io: false,
            unknown: false,
        }
    }

    pub fn add_effect(&mut self, effect: Effect) {
        let location = effect.location.clone();
        match effect.kind {
            EffectKind::Read => { self.reads.insert(location); }
            EffectKind::Write => { self.writes.insert(location); }
            EffectKind::Allocate => { self.allocates.insert(location); }
            EffectKind::Throw => { self.throws = true; }
            EffectKind::Block => { self.blocks = true; }
            EffectKind::Atomic => { self.atomic = true; }
            EffectKind::Io => { self.io = true; }
            EffectKind::Unknown => { self.unknown = true; }
        }
        self.effects.push(effect);
    }

    pub fn is_parallel_safe(&self) -> bool {
        !self.unknown && !self.throws && !self.blocks
    }

    pub fn can_fuse_with(&self, other: &LoopEffectSummary) -> bool {
        for w in &self.writes {
            if other.writes.contains(w) {
                return false;
            }
        }
        if self.throws || other.throws { return false; }
        if self.blocks || other.blocks { return false; }
        true
    }
}

/// Effect summary analyzer (stub)
pub struct EffectAnalyzer;

impl EffectAnalyzer {
    pub fn new(_loop_: LoopStub) -> Self {
        Self
    }

    /// Analyze effects of a loop
    pub fn analyze(&self) -> LoopEffectSummary {
        LoopEffectSummary::new("stub".into())
    }
}

/// Entry point for effect analysis
pub fn analyze_effects(_loop_: &LoopStub) -> EffectSummary {
    EffectSummary {
        reads: vec![],
        writes: vec![],
        allocates: vec![],
        throws: false,
        blocks: false,
        atomic: false,
        io: false,
        unknown: false,
    }
}

/// Library effect summaries
pub mod library_effects {
    use super::*;

    pub fn get_library_summary(func_name: &str) -> Option<LoopEffectSummary> {
        let lower = func_name.to_lowercase();

        if lower == "memcpy" || lower == "memmove" {
            let mut s = LoopEffectSummary::new("libc::memcpy".into());
            s.add_effect(Effect { kind: EffectKind::Read, location: "src".into(), loop_level: None, is_conditional: false });
            s.add_effect(Effect { kind: EffectKind::Write, location: "dst".into(), loop_level: None, is_conditional: false });
            return Some(s);
        }
        if lower == "memset" {
            let mut s = LoopEffectSummary::new("libc::memset".into());
            s.add_effect(Effect { kind: EffectKind::Write, location: "dst".into(), loop_level: None, is_conditional: false });
            return Some(s);
        }
        if lower == "malloc" || lower == "calloc" || lower == "realloc" {
            let mut s = LoopEffectSummary::new("libc::malloc".into());
            s.add_effect(Effect { kind: EffectKind::Allocate, location: "heap".into(), loop_level: None, is_conditional: false });
            return Some(s);
        }
        if lower == "free" {
            let mut s = LoopEffectSummary::new("libc::free".into());
            s.add_effect(Effect { kind: EffectKind::Write, location: "heap".into(), loop_level: None, is_conditional: false });
            return Some(s);
        }
        if lower.contains("read") || lower.contains("write") || lower.contains("send") || lower.contains("recv") {
            let mut s = LoopEffectSummary::new(format!("libc::{}", func_name));
            s.io = true;
            s.blocks = true;
            return Some(s);
        }
        if lower.contains("lock") || lower.contains("mutex") || lower.contains("cond") || lower.contains("sem") {
            let mut s = LoopEffectSummary::new(format!("pthread::{}", func_name));
            s.blocks = true;
            s.atomic = true;
            return Some(s);
        }
        if lower.starts_with("atomic_") || lower.starts_with("__atomic_") {
            let mut s = LoopEffectSummary::new(format!("atomic::{}", func_name));
            s.atomic = true;
            return Some(s);
        }
        if lower.contains("sin") || lower.contains("cos") || lower.contains("sqrt") ||
           lower.contains("log") || lower.contains("exp") || lower.contains("pow") {
            let mut s = LoopEffectSummary::new(format!("math::{}", func_name));
            return Some(s);
        }

        None
    }

    pub fn has_known_effects(func_name: &str) -> bool {
        get_library_summary(func_name).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_stub() {
        let loop_ = LoopStub::new(1);
        let effects = analyze_effects(&loop_);
        assert!(effects.reads.is_empty());
    }

    #[test]
    fn test_library_effects() {
        assert!(library_effects::has_known_effects("memcpy"));
        assert!(library_effects::has_known_effects("malloc"));
        assert!(library_effects::has_known_effects("pthread_mutex_lock"));
    }
}