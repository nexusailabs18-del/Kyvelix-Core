//! # ScalarDD — scalar dependence analysis (Wolfe §6.7, Figure 6.21)
//!
//! Reaching definitions say *which* assignment a read may see; they say
//! nothing about *when*. An optimiser deciding whether it may hoist, re-order
//! or distribute a loop needs the stronger fact: the **direction** (same
//! iteration, or a previous one) and the **distance** (how many iterations
//! back). Wolfe's ScalarDD derives both from the chain graph alone — no data
//! is executed and no constraint system is solved — by walking each use's
//! chain, classifying every definition it meets, and splitting the result
//! into a loop-independent set and a set of per-loop carried dependences.
//!
//! ## The algorithm (Wolfe §6.7.2, Figure 6.21)
//!
//! ```text
//! ScalarDD(cfg, chain, phi_chain):
//!     for each loop-header-φ h: Reaching[h] := ∅; Self[h] := false
//!     for each use u: FindDependence(Chain[u], u)
//!
//! FindDependence(d, u):                        // one walk per use
//!     if Marked[d] = u: return
//!     Marked[d] := u
//!     if d is a loop-header-φ for variable v:  // d lives at v's own header
//!         FindDependence(φ-Chain[d][preheader], u)   // the entry value
//!         if Reaching[d] = ∅:
//!             FindReaching(φ-Chain[d][back edge], d) // what the loop feeds back
//!         if Self[d]: for c ∈ Reaching[d]: Add(c → u, (<))
//!         else:       for c ∈ Reaching[d]: Add(c → u, (1))
//!     else for each incoming link d' of d:
//!         if d' is a definition: Add(d' → u, (0))
//!         else FindDependence(d', u)           // d' is another φ-term
//!
//! FindReaching(d, h):                          // a walk per header, memoised
//!     if MarkedReaching[d] = h: return
//!     MarkedReaching[d] := h
//!     if d = h: Self[h] := true
//!     else if d is a φ-term: for each incoming link d': FindReaching(d', h)
//!     else: Reaching[h] := Reaching[h] + {d}
//! ```
//!
//! ## The four situations (Wolfe §6.7.1)
//!
//! | Situation | Here | Meaning |
//! |---|---|---|
//! | `(0)` | [`ScalarDistance::Zero`] | only the current iteration |
//! | `(1)` | [`ScalarDistance::One`] | only one previous iteration |
//! | `(<)` | [`ScalarDistance::AnyBefore`] | any previous iteration |
//! | `(≤)` | [`ScalarDistance::AnyAtOrBefore`] | current or any previous |
//!
//! All four *emerge* from the walk; none is asserted. A definition the walk
//! meets directly is read in the same iteration (`(0)`). A definition inside
//! `Reaching[h]` is read one iteration later (`(1)`), or an unknown number of
//! iterations later when `Self[h]` holds (`(<)`). When one definition is both
//! met directly **and** carried, the two facts merge by
//! [`ScalarDistance::join`] into `(≤)`.
//!
//! ## The discipline
//!
//! Nothing here is carried in a certificate verbatim: a Phase 4 certificate
//! carries the CFG, and the checker re-runs [`scalar_dd`] from scratch,
//! exactly as it does for [`crate::fud::induct`]. [`project_dd_facts`] is the
//! form the certificate consumes, and it is fully derived from the re-run
//! result — so edited facts cannot survive the checker's recomputation.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::ir::cfg::CfgIndex;
use crate::ir::scalar::{DefId, PhiId, UseId};
use crate::ir::CFG;

use super::chain::{ChainLink, FudChain};
use super::dominators::{compute_dominators, DominatorTree};
use super::FudChains;

/// How far back a definition reaches the sink, in the four situations of
/// Wolfe §6.7.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ScalarDistance {
    /// `(0)` — only the current iteration.
    Zero,
    /// `(1)` — exactly one previous iteration.
    One,
    /// `(<)` — any previous iteration, with no fixed distance.
    AnyBefore,
    /// `(≤)` — the current or any previous iteration.
    AnyAtOrBefore,
}

impl ScalarDistance {
    /// The merge of two observations of the same dependence: the least
    /// situation that contains both. `Zero ⊔ One` is `(≤)`; `One ⊔ (<)` is
    /// `(<)`; `AnyAtOrBefore` absorbs.
    pub fn join(self, other: Self) -> Self {
        use ScalarDistance::*;
        if self == other {
            return self;
        }
        match (self, other) {
            (AnyAtOrBefore, _) | (_, AnyAtOrBefore) => AnyAtOrBefore,
            (AnyBefore, _) | (_, AnyBefore) => AnyBefore,
            (Zero, One) | (One, Zero) => AnyAtOrBefore,
            (Zero, Zero) | (One, One) => unreachable!("equal operands handled above"),
        }
    }

