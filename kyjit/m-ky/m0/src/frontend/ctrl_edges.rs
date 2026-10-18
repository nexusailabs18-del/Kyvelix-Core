//! Phase 9 §9.11 — control dependence: post-dominator tree and PDG control edges.
//!
//! §9.1.1's statement reordering has to know which statements a decision
//! *governs*: two statements may be exchanged only when the same decisions
//! govern both and neither governs the other. That relation is the control-
//! dependence half of a program dependence graph, and this module computes it.
//!
//! ## What is built
//!
//! The AST becomes a control-flow graph whose nodes are statement *occurrences*
//! plus three synthetic kinds:
//!
//! * `ENTRY`/`EXIT` — the procedure's entry and exit;
//! * `JOIN` — a point where control merges and no statement sits: an empty
//!   branch arm, or a loop's exit.
//!
//! [`ControlGraph::successors`] exposes the graph,
//! [`ControlGraph::post_dominators`] the full post-dominator sets,
//! [`ControlGraph::immediate_post_dominator`] the tree they induce, and
//! [`ControlGraph::control_edges`] the PDG's control edges:
//!
//! > `X → Y` iff `Y` is a successor of `X` and `Y` is **not** post-dominated by
//! > `ipdom(X)` — the standard definition (Ferrante, Cytron & Warren 1987).
//!
//! ## Deliberate limits, each in the sound direction
//!
//! * Statements are occurrences, not paths: two syntactically identical
//!   statements in different arms are different nodes, which is what a
//!   reordering pass needs — exchanging them is the very thing it weighs.
//! * A nested `FunctionDecl` is not walked. Control dependence is
//!   intraprocedural; a nested procedure has its own graph, built from its own
//!   body.
//! * `try`/`catch` and `switch` are single opaque statements: their internal
//!   control flow is not modelled, so every statement inside one is treated as
//!   governed by that statement — which *adds* dependence (forbids reordering)
//!   rather than removing it.
//! * `do-while` is modelled like `while`, so its body acquires a "may not
//!   execute" path it does not really have. That direction is safe here: it
//!   makes the body's statements dependent on the header, forbidding a
//!   reordering rather than permitting one.

use std::collections::HashSet;

use crate::frontend::ast::Stmt;

/// A node of the control-flow graph.
pub type Node = usize;

/// A dependence edge: `decision` governs `dependent`.
pub type CtrlEdge = (Node, Node);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Entry,
    Exit,
    /// Control merges here and no statement sits here.
    Join,
    /// A statement occurrence.
    Stmt,
}

/// The control-flow graph of one procedure body.
#[derive(Debug, Clone)]
pub struct ControlGraph {
    kind: Vec<Kind>,
    label: Vec<&'static str>,
    succ: Vec<Vec<Node>>,
}

/// The post-dominator relation, in the form the tree is read from.
#[derive(Debug, Clone)]
pub struct PostDominators {
    /// `sets[x]` is the set of nodes every path from `x` to `EXIT` must pass
    /// through, `x` included.
    pub sets: Vec<HashSet<Node>>,
    /// `ipdom[x]` is the immediate post-dominator of `x`: the least element of
    /// `sets[x] \ {x}`, or `None` for `EXIT` itself.
    pub ipdom: Vec<Option<Node>>,
    exit: Node,
}

impl PostDominators {
    /// Whether `d` post-dominates `n` — every path from `n` reaches `EXIT` only
    /// through `d`.
    pub fn post_dominates(&self, d: Node, n: Node) -> bool {
        self.sets[n].contains(&d)
    }

    /// The immediate post-dominator of `n`, if `n` is not the exit.
    pub fn immediate(&self, n: Node) -> Option<Node> {
        self.ipdom[n]
    }

    /// The exit node the tree is rooted at.
    pub fn exit(&self) -> Node {
        self.exit
    }
}

/// Build the control-flow graph of a procedure body.
///
/// `stmts` is one procedure body — not a program, not a function table. Nested
/// functions inside it are not descended into (module docs).
pub fn build(stmts: &[Stmt]) -> ControlGraph {
    let mut b = Builder::new();
    let top = b.seq(stmts);
    b.finish(top)
}

