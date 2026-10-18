//! Phase 5 §5.1.3 — effect summaries and I/O dependence (Wolfe §8.3).
//!
//! Every statement has an *effect* on the program state: it reads memory,
//! writes memory, performs I/O, or is opaque. Most of those effects are already
//! visible in the array IR (`AccessKind`), but **I/O is not**, and I/O is the
//! effect that most sharply limits reordering: two statements that both touch a
//! device must stay in their original order, however independent their array
//! accesses look.
//!
//! ```text
//! The I/O rule (Wolfe §8.3): if two statements both have Effect::Io,
//! they must be ordered. open/close/rewind/read/write keep their order.
//! ```
//!
//! ## Where the I/O flag comes from
//!
//! The array IR names an array by a free-form `base` string, and nothing in it
//! marks a base as a device. A real frontend knows its I/O library and would
//! pass that table in; this module therefore makes the table explicit
//! ([`IoChannels`]) and ships one *documented convention* as
//! [`IoChannels::standard`]: a base whose name is a standard stream or a
//! file/stream/socket-like prefix. That is a naming convention, not analysis,
//! and the module says so — a caller with real device knowledge should build
//! its own [`IoChannels`] rather than rely on the default.
//!
//! ## Soundness direction
//!
//! [`must_serialize`] returns `true` whenever I/O ordering *might* matter, so it
//! can only ever add a constraint. A caller that forgets a channel gets a
//! weaker-but-still-sound answer ("independent") — which is why the convention
//! is stated rather than hidden: the failure mode is a missed optimization, not
//! a wrong reordering.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::ir::{Access, AccessKind, Loop};

/// The effect a statement has on program state (Wolfe §8.3/§8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Reads memory.
    Read,
    /// Writes memory.
    Write,
    /// Allocates storage.
    Allocate,
    /// May throw.
    Throw,
    /// Reads or writes an external device (Wolfe §8.3).
    Io,
    /// An atomic memory operation.
    Atomic,
    /// Not modelled.
    Unknown,
}

impl Effect {
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
            Effect::Allocate => "allocate",
            Effect::Throw => "throw",
            Effect::Io => "io",
            Effect::Atomic => "atomic",
            Effect::Unknown => "unknown",
        }
    }
}

/// The set of base names (and name prefixes) treated as external devices.
///
/// Built explicitly so the I/O rule never depends on a hidden global: the
/// caller owns the device table, or opts into the documented convention with
/// [`IoChannels::standard`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoChannels {
    names: HashSet<String>,
    prefixes: Vec<String>,
}

impl IoChannels {
    /// An empty table: nothing is I/O. Sound as a *starting point* (it can only
    /// miss constraints), never as a detector.
    pub fn none() -> IoChannels {
        IoChannels { names: HashSet::new(), prefixes: Vec::new() }
    }

    /// The documented convention: standard streams by exact name, plus
    /// file/stream/socket-like bases by prefix.
    pub fn standard() -> IoChannels {
        let names = [
            "stdin", "stdout", "stderr", "console", "read", "write", "open", "close", "rewind",
            "print", "println", "input", "output", "put", "get",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let prefixes = ["io_", "file", "stream", "chan", "sock", "pipe", "dev", "tty"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        IoChannels { names, prefixes }
    }

    /// A table of exact names (no prefixes).
    pub fn from_names<I, S>(names: I) -> IoChannels
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        IoChannels { names: names.into_iter().map(Into::into).collect(), prefixes: Vec::new() }
    }

    /// Add an exact name.
    pub fn with_name(mut self, name: impl Into<String>) -> IoChannels {
        self.names.insert(name.into());
        self
    }

    /// Add a prefix.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> IoChannels {
        self.prefixes.push(prefix.into());
        self
    }

    /// Is `base` an external device under this table?
    pub fn is_io(&self, base: &str) -> bool {
        self.names.contains(base) || self.prefixes.iter().any(|p| base.starts_with(p.as_str()))
    }
}

/// The effect of an `Access` on `channels`.
///
/// An access on a device base has `Effect::Io` regardless of its `kind`: an I/O
/// *read* and an I/O *write* are both device traffic and both must stay ordered.
pub fn effect_of(access: &Access, channels: &IoChannels) -> Effect {
    if channels.is_io(&access.base) {
        return Effect::Io;
    }
    match access.kind {
        AccessKind::Read => Effect::Read,
        AccessKind::Write => Effect::Write,
    }
}