    /// The concrete distance, when the situation pins one down.
    pub fn concrete(self) -> Option<i64> {
        match self {
            ScalarDistance::Zero => Some(0),
            ScalarDistance::One => Some(1),
            _ => None,
        }
    }

    /// The certificate spelling of the situation.
    pub fn as_str(self) -> &'static str {
        match self {
            ScalarDistance::Zero => "(0)",
            ScalarDistance::One => "(1)",
            ScalarDistance::AnyBefore => "(<)",
            ScalarDistance::AnyAtOrBefore => "(<=)",
        }
    }
}

/// The direction of a dependence.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum DependenceKind {
    /// Write → read: the sink reads what the source writes.
    Flow,
    /// Read → write: the sink overwrites a value the source reads.
    Anti,
    /// Write → write: the sink overwrites the source's value.
    Output,
    /// Read → read: both read the same value.
    Input,
    /// Control: the sink's execution is governed by a predicate the source
    /// computes (Phase 7; never emitted here).
    Control,
}

impl DependenceKind {
    /// The certificate `dep_type` string for this direction.
    pub fn as_str(self) -> &'static str {
        match self {
            DependenceKind::Flow => "flow",
            DependenceKind::Anti => "anti",
            DependenceKind::Output => "output",
            DependenceKind::Input => "input",
            DependenceKind::Control => "control",
        }
    }
}

/// One end of a dependence: a concrete program point.
///
/// `Phi` denotes the *left-hand side* of a φ-term — the write standing for
/// the merge of the incoming values.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum DdPoint {
    /// A statement definition (a write).
    Def(DefId),
    /// The LHS of a φ-term (a write).
    Phi(PhiId),
    /// A use (a read).
    Use(UseId),
}

impl DdPoint {
    /// `true` for reads (uses), `false` for writes (defs and φ-LHS).
    pub fn is_read(self) -> bool {
        matches!(self, DdPoint::Use(_))
    }
}

/// Where a dependence endpoint lives, in printable form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Loc {
    /// The statement index, for a use or a def. `None` for a φ-term, which is
    /// identified by `(block, variable)`.
    pub stmt: Option<usize>,
    /// Index of the variable the endpoint writes or reads.
    pub variable: usize,
    pub block: usize,
    /// `false` for defs/uses, `true` for φ-terms.
    pub is_phi: bool,
    /// `true` when the location is read (`anti`/`input` sinks, `anti` sources).
    pub is_read: bool,
}

/// A dependence edge: an ordered pair of endpoints plus its distance vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub src: Loc,
    pub dst: Loc,
    /// Direction of the edge.
    pub kind: DependenceKind,
    /// Distances, one entry per loop nesting level, outermost first. `Some(0)`
    /// is same-iteration; `None` is "carried an unknown number of iterations"
    /// (an empty vector means the dependence is loop-free and straight-line).
    pub distance: Vec<Option<i64>>,
    /// Loops this edge was found through, outermost first.
    pub loops: Vec<usize>,
}

impl Edge {
    /// True when no entry of the distance vector is non-zero: the edge runs
    /// entirely within one iteration (or one unrolled pass).
    pub fn is_local(&self) -> bool {
        self.distance.iter().all(|d| d.map_or(false, |v| v == 0))
    }
}

/// The loops a dependence crosses, innermost loop first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopSource {
    /// Loop-free, straight-line code.
    None,
    /// Loops enclosing the endpoints, as header block indices (innermost
    /// first). The dependence runs within one iteration of all of them.
    Enclosing(Vec<usize>),
    /// A loop-carried dependence: the loops it crosses (innermost first) plus
    /// the latch block whose back edge carries it.
    Carried { loops: Vec<usize>, back_edge: usize },
}

/// A natural loop: header plus the blocks that flow back to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopInfo {
    /// The loop header block (the back-edge target).
    pub header: usize,
    /// The block ending in the back edge (the loop's latch).
    pub latch: usize,
    /// The blocks of the loop body, sorted, header included.
    pub body: Vec<usize>,
}

impl LoopInfo {
    /// True when `b` is part of this loop.
    pub fn contains(&self, b: usize) -> bool {
        self.body.binary_search(&b).is_ok()
    }

    /// True when `self` is the same loop as, or an outer loop of, `inner`.
    pub fn encloses(&self, inner: &LoopInfo) -> bool {
        self.body.len() >= inner.body.len()
            && self.header != inner.header
            && inner.body.iter().all(|b| self.contains(*b))
    }
}