// ---- construction ----------------------------------------------------------

/// A walk position: where control enters, and where it can leave.
///
/// An empty `exits` is meaningful — it is a statement that does not fall through
/// (`return`, `break`, `continue`).
struct Flow {
    entry: Option<Node>,
    exits: Vec<Node>,
}

struct Builder {
    kind: Vec<Kind>,
    label: Vec<&'static str>,
    succ: Vec<Vec<Node>>,
    /// The exit join of each open loop, for `break`.
    loop_exit: Vec<Node>,
    /// The header of each open loop, for `continue`.
    loop_head: Vec<Node>,
    /// `return`/`throw` nodes, wired to `EXIT` once the body is walked.
    to_exit: Vec<Node>,
}

impl Builder {
    const ENTRY: Node = 0;
    const EXIT: Node = 1;

    fn new() -> Builder {
        Builder {
            kind: vec![Kind::Entry, Kind::Exit],
            label: vec!["ENTRY", "EXIT"],
            succ: vec![Vec::new(), Vec::new()],
            loop_exit: Vec::new(),
            loop_head: Vec::new(),
            to_exit: Vec::new(),
        }
    }

    fn node(&mut self, kind: Kind, label: &'static str) -> Node {
        self.kind.push(kind);
        self.label.push(label);
        self.succ.push(Vec::new());
        self.kind.len() - 1
    }

    fn edge(&mut self, from: Node, to: Node) {
        self.succ[from].push(to);
    }

    /// Walk a statement list: its entry, and the nodes control can leave from.
    fn seq(&mut self, stmts: &[Stmt]) -> Flow {
        let mut entry = None;
        let mut exits: Vec<Node> = Vec::new();
        for s in stmts {
            let f = self.one(s);
            if let Some(e) = f.entry {
                entry.get_or_insert(e);
                for o in &exits {
                    self.edge(*o, e);
                }
                exits = f.exits;
            }
            // A statement with no node (an empty block, an empty statement) is
            // transparent: control passes through unchanged.
        }
        Flow { entry, exits }
    }

    /// Walk one statement, or the block that forms a construct's body.
    fn branch(&mut self, s: &Stmt) -> Flow {
        match s.as_block() {
            Some(v) => self.seq(v),
            None => self.one(s),
        }
    }

    fn leaf(&mut self, label: &'static str) -> Flow {
        let n = self.node(Kind::Stmt, label);
        Flow { entry: Some(n), exits: vec![n] }
    }