impl Access {
    /// §5.1.3: the I/O effect of this access under the standard channel
    /// convention, if any. See [`IoChannels`] for why the table is explicit.
    pub fn io_effect(&self) -> Option<Effect> {
        if IoChannels::standard().is_io(&self.base) {
            Some(Effect::Io)
        } else {
            None
        }
    }
}

/// The ordered pairs of loop-body statement indices that must not be reordered
/// because both perform I/O (Wolfe §8.3): `(i, j)` with `i < j`.
pub fn io_dependences(loop_: &Loop, channels: &IoChannels) -> Vec<(usize, usize)> {
    let io: Vec<usize> = loop_
        .body
        .iter()
        .enumerate()
        .filter(|(_, a)| effect_of(a, channels) == Effect::Io)
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::new();
    for (k, &i) in io.iter().enumerate() {
        for &j in &io[k + 1..] {
            out.push((i, j));
        }
    }
    out
}

/// §5.1.3's rule as a predicate: must statements `p` and `q` of `loop_` keep
/// their relative order purely because of I/O?
pub fn must_serialize(loop_: &Loop, p: usize, q: usize, channels: &IoChannels) -> bool {
    let (i, j) = if p <= q { (p, q) } else { (q, p) };
    if i == j {
        return false;
    }
    let is_io = |k: usize| {
        loop_
            .body
            .get(k)
            .map(|a| effect_of(a, channels) == Effect::Io)
            .unwrap_or(false)
    };
    is_io(i) && is_io(j)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AccessKind, IndexKind, IV, OffsetValue};

    fn acc(kind: AccessKind, base: &str) -> Access {
        Access {
            kind,
            base: base.into(),
            index: vec![1],
            elem_size: 8,
            offset: OffsetValue::Const(0),
            expr: None,
            indirect: None,
            guard: None,
            range: None,
            index_kind: IndexKind::Affine,
        }
    }

    fn loop_of(body: Vec<Access>) -> Loop {
        Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 4, 1)],
            body,
            recursion: None,
            chase: None,
            assertions: vec![],
            scalars: vec![],
            stmts: None,
        }
    }

    #[test]
    fn ordinary_arrays_are_read_or_write() {
        let ch = IoChannels::standard();
        assert_eq!(effect_of(&acc(AccessKind::Read, "A"), &ch), Effect::Read);
        assert_eq!(effect_of(&acc(AccessKind::Write, "A"), &ch), Effect::Write);
    }

    #[test]
    fn standard_convention_flags_streams_and_devices() {
        let ch = IoChannels::standard();
        for base in ["stdin", "stdout", "stderr", "file1", "io_buf", "sock", "pipe"] {
            assert_eq!(effect_of(&acc(AccessKind::Write, base), &ch), Effect::Io, "{base}");
        }
        // An ordinary array is not I/O, even under the convention.
        assert_eq!(effect_of(&acc(AccessKind::Write, "A"), &ch), Effect::Write);
        assert!(acc(AccessKind::Write, "stdout").io_effect().is_some());
        assert!(acc(AccessKind::Write, "A").io_effect().is_none());
    }

    #[test]
    fn io_pairs_are_all_ordered_pairs() {
        let ch = IoChannels::standard();
        let l = loop_of(vec![
            acc(AccessKind::Read, "stdin"),
            acc(AccessKind::Write, "A"),
            acc(AccessKind::Write, "stdout"),
        ]);
        assert_eq!(io_dependences(&l, &ch), vec![(0, 2)]);
        assert!(must_serialize(&l, 0, 2, &ch));
        assert!(must_serialize(&l, 2, 0, &ch), "order is symmetric in serialization");
        assert!(!must_serialize(&l, 0, 1, &ch), "one side is not I/O");
    }

    #[test]
    fn empty_table_flags_nothing() {
        let ch = IoChannels::none();
        let l = loop_of(vec![acc(AccessKind::Write, "stdout"), acc(AccessKind::Write, "stdin")]);
        assert!(io_dependences(&l, &ch).is_empty());
    }

    #[test]
    fn explicit_names_are_honoured() {
        let ch = IoChannels::from_names(["log"]);
        let l = loop_of(vec![acc(AccessKind::Write, "log"), acc(AccessKind::Write, "A")]);
        assert_eq!(io_dependences(&l, &ch), vec![]);
        let l2 = loop_of(vec![
            acc(AccessKind::Write, "log"),
            acc(AccessKind::Write, "A"),
            acc(AccessKind::Read, "log"),
        ]);
        assert_eq!(io_dependences(&l2, &ch), vec![(0, 2)]);
    }
}