/// A dependence between scalar program points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScalarDependence {
    /// The earlier access.
    pub src: DdPoint,
    /// The later access.
    pub dst: DdPoint,
    pub kind: DependenceKind,
    /// The Wolfe situation of the dependence (the four directions of §6.7.1).
    pub situation: ScalarDistance,
    /// Per-loop distances, outermost level first. Negative components are
    /// legal and are kept as-is (the sink may precede the source in the
    /// loop order).
    pub distance: Vec<Option<i64>>,
    pub source: Loc,
    pub sink: Loc,
    pub loops: LoopSource,
    /// A read of the loop counter (or accumulator) inside the loop body.
    pub is_read: bool,
    /// A write to the loop counter (or accumulator) inside the loop body.
    pub is_write: bool,
}

impl ScalarDependence {
    /// True when the edge runs entirely within one iteration.
    pub fn is_local(&self) -> bool {
        self.distance.iter().all(|d| d.map_or(false, |v| v == 0))
    }

    /// True when some loop carries this dependence.
    pub fn is_carried(&self) -> bool {
        !self.is_local()
    }

    /// The nesting level of the leftmost non-zero distance component
    /// (Wolfe §5.1); the vector length when the edge is local. A `None`
    /// component (`<`) is non-zero: it is carried.
    pub fn level(&self) -> usize {
        self.distance
            .iter()
            .position(|d| !matches!(d, Some(0)))
            .unwrap_or(self.distance.len())
    }
}

/// CFG block indices in a deterministic DFS pre-order from `entry`, following
/// successors in stored order. A pre-order index is a stable statement order
/// key: everything in a block has the same index, and ties inside a block are
/// broken by statement position.
pub fn block_preorder(cfg: &CFG) -> HashMap<usize, usize> {
    let mut order = HashMap::new();
    let mut stack = vec![cfg.entry];
    while let Some(b) = stack.pop() {
        if order.contains_key(&b) {
            continue;
        }
        let idx = order.len();
        order.insert(b, idx);
        for s in cfg.blocks[b].succs.iter().rev() {
            if !order.contains_key(s) {
                stack.push(*s);
            }
        }
    }
    order
}

/// A back edge `<s, h>`: `h` dominates `s`, so the edge closes a natural loop.
pub fn find_back_edges(cfg: &CFG, dom: &DominatorTree) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for s in 0..cfg.blocks.len() {
        for &h in &cfg.blocks[s].succs {
            if dom.dominates(h, s) {
                out.push((s, h));
            }
        }
    }
    out.sort();
    out
}

/// The natural loops of `cfg`, **one per back edge**, ordered by `(header,
/// latch)`. A loop is `{header}` plus every block that reaches the latch on a
/// path that avoids the header (Wolfe Figure 6.21's `natural_loop`).
///
/// Note that the list is *not* one per distinct loop: a header with two back
/// edges appears twice, once per latch, with two different bodies. Anything that
/// treats an entry as a nesting **level** must deduplicate by header first —
/// which is what [`enclosing_loops`] does.
pub fn find_loops(cfg: &CFG, dom: &DominatorTree) -> Vec<LoopInfo> {
    let mut loops: Vec<LoopInfo> = Vec::new();
    for (latch, header) in find_back_edges(cfg, dom) {
        let mut body = BTreeSet::from([header]);
        let mut stack = vec![latch];
        while let Some(b) = stack.pop() {
            if !body.insert(b) {
                continue;
            }
            for &p in &cfg.blocks[b].preds {
                if p != header {
                    stack.push(p);
                }
            }
        }
        body.insert(header);
        loops.push(LoopInfo {
            header,
            latch,
            body: body.into_iter().collect(),
        });
    }
    loops.sort_by_key(|l| (l.header, l.latch));
    loops
}

/// The loops enclosing block `b`, innermost first.
///
/// Collapsed **by header**. `find_loops` produces one [`LoopInfo`] per back edge,
/// so a header with two latches is two entries describing the *same* loop with two
/// different bodies. A nesting level is a loop, not a back edge: listing the header
/// twice would ask a dependence for two independent distances around one loop, and
/// no execution could satisfy both. The collapse happens **before** the sort, using
/// the widest body seen for that header, because the two entries of a multi-latch
/// header have different sizes and would not necessarily sort adjacent.
pub fn enclosing_loops(loops: &[LoopInfo], b: usize) -> Vec<usize> {
    // The widest body per header — the loop as broad as it ever is, which is the
    // one that puts it in the right nesting position.
    let width = |h: usize| {
        loops
            .iter()
            .filter(|l| l.header == h)
            .map(|l| l.body.len())
            .max()
            .unwrap_or(0)
    };
    let mut outer: Vec<usize> = loops
        .iter()
        .filter(|l| l.contains(b))
        .map(|l| l.header)
        .collect();
    // A loop that contains another is wider; sorting by descending width puts the
    // innermost header first.
    outer.sort_by_key(|h| std::cmp::Reverse(width(*h)));
    outer.dedup();
    outer
}