    fn one(&mut self, s: &Stmt) -> Flow {
        match s {
            Stmt::Block(v, _) => self.seq(v),
            Stmt::Empty(_) => Flow { entry: None, exits: Vec::new() },

            Stmt::If { then, els, .. } => {
                let n = self.node(Kind::Stmt, "if");
                // An empty arm still needs a node to *be* the successor: that is
                // what a `JOIN` is for.
                let mut t = self.branch(then);
                let t_entry = match t.entry {
                    Some(e) => e,
                    None => {
                        let j = self.node(Kind::Join, "empty-arm");
                        t.exits.push(j);
                        j
                    }
                };
                self.edge(n, t_entry);
                let mut exits = t.exits;
                match els {
                    Some(e) => {
                        let f = self.branch(e);
                        let e_entry = match f.entry {
                            Some(x) => x,
                            None => {
                                let j = self.node(Kind::Join, "empty-arm");
                                exits.push(j);
                                j
                            }
                        };
                        self.edge(n, e_entry);
                        exits.extend(f.exits);
                    }
                    // No `else`: the false edge goes to the merge point directly.
                    None => {
                        let j = self.node(Kind::Join, "no-else");
                        self.edge(n, j);
                        exits.push(j);
                    }
                }
                Flow { entry: Some(n), exits }
            }

            // Every loop head is a decision; its body is governed by it.
            Stmt::For { body, .. }
            | Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForIn { body, .. } => {
                let n = self.node(Kind::Stmt, "loop");
                let exit = self.node(Kind::Join, "loop-exit");
                self.loop_head.push(n);
                self.loop_exit.push(exit);
                let mut b = self.branch(body);
                self.loop_head.pop();
                self.loop_exit.pop();
                let b_entry = match b.entry {
                    Some(e) => e,
                    None => {
                        let j = self.node(Kind::Join, "empty-body");
                        b.exits.push(j);
                        j
                    }
                };
                self.edge(n, b_entry);
                self.edge(n, exit); // the loop may run zero times
                if b.entry.is_none() {
                    self.edge(b_entry, n); // an empty body still loops
                }
                for o in b.exits {
                    self.edge(o, n); // the back edge
                }
                Flow { entry: Some(n), exits: vec![exit] }
            }

            Stmt::Break(_, _) => {
                let n = self.node(Kind::Stmt, "break");
                if let Some(e) = self.loop_exit.last().copied() {
                    self.edge(n, e);
                }
                Flow { entry: Some(n), exits: Vec::new() } // does not fall through
            }
            Stmt::Continue(_, _) => {
                let n = self.node(Kind::Stmt, "continue");
                if let Some(h) = self.loop_head.last().copied() {
                    self.edge(n, h);
                }
                Flow { entry: Some(n), exits: Vec::new() }
            }
            Stmt::Return(_, _) | Stmt::Throw(_, _) => {
                let n = self.node(Kind::Stmt, "exit-stmt");
                self.to_exit.push(n);
                Flow { entry: Some(n), exits: Vec::new() }
            }

            // Opaque leaves: a statement whose inside is not modelled.
            Stmt::Try { .. } => self.leaf("try"),
            Stmt::Switch { .. } => self.leaf("switch"),
            Stmt::FunctionDecl { .. } => self.leaf("function"),
            Stmt::Class { .. } => self.leaf("class"),
            Stmt::Let { .. } => self.leaf("let"),
            Stmt::Var { .. } => self.leaf("var"),
            Stmt::Const { .. } => self.leaf("const"),
            Stmt::Expr(_) => self.leaf("expr"),
            Stmt::Import { .. } => self.leaf("import"),
            Stmt::Export { .. } => self.leaf("export"),
        }
    }

    fn finish(mut self, top: Flow) -> ControlGraph {
        match top.entry {
            Some(e) => self.edge(Self::ENTRY, e),
            None => self.edge(Self::ENTRY, Self::EXIT),
        }
        for o in top.exits {
            self.edge(o, Self::EXIT);
        }
        // Taken out first: `self.edge` needs `&mut self`, and a `for` over the
        // field would move it out of `self`.
        let to_exit = std::mem::take(&mut self.to_exit);
        for t in to_exit {
            self.edge(t, Self::EXIT);
        }
        ControlGraph { kind: self.kind, label: self.label, succ: self.succ }
    }
}

// ---- reading the graph ------------------------------------------------------

impl ControlGraph {
    /// The synthetic entry node.
    pub fn entry(&self) -> Node {
        0
    }

    /// The synthetic exit node.
    pub fn exit(&self) -> Node {
        1
    }

    /// How many nodes the graph has.
    pub fn len(&self) -> usize {
        self.kind.len()
    }

    /// Whether the graph has no nodes (never true for a graph `build` returned).
    pub fn is_empty(&self) -> bool {
        self.kind.is_empty()
    }

    /// Is this node a statement occurrence, rather than ENTRY/EXIT/a `JOIN`?
    pub fn is_statement(&self, n: Node) -> bool {
        self.kind[n] == Kind::Stmt
    }