/// The full result of one Wolfe ScalarDD run (Figure 6.21): the precise flow
/// edges, the pairwise anti/output/input edges, and the intermediate tables
/// (`Reaching[]`, `Self[]`) the checker may inspect.
#[derive(Debug, Clone)]
pub struct ScalarDD {
    /// Every dependence edge, sorted by (source, sink).
    pub edges: Vec<Edge>,
    /// The loop headers, sorted.
    pub headers: BTreeSet<usize>,
    /// The natural loops, outermost-first.
    pub loops: Vec<LoopInfo>,
    /// Header φ-terms whose incoming set contains themselves (§6.7.2).
    pub self_phi: BTreeSet<PhiId>,
    /// The `Reaching[]` table: header φ → the definitions that flow into it
    /// from the back edge, sorted.
    pub reaching: HashMap<PhiId, Vec<ChainLink>>,
    /// Flow dependences as `def → use → situation`, the four values of
    /// Wolfe §6.7.1.
    pub situations: BTreeMap<(DefId, UseId), ScalarDistance>,
}

impl ScalarDD {
    /// Run Wolfe's ScalarDD (Figure 6.21) over the φ-annotated `cfg` and its
    /// forward chains.
    pub fn run(cfg: &CFG, chains: &FudChains) -> ScalarDD {
        let dom = compute_dominators(cfg);
        let loops = find_loops(cfg, &dom);
        let headers: BTreeSet<usize> = loops.iter().map(|l| l.header).collect();
        let idx = cfg.index();

        let back_edges: BTreeSet<(usize, usize)> = find_back_edges(cfg, &dom).into_iter().collect();
        let mut walk = Walk {
            cfg,
            idx: &idx,
            chain: &chains.chain,
            loops: &loops,
            headers: &headers,
            back_edges,
            marked: HashMap::new(),
            marked_reaching: HashMap::new(),
            reaching: HashMap::new(),
            self_phi: BTreeSet::new(),
            situations: BTreeMap::new(),
            carriers: BTreeMap::new(),
        };
        walk.flow_dependences();
        let edges = walk.edges();
        let self_phi = std::mem::take(&mut walk.self_phi);
        let reaching = std::mem::take(&mut walk.reaching);
        let situations = std::mem::take(&mut walk.situations);
        drop(walk);
        ScalarDD {
            edges,
            headers,
            loops,
            self_phi,
            reaching,
            situations,
        }
    }
}

/// One use's walk of Figure 6.21. The state (`Marked`, `MarkedReaching`,
/// `Reaching`, `Self`) is exactly the algorithm's global tables; `Marked`
/// stores the use that last visited a link, which is how the figure resets
/// its visited set for free.
struct Walk<'a> {
    cfg: &'a CFG,
    idx: &'a CfgIndex,
    chain: &'a FudChain,
    loops: &'a [LoopInfo],
    headers: &'a BTreeSet<usize>,
    back_edges: BTreeSet<(usize, usize)>,
    marked: HashMap<ChainLink, UseId>,
    marked_reaching: HashMap<ChainLink, PhiId>,
    reaching: HashMap<PhiId, Vec<ChainLink>>,
    self_phi: BTreeSet<PhiId>,
    situations: BTreeMap<(DefId, UseId), ScalarDistance>,
    /// (def, use) → the header φ whose loop carries the dependence.
    carriers: BTreeMap<(DefId, UseId), PhiId>,
}

impl<'a> Walk<'a> {
    fn add(&mut self, src: ChainLink, u: UseId, s: ScalarDistance, carrier: Option<PhiId>) {
        let ChainLink::Def(d) = src else { return };
        let key = (d, u);
        let merged = match self.situations.get(&key) {
            Some(prev) => prev.join(s),
            None => s,
        };
        self.situations.insert(key, merged);
        if let Some(h) = carrier {
            self.carriers.entry(key).or_insert(h);
        }
    }

    /// Wolfe Figure 6.21, `FindDependence`.
    fn find_dependence(&mut self, d: ChainLink, u: UseId) {
        if self.marked.get(&d) == Some(&u) {
            return;
        }
        self.marked.insert(d, u);
        if let ChainLink::Phi(p) = d {
            let header_block = self.idx.phis[p.index()].block;
            if self.headers.contains(&header_block) {
                // The value coming in from outside the loop.
                for link in self.links_from(d, false) {
                    self.find_dependence(link, u);
                }
                if self.reaching[&p].is_empty() {
                    let seeds: Vec<ChainLink> = self.links_from(d, true);
                    for link in seeds {
                        self.find_reaching(link, p);
                    }
                }
                let carried: Vec<ChainLink> = self.reaching[&p].clone();
                let is_self = self.self_phi.contains(&p);
                for c in carried {
                    let s = if is_self {
                        ScalarDistance::AnyBefore
                    } else {
                        ScalarDistance::One
                    };
                    self.add(c, u, s, Some(p));
                }
                return;
            }
        }
        // `if d is a definition: Add(d → u, (0))` — a definition the use sees
        // is read in the same pass; the definitions it overwrote reach the
        // use only through a header φ, which the branch above has handled.
        // Otherwise `d` is a non-header φ: follow its incoming links.
        if let ChainLink::Def(_) = d {
            self.add(d, u, ScalarDistance::Zero, None);
            return;
        }
        for link in self.links_of(d) {
            self.find_dependence(link, u);
        }
    }

    /// Wolfe Figure 6.21, `FindReaching`.
    fn find_reaching(&mut self, d: ChainLink, h: PhiId) {
        if self.marked_reaching.get(&d) == Some(&h) {
            return;
        }
        self.marked_reaching.insert(d, h);
        if d == ChainLink::Phi(h) {
            self.self_phi.insert(h);
            return;
        }
        match d {
            ChainLink::Phi(p) => {
                let links = self.links_of(d);
                for link in links {
                    let _ = p;
                    self.find_reaching(link, h);
                }
            }
            ChainLink::Def(decl) => {
                // `add d to Reaching[h]`, and **stop**. Figure 6.21 continues
                // with `if d is a nonkilling definition: FindReaching(Chain[d])`
                // — and for scalars every definition is killing
                // (`crate::fud::reaching`), so that branch never runs. Following
                // `def_to_prev` here collected the definitions the back-edge one
                // *overwrote* into `Reaching[h]`, and every one of them then
                // produced a `flow(def → use)` claim for a def that cannot reach
                // the header φ: the last store before the back edge is the only
                // one that survives into the next trip.
                let _ = decl;
                self.reaching.entry(h).or_default().push(d);
            }
        }
    }

    /// Every link `d` flows into: a φ's incoming, or a def's overwritten
    /// definition (the `Save`/`φ_def` chain).
    fn links_of(&self, d: ChainLink) -> Vec<ChainLink> {
        match d {
            ChainLink::Phi(p) => self.chain.phi_incoming[&p]
                .iter()
                .filter_map(|l| *l)
                .collect(),
            ChainLink::Def(x) => match self.chain.def_to_prev.get(&x) {
                Some(Some(prev)) => vec![ChainLink::Def(*prev)],
                _ => Vec::new(),
            },
        }
    }

    /// The incoming links of header φ `d` from the back edges of its loop
    /// (`back = true`) or from outside the loop (`back = false`).
    fn links_from(&self, d: ChainLink, back: bool) -> Vec<ChainLink> {
        let ChainLink::Phi(p) = d else { return Vec::new() };
        let site = self.idx.phis[p.index()];
        let preds = &self.cfg.blocks[site.block].preds;
        let links = &self.chain.phi_incoming[&p];
        let mut out = Vec::new();
        for (i, pred) in preds.iter().enumerate() {
            let is_back = self.back_edges.contains(&(*pred, site.block));
            if is_back != back {
                continue;
            }
            // Slot `i` is the one that corresponds to *this* predecessor, so it
            // is indexed before anything is read. The old form opened with
            // `if i >= links.len() { break }`: on a chain shorter than the
            // predecessor list it silently abandoned *every* remaining edge,
            // including back edges that carry the loop's own definitions, which
            // is a missing-dependence unsoundness rather than a precision loss.
            debug_assert!(
                links.len() == preds.len(),
                "header φ of variable {} in block {} has {} predecessors but {} chain slots",
                site.var,
                site.block,
                preds.len(),
                links.len()
            );
            let Some(link) = links.get(i).copied().flatten() else {
                continue;
            };
            // A `Algorithm Chain` link for a φ of variable `v` always defines or
            // merges `v`. Assert it, and skip rather than trust it in release
            // builds, so a hand-built or future-built `FudChains` that mixed up
            // its slots cannot smuggle another variable's definition into this
            // loop's dependence set.
            if self.link_matches_var(link, site.var) {
                out.push(link);
            }
        }
        out
    }