    /// The node's diagnostic name: a statement kind, or its synthetic role.
    pub fn label(&self, n: Node) -> &'static str {
        self.label[n]
    }

    /// Every statement node, in source order.
    pub fn statements(&self) -> Vec<Node> {
        (0..self.len()).filter(|n| self.is_statement(*n)).collect()
    }

    /// The node's successors.
    pub fn successors(&self, n: Node) -> &[Node] {
        &self.succ[n]
    }

    /// Every edge of the graph.
    pub fn edges(&self) -> Vec<CtrlEdge> {
        let mut v = Vec::new();
        for x in 0..self.len() {
            for &y in &self.succ[x] {
                v.push((x, y));
            }
        }
        v
    }

    /// The post-dominator sets, and the tree they induce.
    ///
    /// Computed by fixpoint rather than by the Lengauer–Tarjan-style algorithm:
    /// the graphs here are one procedure body, so the direct iteration is both
    /// fast enough and obviously correct (each step only ever *shrinks* a set,
    /// and the lattice is finite).
    pub fn post_dominators(&self) -> PostDominators {
        let n = self.len();
        let exit = self.exit();
        // Start optimistic (everything), then intersect along successors.
        let mut sets: Vec<HashSet<Node>> = (0..n).map(|_| (0..n).collect()).collect();
        sets[exit] = [exit].into_iter().collect();
        let mut changed = true;
        while changed {
            changed = false;
            for x in 0..n {
                if x == exit {
                    continue;
                }
                let mut acc: Option<HashSet<Node>> = None;
                for &s in &self.succ[x] {
                    acc = Some(match acc {
                        None => sets[s].clone(),
                        Some(a) => a.intersection(&sets[s]).copied().collect(),
                    });
                }
                // A node with no successors is its own exit: it post-dominates
                // itself and reaches `EXIT` through nothing else.
                let mut next = acc.unwrap_or_else(|| [exit].into_iter().collect());
                next.insert(x);
                if next != sets[x] {
                    sets[x] = next;
                    changed = true;
                }
            }
        }
        // The post-dominator relation is a chain, so the immediate post-dominator
        // is the member of `sets[x] \ {x}` with the *largest* set of its own:
        // `PD(ipdom(x)) == PD(x) \ {x}`.
        let ipdom = (0..n)
            .map(|x| {
                sets[x]
                    .iter()
                    .copied()
                    .filter(|c| *c != x)
                    .max_by_key(|c| sets[*c].len())
            })
            .collect();
        PostDominators { sets, ipdom, exit }
    }

    /// The PDG's control edges: `X → Y` means `X`'s decision governs `Y`.
    ///
    /// Uses the post-dominator form of the definition, which is equivalent to the
    /// succ/`ipdom` walk but cannot produce a header-dependent-on-itself artifact:
    /// `X` governs `Y` when some successor `Z` of `X` needs `Y` on its way to the
    /// exit (`Y ∈ PD(Z)`) while `X` itself does not (`Y ∉ PD(X)`).
    pub fn control_edges(&self) -> Vec<CtrlEdge> {
        let pd = self.post_dominators();
        let mut out = Vec::new();
        for x in 0..self.len() {
            for &z in &self.succ[x] {
                if pd.sets[x].contains(&z) {
                    // This successor post-dominates `x`: it is on *every* path,
                    // so `x` does not choose it.
                    continue;
                }
                for y in &pd.sets[z] {
                    if *y != x && !pd.sets[x].contains(y) {
                        out.push((x, *y));
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The decisions that govern `n`, ascending.
    pub fn controlling_decisions(&self, n: Node) -> Vec<Node> {
        let mut v: Vec<Node> = self
            .control_edges()
            .into_iter()
            .filter(|(_, y)| *y == n)
            .map(|(x, _)| x)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The SPP Layer 2 reordering plan (§9.1.1): the statement-occurrence groups
    /// a reordering pass may permute among themselves.
    ///
    /// Two statements are *control-equivalent* when exactly the same decisions
    /// govern them — they always execute together or not at all. Within a
    /// control-equivalent class, exchanging two statements changes neither which
    /// decisions are made nor which statements those decisions govern. A class is
    /// not shipped whole, though: a group is one **straight-line run** of plain
    /// statements (single successor, not a terminator) inside the class. That
    /// extra split keeps a reorder from moving a statement across a decision, a
    /// join, an arm boundary, a `break`/`continue`/`return`, or between a loop
    /// header and its body.
    ///
    /// This is the consumer §7 Stub 2 named: `ctrl_edges` is no longer only
    /// computed, it is *read* — a Layer 2 pass drives its permutation off these
    /// groups rather than re-deriving control dependence itself.
    ///
    /// Only groups of two or more statements are returned, largest first (ties
    /// broken by the first member's source order), because a singleton group
    /// carries no reordering freedom.
    pub fn reorder_plan(&self) -> ReorderPlan {
        // A *plain* statement is one that may move in a sequential reorder: it is
        // a real statement, it has a single successor (so it is not a decision),
        // and it is not a terminator. A `break`, `continue`, `return` or `throw`
        // has a single successor too, but that successor is a jump target rather
        // than the next statement, so it must never be reordered past its
        // neighbours. The builder labels those nodes `break`/`continue`/
        // `exit-stmt`, which is what this test reads.
        let plain = |n: Node| {
            self.is_statement(n)
                && self.succ[n].len() == 1
                && !matches!(self.label(n), "break" | "continue" | "exit-stmt")
        };

        // Statements that share a governing set and sit in one straight-line run
        // form a permutation group. Partition first by governing set — a reorder
        // must never change which decisions govern a statement — then union the
        // plain statements that are consecutive (`s → n`, `n` plain) within the
        // set, so statements separated by a decision, a join, an arm boundary or
        // a terminator are *not* grouped.
        let mut by_gov: std::collections::BTreeMap<Vec<Node>, Vec<Node>> =
            std::collections::BTreeMap::new();
        for n in self.statements() {
            if plain(n) {
                by_gov.entry(self.controlling_decisions(n)).or_default().push(n);
            }
        }

        fn find(parent: &mut std::collections::HashMap<Node, Node>, x: Node) -> Node {
            let mut r = x;
            while parent[&r] != r {
                r = parent[&r];
            }
            let mut c = x;
            while parent[&c] != r {
                let next = parent[&c];
                parent.insert(c, r);
                c = next;
            }
            r
        }

        let mut groups: Vec<Vec<Node>> = Vec::new();
        for (_, members) in by_gov {
            let set: std::collections::HashSet<Node> = members.iter().copied().collect();
            let mut parent: std::collections::HashMap<Node, Node> =
                members.iter().map(|n| (*n, *n)).collect();
            for &m in &members {
                let mut cur = m;
                let mut seen: std::collections::HashSet<Node> = std::collections::HashSet::new();
                seen.insert(cur);
                loop {
                    let s = &self.succ[cur];
                    if s.len() != 1 {
                        break;
                    }
                    let n = s[0];
                    // The successor must be in this governing set (which already
                    // makes it plain); anything else ends the run.
                    if !set.contains(&n) || !seen.insert(n) {
                        break;
                    }
                    let rn = find(&mut parent, n);
                    let rm = find(&mut parent, m);
                    parent.insert(rn, rm);
                    cur = n;
                }
            }
            let mut by_root: std::collections::BTreeMap<Node, Vec<Node>> =
                std::collections::BTreeMap::new();
            for &m in &members {
                let r = find(&mut parent, m);
                by_root.entry(r).or_default().push(m);
            }
            for (_, mut g) in by_root {
                g.sort_unstable();
                if g.len() > 1 {
                    groups.push(g);
                }
            }
        }
        // Largest first, ties by first member.
        groups.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
        ReorderPlan { groups }
    }
}

/// A statement-reordering plan: the groups of control-equivalent statement
/// occurrences a Layer 2 pass may permute among themselves.
///
/// Produced by [`ControlGraph::reorder_plan`]. Each group is in source order; a
/// pass is free to emit the group's members in any order (its own dependences
/// permitting) without re-deriving which decisions govern them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderPlan {
    /// Groups of statement occurrences, each of size ≥ 2, largest first.
    pub groups: Vec<Vec<Node>>,
}

impl ReorderPlan {
    /// Whether any reordering freedom exists at all.
    pub fn is_reorderable(&self) -> bool {
        !self.groups.is_empty()
    }

    /// The total number of statement occurrences covered by a group.
    pub fn covered(&self) -> usize {
        self.groups.iter().map(|g| g.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::parse;

    /// The body of the first function in `src` — the procedure `build` walks.
    fn body(src: &str) -> Vec<Stmt> {
        parse(src)
            .expect("parses")
            .body
            .into_iter()
            .find_map(|s| match s {
                Stmt::FunctionDecl { body, .. } => Some(body),
                _ => None,
            })
            .expect("a function declaration")
    }

    fn graph(src: &str) -> ControlGraph {
        build(&body(src))
    }

    #[test]
    fn a_diamonds_arms_are_governed_by_the_if() {
        let g = graph("function f() { if (a) { x = 1; } else { y = 2; } z = 3; }");
        let s = g.statements();
        assert_eq!(s.len(), 4, "if, `x = 1`, `y = 2`, `z = 3`: {:?}", s.iter().map(|n| g.label(*n)).collect::<Vec<_>>());
        let e = g.control_edges();
        assert!(e.contains(&(s[0], s[1])), "the then arm is governed by the if: {e:?}");
        assert!(e.contains(&(s[0], s[2])), "so is the else arm: {e:?}");
        assert!(!e.contains(&(s[0], s[3])), "`z = 3` runs either way: {e:?}");
        assert!(g.controlling_decisions(s[3]).is_empty(), "{:?}", g.controlling_decisions(s[3]));
    }

    #[test]
    fn a_loop_body_is_governed_by_its_header() {
        let g = graph("function f() { for (let i = 0; i < 8; i++) { A[i] = 0; } B[0] = 1; }");
        let s = g.statements();
        assert_eq!(s.len(), 3, "the loop, its body, and the statement after it");
        let e = g.control_edges();
        assert!(e.contains(&(s[0], s[1])), "the body runs at the header's discretion: {e:?}");
        assert!(!e.contains(&(s[0], s[2])), "the loop does not govern what follows: {e:?}");
        assert!(g.controlling_decisions(s[2]).is_empty(), "{:?}", g.controlling_decisions(s[2]));
        // And the header is never control-dependent on itself — the artifact the
        // succ/`ipdom` walk produces on a loop header.
        assert!(!e.contains(&(s[0], s[0])), "{e:?}");
    }

    #[test]
    fn a_returning_arm_governs_what_follows_the_if() {
        let g = graph("function f() { if (a) { return 1; } y = 2; }");
        let s = g.statements();
        assert_eq!(s.len(), 3, "if, return, `y = 2`");
        let d = g.controlling_decisions(s[2]);
        assert_eq!(d, vec![s[0]], "`y = 2` runs only when the arm was not taken");
    }

    #[test]
    fn break_is_governed_by_the_if_that_selects_it() {
        let g = graph("function f() { for (let i = 0; i < 8; i++) { if (a) { break; } A[i] = 0; } }");
        let s = g.statements();
        assert_eq!(s.len(), 4, "loop, if, break, `A[i] = 0`");
        let d = g.controlling_decisions(s[2]);
        assert_eq!(d, vec![s[1]], "the `break` is chosen by the `if`, not by the loop");
    }

    #[test]
    fn post_dominators_are_rooted_at_the_exit() {
        let g = graph("function f() { if (a) { x = 1; } else { y = 2; } z = 3; }");
        let pd = g.post_dominators();
        let s = g.statements();
        assert_eq!(pd.exit(), g.exit());
        assert!(pd.post_dominates(g.exit(), g.entry()), "every path ends at EXIT");
        assert!(!pd.post_dominates(g.entry(), g.exit()), "EXIT reaches nothing");
        assert_eq!(pd.immediate(pd.exit()), None, "the tree's root has no parent");
        assert_eq!(pd.immediate(s[1]), Some(s[3]), "both arms merge at `z = 3`");
        assert_eq!(pd.immediate(s[2]), Some(s[3]));
    }

    #[test]
    fn an_empty_body_still_loops_and_still_governs_nothing_extra() {
        // `{}` bodies, empty arms, and a missing `else` all need a node to be the
        // successor of a decision; a `JOIN` supplies it.
        let g = graph("function f() { for (;;) { } if (a) { } else { } w = 1; }");
        let s = g.statements();
        assert_eq!(s.len(), 3, "loop, if, `w = 1`");
        assert!(g.edges().iter().any(|(x, _)| *x == s[0]), "the empty loop still has successors");
        assert!(g.controlling_decisions(s[2]).is_empty(), "{:?}", g.controlling_decisions(s[2]));
    }

    #[test]
    fn an_empty_program_is_entry_to_exit() {
        let g = build(&[]);
        assert_eq!(g.len(), 2, "ENTRY and EXIT only");
        assert_eq!(g.successors(g.entry()), &[g.exit()]);
        assert!(g.control_edges().is_empty());
    }

    // ---- §9.1.1 statement reordering: the `reorder_plan` consumer ----------

    #[test]
    fn a_straight_line_run_is_one_permutation_group() {
        let g = graph("function f() { x = 1; y = 2; z = 3; }");
        let s = g.statements();
        assert_eq!(s.len(), 3, "x=1, y=2, z=3");
        let p = g.reorder_plan();
        assert_eq!(p.groups, vec![vec![s[0], s[1], s[2]]], "one free run");
        assert!(p.is_reorderable());
        assert_eq!(p.covered(), 3);
    }

    #[test]
    fn statements_in_different_arms_are_not_grouped() {
        // Both arms are governed by the same `if`, but they are chosen by
        // different branches, so a reorder must never swap them.
        let g = graph("function f() { if (a) { x = 1; } else { y = 2; } }");
        let p = g.reorder_plan();
        assert!(p.groups.is_empty(), "no freedom across an arm boundary: {:?}", p.groups);
    }

    #[test]
    fn a_decision_splits_the_run_it_sits_in() {
        // `a = 1` and `b = 2` are governed only by ENTRY; the `if` is a decision
        // and `c = 3` is not governed by it. The `if` must therefore break the
        // run, leaving at most a singleton on either side.
        let g = graph("function f() { a = 1; b = 2; if (c) { d = 4; } e = 5; }");
        let p = g.reorder_plan();
        for grp in &p.groups {
            for w in grp.windows(2) {
                // No group may span the decision.
                assert!(
                    !(w[0] < g.statements()[2] && w[1] > g.statements()[2]),
                    "a group crossed the decision: {:?}",
                    grp
                );
            }
        }
        // The `a = 1; b = 2` prefix is contiguous and ungoverned, so it is one
        // group.
        let s = g.statements();
        assert!(p.groups.contains(&vec![s[0], s[1]]), "{:?}", p.groups);
    }

    #[test]
    fn a_loop_header_and_body_are_not_a_permutation_group() {
        let g = graph("function f() { for (let i = 0; i < 8; i++) { A[i] = 0; B[i] = 1; } }");
        let s = g.statements();
        let p = g.reorder_plan();
        // The header is a decision (two successors), so it can never be in a
        // group; the two body statements share the header as their decision and
        // run consecutively, so they may.
        for grp in &p.groups {
            assert!(!grp.contains(&s[0]), "the loop header must not be reorderable: {:?}", grp);
        }
        assert!(p.groups.contains(&vec![s[1], s[2]]), "{:?}", p.groups);
    }

    #[test]
    fn a_terminator_is_never_reordered_past_its_neighbour() {
        // `return` has a single successor (EXIT) but must not move; the two
        // statements before it are a free run.
        let g = graph("function f() { a = 1; b = 2; return a; }");
        let s = g.statements();
        let p = g.reorder_plan();
        assert!(p.groups.contains(&vec![s[0], s[1]]), "{:?}", p.groups);
        for grp in &p.groups {
            assert!(!grp.contains(&s[2]), "the `return` is not reorderable: {:?}", grp);
        }
    }
}