    /// Does `link` carry the variable `var` — i.e. is it a definition of it, or
    /// a φ merging it?
    fn link_matches_var(&self, link: ChainLink, var: usize) -> bool {
        match link {
            ChainLink::Phi(q) => self.idx.phis[q.index()].var == var,
            ChainLink::Def(d) => self.idx.defs[d.index()].var == var,
        }
    }
}

impl<'a> Walk<'a> {
    /// `Add(source → sink, (0))` for the flow direction happens inside the
    /// walk; the anti/output/input edges are pairwise, over same-variable
    /// accesses inside one block, in program order.
    fn pairwise(&mut self) -> Vec<Edge> {
        let mut out = Vec::new();
        for (b, block) in self.cfg.blocks.iter().enumerate() {
            // (rank, kind, point, var). Reads rank before writes of the same
            // statement: a statement reads its operands, then writes its LHS.
            let mut accesses: Vec<(u32, u32, u32, DdPoint, usize)> = Vec::new();
            for (si, stmt) in block.stmts.iter().enumerate() {
                for ui in 0..stmt.uses.len() {
                    if let Some(u) = self.stmt_use(b, si, ui) {
                        let site = self.idx.uses[u.index()];
                        accesses.push((
                            site.stmt as u32,
                            0,
                            site.order as u32,
                            DdPoint::Use(u),
                            site.var,
                        ));
                    }
                }
                for di in 0..stmt.defs.len() {
                    if let Some(d) = self.stmt_def(b, si, di) {
                        let site = self.idx.defs[d.index()];
                        accesses.push((
                            site.stmt as u32,
                            1,
                            site.order as u32,
                            DdPoint::Def(d),
                            site.var,
                        ));
                    }
                }
            }
            accesses.sort();
            for i in 0..accesses.len() {
                for j in (i + 1)..accesses.len() {
                    let (.., src, v1) = &accesses[i];
                    let (.., dst, v2) = &accesses[j];
                    if v1 != v2 {
                        continue;
                    }
                    let kind = match (src.is_read(), dst.is_read()) {
                        (false, true) => continue, // flow: the walk owns it
                        (false, false) => DependenceKind::Output,
                        (true, false) => DependenceKind::Anti,
                        (true, true) => {
                            // Input only when both reads see the same value.
                            if self.reached_by(*src) != self.reached_by(*dst) {
                                continue;
                            }
                            DependenceKind::Input
                        }
                    };
                    let (src_loc, dst_loc) = (self.loc_of(*src), self.loc_of(*dst));
                    let distance = self.zeros(b);
                    out.push(Edge {
                        src: src_loc,
                        dst: dst_loc,
                        kind,
                        distance,
                        loops: self.nests(b),
                    });
                }
            }
        }
        out
    }

    fn stmt_use(&self, block: usize, stmt: usize, order: usize) -> Option<UseId> {
        self.idx
            .uses
            .iter()
            .position(|s| s.block == block && s.stmt == stmt && s.order == order)
            .map(|i| UseId(i))
    }

    fn stmt_def(&self, block: usize, stmt: usize, order: usize) -> Option<DefId> {
        self.idx
            .defs
            .iter()
            .position(|s| s.block == block && s.stmt == stmt && s.order == order)
            .map(|i| DefId(i))
    }

    fn reached_by(&self, p: DdPoint) -> Option<ChainLink> {
        match p {
            DdPoint::Use(u) => self.chain.use_to_def.get(&u).copied(),
            DdPoint::Def(d) => Some(ChainLink::Def(d)),
            DdPoint::Phi(x) => Some(ChainLink::Phi(x)),
        }
    }

    /// A distance vector of zeros as long as `block`'s loop nest.
    fn zeros(&self, block: usize) -> Vec<Option<i64>> {
        vec![Some(0); self.enclosing(block).len()]
    }

    /// The loops enclosing `block`, innermost first.
    fn enclosing(&self, block: usize) -> Vec<usize> {
        enclosing_loops(self.loops, block)
    }

    /// The same nest, printed outermost first.
    fn nests(&self, block: usize) -> Vec<usize> {
        let mut v = self.enclosing(block);
        v.reverse();
        v
    }

    fn loc_of(&self, p: DdPoint) -> Loc {
        let (stmt, variable, block, is_phi, is_read) = match p {
            DdPoint::Use(u) => {
                let s = &self.idx.uses[u.index()];
                (Some(s.stmt), s.var, s.block, false, true)
            }
            DdPoint::Def(d) => {
                let s = &self.idx.defs[d.index()];
                (Some(s.stmt), s.var, s.block, false, false)
            }
            DdPoint::Phi(x) => {
                let s = &self.idx.phis[x.index()];
                (None, s.var, s.block, true, false)
            }
        };
        Loc {
            stmt,
            variable,
            block,
            is_phi,
            is_read,
        }
    }

    /// The distance vector of a dependence from `src` to `dst`, given the
    /// header φ `carrier` that carries it (if any). Levels are indexed by the
    /// shared nest returned by [`Self::shared_nest`], outermost first.
    fn vector(&self, carrier: Option<PhiId>, nest: &[usize]) -> Vec<Option<i64>> {
        let mut v = vec![Some(0); nest.len()];
        let Some(h) = carrier else {
            return v;
        };
        let header_block = self.idx.phis[h.index()].block;
        if let Some(level) = nest.iter().position(|b| *b == header_block) {
            v[level] = if self.self_phi.contains(&h) { None } else { Some(1) };
        }
        v
    }

    /// Every loop that encloses either endpoint, outermost first. A
    /// definition inside a loop read outside it still crosses that loop, so
    /// the distance vector and the reported loop list cover the same nest.
    fn shared_nest(&self, src_block: usize, dst_block: usize) -> Vec<usize> {
        let mut v = self.nests(dst_block);
        for h in self.nests(src_block) {
            if !v.contains(&h) {
                v.push(h);
            }
        }
        // The wider a loop's body, the further out it sits, so ascending body
        // size lists the loops outermost first. A header with two back edges has
        // two [`LoopInfo`] entries; take the widest, matching [`enclosing_loops`].
        let size = |h: &usize| {
            self.loops
                .iter()
                .filter(|l| l.header == *h)
                .map(|l| l.body.len())
                .max()
                .unwrap_or(0)
        };
        v.sort_by_key(size);
        v
    }
}

impl<'a> Walk<'a> {
    /// The driver of Figure 6.21: seed `Reaching[h]` with the empty set for
    /// every header φ, then walk every use's chain.
    fn flow_dependences(&mut self) {
        for (i, site) in self.idx.phis.iter().enumerate() {
            if self.headers.contains(&site.block) {
                self.reaching.entry(PhiId(i)).or_default();
            }
        }
        for (i, _) in self.idx.uses.iter().enumerate() {
            let u = UseId(i);
            if let Some(&d) = self.chain.use_to_def.get(&u) {
                self.find_dependence(d, u);
            }
        }
    }

    /// The edges of the analysis: the flow dependences collected by the walk,
    /// then the pairwise anti/output/input edges.
    fn edges(&mut self) -> Vec<Edge> {
        let situations = self.situations.clone();
        let carriers = self.carriers.clone();
        let mut out: Vec<Edge> = Vec::new();
        for ((d, u), _situation) in situations {
            let src = self.loc_of(DdPoint::Def(d));
            let dst = self.loc_of(DdPoint::Use(u));
            let carrier = carriers.get(&(d, u)).copied();
            let loops = self.shared_nest(src.block, dst.block);
            // Loop-free straight-line code has no levels, so the vector is
            // empty (printed `[]`).
            let distance = self.vector(carrier, &loops);
            out.push(Edge {
                src,
                dst,
                kind: DependenceKind::Flow,
                distance,
                loops,
            });
        }
        out.append(&mut self.pairwise());
        out.sort_by(|a, b| (&a.src, &a.dst, a.kind).cmp(&(&b.src, &b.dst, b.kind)));
        out
    }
}

/// Run Wolfe's ScalarDD (Figure 6.21) over the φ-annotated `cfg` and its
/// forward chains `chains`.
pub fn scalar_dd(cfg: &CFG, chains: &FudChains) -> ScalarDD {
    ScalarDD::run(cfg, chains)
}

/// One dependence, in the form a Phase 4 certificate carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DdFact {
    pub dep_type: String,
    pub source: Loc,
    pub sink: Loc,
    /// The distance vector, one entry per loop nesting level, outermost first;
    /// `"<"` is an unknown (symbolic) distance.
    pub distance: Vec<String>,
    /// The loops the dependence crosses, outermost first.
    pub loops: Vec<usize>,
}

/// Project a [`ScalarDD`] run onto the fact form a Phase 4 certificate
/// carries. Every fact is derived from the re-run analysis, so the checker
/// recomputes the same facts from the CFG it already verifies.
///
/// The result is a set: sorted by (source, sink, kind, distance, loops) and
/// duplicate-free. Several edges can share one fact when a statement accesses
/// the same variable more than once, since a [`Loc`] identifies an access by
/// `(statement, variable, block)` and not by its position within the
/// statement.
pub fn project_dd_facts(dd: &ScalarDD) -> Vec<DdFact> {
    let mut out: Vec<DdFact> = Vec::new();
    for edge in &dd.edges {
        let distance: Vec<String> = edge
            .distance
            .iter()
            .map(|d| d.map_or_else(|| "<".to_string(), |v| v.to_string()))
            .collect();
        out.push(DdFact {
            dep_type: edge.kind.as_str().to_string(),
            source: edge.src.clone(),
            sink: edge.dst.clone(),
            distance,
            loops: edge.loops.clone(),
        });
    }
    out.sort_by(|a, b| {
        (
            &a.source,
            &a.sink,
            &a.dep_type,
            &a.distance,
            &a.loops,
        )
        .cmp(&(&b.source, &b.sink, &b.dep_type, &b.distance, &b.loops))
    });
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fud::build_fud_chains;
    use crate::fud::cfg::build_cfg_from_loop;
    use crate::ir::stmt::{ScalarExpr, Stmt as LoopStmt, StmtKind};
    use crate::ir::{Access, AccessKind, IndexKind, OffsetValue, ScalarTy, ScalarVar, IV, Loop};

    fn var(n: &str) -> ScalarVar {
        ScalarVar { name: n.into(), ty: ScalarTy::I64 }
    }

    /// The four situations form a join-semilattice: equal operands are idempotent,
    /// `(≤)` is top, `(<)` absorbs the two exact distances, and `(0) ⊔ (1)` is
    /// exactly `(≤)` — the only non-trivial merge.
    #[test]
    fn scalar_distance_join_is_the_specs_four_point_lattice() {
        use ScalarDistance::*;
        // Idempotence.
        for s in [Zero, One, AnyBefore, AnyAtOrBefore] {
            assert_eq!(s.join(s), s);
        }
        // The two exact distances and the two inexact ones agree with Wolfe §6.7.1.
        assert_eq!(Zero.join(One), AnyAtOrBefore);
        assert_eq!(One.join(Zero), AnyAtOrBefore);
        assert_eq!(Zero.join(AnyBefore), AnyBefore);
        assert_eq!(One.join(AnyBefore), AnyBefore);
        assert_eq!(Zero.join(AnyAtOrBefore), AnyAtOrBefore);
        assert_eq!(AnyBefore.join(AnyAtOrBefore), AnyAtOrBefore);
        // Join is commutative on the whole lattice.
        for a in [Zero, One, AnyBefore, AnyAtOrBefore] {
            for b in [Zero, One, AnyBefore, AnyAtOrBefore] {
                assert_eq!(a.join(b), b.join(a), "{a:?} ⊔ {b:?}");
            }
        }
        // Only the two exact situations pin a distance.
        assert_eq!(Zero.concrete(), Some(0));
        assert_eq!(One.concrete(), Some(1));
        assert_eq!(AnyBefore.concrete(), None);
        assert_eq!(AnyAtOrBefore.concrete(), None);
        // The certificate spelling round-trips through the four forms.
        assert_eq!(
            [Zero, One, AnyBefore, AnyAtOrBefore].map(|s| s.as_str()),
            ["(0)", "(1)", "(<)", "(<=)"]
        );
    }

    /// A self-referential accumulator `s = s + i` over a real Phase 9a loop CFG
    /// is a loop-carried dependence, so `scalar_dd` must report an `(1)`
    /// situation. This is the in-file proof that the analysis reads the CFG the
    /// frontend builder emits.
    #[test]
    fn a_self_referential_accumulator_is_carried() {
        let l = Loop {
            depth: 1,
            ivs: vec![IV::new("i", 0, 8, 1)],
            body: vec![Access {
                kind: AccessKind::Write,
                base: "A".into(),
                index: vec![1],
                elem_size: 8,
                offset: OffsetValue::Const(0),
                expr: None,
                indirect: None,
                guard: None,
                range: None,
                index_kind: IndexKind::Affine,
            }],
            // IV first (skipped by name), then the accumulator `s` at scalar 1.
            scalars: vec![var("i"), var("s")],
            stmts: Some(vec![LoopStmt {
                id: 0,
                kind: StmtKind::Assign {
                    target: 1,
                    // s := s + i
                    expr: ScalarExpr::Add(
                        Box::new(ScalarExpr::Scalar(1)),
                        Box::new(ScalarExpr::Iv(0)),
                    ),
                },
            }]),
            recursion: None,
            chase: None,
            assertions: vec![],
        };
        let cfg = build_cfg_from_loop(&l).expect("an IV loop has a CFG");
        let (annot, chains) = build_fud_chains(&cfg);
        let dd = scalar_dd(&annot, &chains);

        assert!(!dd.headers.is_empty(), "the loop has a header");
        assert!(
            dd.situations.values().any(|s| *s == ScalarDistance::One),
            "`s = s + i` is read one iteration later: {:?}",
            dd.situations.values().collect::<Vec<_>>()
        );
    }
}
