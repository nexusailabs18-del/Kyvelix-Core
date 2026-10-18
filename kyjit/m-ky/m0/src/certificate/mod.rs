use crate::param_expr::ParamExpr;
use serde::{Deserialize, Serialize};

/// Phase 9a §4.2: the production emitters for the two Phase 4 evidence claim
/// kinds. Declared here rather than in `fud` because both the analyses and the
/// certificate vocabulary have to be in scope at once, and `fud` deliberately
/// does not depend on this module.
pub mod emit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub level: String,
    pub loop_hash: String,
    pub pair: (usize, usize),
    pub claim: Claim,
    pub witness: Witness,
    pub checker: String,
}

/// Memory ordering kinds for Item 6 (memory-model awareness).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderingKind {
    /// Relaxed - no ordering constraints
    Relaxed,
    /// Acquire - synchronizes with a release operation
    Acquire,
    /// Release - synchronizes with an acquire operation
    Release,
    /// AcqRel - both acquire and release
    AcqRel,
    /// SeqCst - sequential consistency
    SeqCst,
    /// Data dependency ordering (for dependency-ordered before)
    Dependency,
}

/// Item 7: Library effect summary for external function calls.
/// The checker verifies this against a known-identity library table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryEffectSummary {
    /// Effect bits: READ, WRITE, ALLOCATE, THROW, BLOCK, ATOMIC, IO, UNKNOWN
    pub effects: Vec<EffectBit>,
    /// Whether the function has external side effects not captured above
    pub has_unknown_external_state: bool,
}

/// Effect bits for library function summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectBit {
    Read,
    Write,
    Allocate,
    Throw,
    Block,
    Atomic,
    Io,
    Unknown,
}

/// One conjunctive leg of a run-time dependence test (Phase 5 §5.9, Ch. 7.6).
///
/// A versioned loop's test is `∀ i` over the loop's own IV domain of
/// `NonStraddling(i) || ByteEqual(i)`: on every iteration where the pair does
/// not straddle, nothing is checked; on the iterations where it does, the two
/// locations must hold the same value. The legs are named rather than inlined
/// because the checker reconstructs the predicate *from the access*, never from
/// the certificate: the kind says which predicate to apply, and the analysis's
/// constants (`k`, `es`) come back out of the loop.
///
/// This is the certificate-layer form of §5.9's check. The speculative
/// `crate::runtime::RuntimeCheckKind` names the *emitted-code* shapes a backend
/// would need (parallel/serial versions); this type names the verifiable
/// predicate the certificate layer can check today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeCheckLeg {
    /// `i >= k` with `k = |index coefficient|` — the two iterations do not
    /// straddle, so the anti-dependence is benign.
    NonStraddling,
    /// The straddling iterations satisfy the byte equality
    /// `A[i·es] == A[(i − k)·es]`, so the write stores what the read would see.
    ByteEqual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Claim {
    Exact(Vec<i64>),
    None,
    /// Phase 2: an exact integer solution with a parametric description.
    /// The decision tree specifies which witness to use depending on the
    /// runtime values of the parameters; the checker walks it against the
    /// certificate's environment and verifies the reached leaf from scratch.
    ///
    /// # Why a mis-made tree cannot fool the checker
    ///
    /// The tree is only a *case split*. Every leaf carries its own concrete
    /// witness, and the checker re-verifies that witness (in-bounds IVs, equal
    /// locations, claimed distance) against the loop and the environment,
    /// exactly as it does for `Witness::IntSolution`. A branch that fails to
    /// cover some environment is rejected ("no leaf applies"); a leaf whose
    /// witness is wrong is rejected on verification. So the tree can make a
    /// certificate *incomplete*, never *unsound*.
    Parametric(DecisionTree),
    /// Phase 1: the direction vector of the dependence — one ordering
    /// relation per loop level (Wolfe 4.1, `sign(j - i)`). Witnessed by a
    /// concrete `IntSolution { i, j }`; the checker replays the pair and
    /// requires every component's relation to hold.
    Direction(Vec<Dir>),
    /// Phase 1: the dependence level — the index of the leftmost nonzero
    /// component of the distance vector (Wolfe 5.1). `Level(depth)` means
    /// the dependence is loop-independent. Witnessed by `IntSolution`.
    Level(usize),
    /// Phase 1: the cross-loop dependence distance between two adjacent
    /// loops (Wolfe 9.2, fusion). Carried by `Witness::CrossLoopSolution`,
    /// which holds both iteration vectors.
    CrossLoop(Vec<i64>),
    /// Phase 3: a scalar dependence claim. The value defined at `def_id`
    /// reaches the use at `use_id` (Wolfe §6.3). The checker rebuilds the
    /// FUD chain from the carried CFG and confirms the definition is in the
    /// reaching set; it never trusts the analyzer's chain.
    ScalarReach { use_id: UseId, def_id: DefId },
    /// Phase 3: the negative form. The use at `use_id` has no reaching
    /// definition (it is uninitialized along every path to it). The checker
    /// rebuilds the chain and confirms the reaching set is empty.
    ScalarNoReach { use_id: UseId },
    /// Phase 4: a scalar dependence. The definition at `def` reaches the use
    /// at `use_` with the Wolfe §6.7.1 distance situation `distance`.
    ///
    /// Witnessed by [`Witness::ScalarDependenceEvidence`], which carries the
    /// CFG the claim was computed from. The checker rebuilds the FUD chains
    /// from that CFG and re-runs [`crate::fud::scalar_dd`]; the asserted
    /// `(def, use_, distance)` triple must appear in the re-derived
    /// `situations` map. Nothing the analyzer says about the chains or the
    /// dependence is trusted across the certificate boundary.
    ScalarDependence {
        def: DefId,
        use_: UseId,
        distance: ScalarDistance,
    },
    /// Phase 4: the trip count of a countable loop. The body runs `count`
    /// times and the induction variable exits at `exit_value`.
    ///
    /// Witnessed by [`Witness::TripCountEvidence`], which carries the CFG and
    /// names the induction variable and the loop guard's comparison operands.
    /// The checker rebuilds the FUD chains, re-runs
    /// [`crate::fud::induct`] to recover the IV, recomputes
    /// [`crate::fud::trip_count`], and requires both `count` and `exit_value`
    /// to agree. A loop the recomputation finds uncountable is rejected: this
    /// claim speaks only for countable loops.
    TripCount { count: i64, exit_value: i64 },
    /// Phase 6: an array region. The claim says an access touches only the
    /// elements in `region`.
    ///
    /// Witnessed by [`Witness::ArrayRegionEvidence`], which carries the loop the
    /// region was derived from and the access index. The checker re-derives the
    /// region from that loop (via [`crate::region::region_of_access`]) and
    /// requires the derived region to be a subset of the claimed one — so a
    /// region that *under*-approximates the access is rejected.
    ArrayRegion {
        access: usize,
        region: crate::region::Region,
    },
    /// Phase 6: the `MayUse` / `MayMod` / `Kill` summary of a loop (Wolfe
    /// §8.1.1). The checker rebuilds the summary from the carried loop and
    /// requires the three sets to agree exactly.
    FunctionSummary {
        func: String,
        may_use: Vec<crate::region::Region>,
        may_mod: Vec<crate::region::Region>,
        kill: Vec<crate::region::Region>,
    },
    /// Phase 5 §5.1.8: a recognised reduction. `var` accumulates in the loop
    /// whose header is `loop_` under `kind`.
    ///
    /// The spec's shape is `Reduction(ReductionKind)`; the `var`/`loop_` fields
    /// are the identities the checker needs to re-derive the same reduction, so
    /// they are carried rather than left implicit. Witnessed by
    /// [`Witness::Phase5Evidence`]; the checker rebuilds the loop's CFG, re-runs
    /// [`crate::ir::reduction::detect_reductions`], and requires the exact
    /// `(var, loop_, kind)` triple to be present.
    Reduction {
        var: usize,
        loop_: usize,
        kind: crate::ir::reduction::ReductionKind,
    },
    /// Phase 5 §5.1.9: an alignment that removes the loop's loop-carried
    /// dependences. Witnessed by [`Witness::Phase5Evidence`]; the checker
    /// recomputes the alignment from the carried loop and its dependence set
    /// and requires exact agreement.
    Aligned(Vec<crate::fud::Alignment>),
    /// Phase 5 §5.1.11: the loop kind (Wolfe Ch. 9). Witnessed by
    /// [`Witness::Phase5Evidence`]; the checker re-derives the loop's
    /// dependence set and re-runs
    /// [`crate::ir::loopkind::classify_loop_kind`], requiring agreement.
    ///
    /// The §5.1.10 cross-loop distance is *not* a new claim: Phase 1's
    /// [`Claim::CrossLoop`] already carries that vocabulary with a
    /// [`Witness::CrossLoopSolution`] witness, and
    /// [`crate::fud::crossloop`] is the computation behind it. Adding a second,
    /// differently-witnessed claim for the same fact would be duplication, so
    /// §5.1.10 is served by the Phase 1 claim plus the new module.
    LoopKind(crate::ir::loopkind::LoopKind),
    /// Phase 7 §7.8: an alias fact — `MustAlias`, `MayAlias`, `NoAlias` or
    /// `DisjointArrays` (Wolfe §8.2.2–§8.2.3).
    ///
    /// Witnessed by [`Witness::AliasEvidence`], which carries the *pointer
    /// program* the fact was derived from and the basis connecting the claim to
    /// it. The checker re-runs the points-to fixpoint over that program and
    /// re-derives the answer, so an analyzer that mis-derives an alias produces
    /// a certificate its own checker rejects — the same discipline as every
    /// other claim kind.
    Alias(crate::points::certificate::AliasClaim),
    /// Phase 8 §8.10: an interprocedural fact — purity, MOD set, I/O order,
    /// formal value, or whole-procedure summary.
    ///
    /// Witnessed by [`Witness::IpaEvidence`], which carries the whole program
    /// the fact was computed from. The checker re-runs the corresponding
    /// interprocedural analysis over that program and re-derives the fact, so an
    /// analyzer that mis-derived it produces a certificate its own checker
    /// rejects.
    Interprocedural(crate::ipa::certificate::IpaClaim),
    /// Phase 5 §5.9 (Ch. 7.6): a run-time dependence test. The loop's access
    /// pair is *provably independent only at run time*, under the versioned
    /// predicate the legs below describe.
    ///
    /// Unlike every other claim, the certificate does not assert that the
    /// accesses are independent. It asserts that the emitter's analysis of the
    /// pair yields exactly `legs`, and the checker re-derives that analysis from
    /// the loop's own accesses. A caller who reads the claim as "independent"
    /// has mis-read it: the run-time check is the fallback that licenses
    /// parallelization *if* it holds, and this claim only certifies that the
    /// check is the right one.
    RuntimeCheck {
        legs: Vec<RuntimeCheckLeg>,
    },
    /// Item 6: Memory-model awareness — ordering constraint between two accesses.
    /// The checker re-derives the ordering from the operation semantics.
    Ordering {
        kind: OrderingKind,
        pair: (usize, usize),
    },
    /// Item 7: Library-effect summary — effect summary for an external library call.
    /// The checker verifies the summary against a known-identity library table.
    LibraryEffect {
        library: String,
        function: String,
        effect: LibraryEffectSummary,
    },
}

// Phase 3 scalar ids, re-exported here so the certificate vocabulary is
// self-contained (the certificate module is the public face of a claim).
pub use crate::ir::scalar::{DefId, UseId};

// Phase 4: the distance situation an `ScalarDependence` claim asserts. Re-
// exported from the analysis (`fud::scalardd`) so the claim and the checker use
// the exact same vocabulary the analysis emits; `fud` does not depend on
// `certificate`, so this edge is acyclic.
pub use crate::fud::scalardd::ScalarDistance;

/// A case split over the runtime parameters of a loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionTree {
    pub root: DecisionNode,
}

/// One node of a [`DecisionNode`] tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionNode {
    /// A terminal case: a dependence exists, witnessed by this leaf.
    Leaf(DecisionLeaf),
    /// A terminal case: the level proved **no** dependence exists for any
    /// environment reaching here. Paired with `Claim::None`; the checker
    /// confirms it by enumerating the (already concrete) iteration space.
    NoDependence,
    /// Test `condition`; take `then_branch` when it holds, `else_branch`
    /// otherwise.
    Branch {
        condition: ParamCond,
        then_branch: Box<DecisionNode>,
        else_branch: Box<DecisionNode>,
    },
}

/// The terminal witness of a decision path.
///
/// The vectors are **symbolic**: they are evaluated at the certificate's
/// environment, so one leaf covers a whole region of parameter space (a
/// distance of `P/2` needs `ParamExpr::Div`, which no concrete witness can
/// express). A constant leaf is written with `ParamExpr::Const`, so the
/// reference's concrete `Witness::IntSolution` shape is still expressible.
///
/// Nothing here is trusted: the checker evaluates the expressions, then checks
/// the resulting pair is in bounds, that both accesses name the same location,
/// and that `distance` really is `j - i`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionLeaf {
    /// The write side's iteration vector.
    pub i: Vec<ParamExpr>,
    /// The read side's iteration vector.
    pub j: Vec<ParamExpr>,
    /// The claimed dependence distance `j - i`.
    pub distance: Vec<ParamExpr>,
}

impl DecisionLeaf {
    /// A leaf that holds for every environment, with constant witnesses.
    pub fn constant(i: &[i64], j: &[i64], distance: &[i64]) -> Self {
        DecisionLeaf {
            i: i.iter().map(|&v| ParamExpr::Const(v)).collect(),
            j: j.iter().map(|&v| ParamExpr::Const(v)).collect(),
            distance: distance.iter().map(|&v| ParamExpr::Const(v)).collect(),
        }
    }
}

/// The outcome of walking a tree at one environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    /// A dependence, with its leaf and its pre-order terminal index.
    Dependence(usize, DecisionLeaf),
    /// Proved absence of any dependence, with its pre-order terminal index.
    Absent(usize),
}

impl DecisionNode {
    /// The leaf this path reaches for `env`, or `None` when the path ends at
    /// [`DecisionNode::NoDependence`] or a condition cannot be evaluated.
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<DecisionLeaf> {
        match self.resolve_terminal(env)? {
            Terminal::Dependence(_, l) => Some(l),
            Terminal::Absent(_) => None,
        }
    }

    /// Walk the tree at `env`, returning the terminal reached together with its
    /// pre-order index (counting `NoDependence` nodes as terminals too, so
    /// indices stay stable when a level adds refutation cases).
    ///
    /// `None` means the walk could not be completed -- a condition referenced a
    /// parameter that `env` does not bind, or a division was not exact. The
    /// checker treats that as "this certificate does not apply", never as a
    /// silent false, because silently taking the else-branch would let an
    /// unknown condition pick an arbitrary witness.
    pub fn resolve_terminal(&self, env: &[(String, i64)]) -> Option<Terminal> {
        self.walk(env, 0)
    }

    /// `base` is this subtree's pre-order terminal offset: descending into an
    /// else-branch must skip over every terminal in the then-branch it did not
    /// take, or the indices would not match a full pre-order numbering.
    fn walk(&self, env: &[(String, i64)], base: usize) -> Option<Terminal> {
        match self {
            DecisionNode::Leaf(l) => Some(Terminal::Dependence(base, l.clone())),
            DecisionNode::NoDependence => Some(Terminal::Absent(base)),
            DecisionNode::Branch {
                condition,
                then_branch,
                else_branch,
            } => {
                if condition.eval(env)? {
                    then_branch.walk(env, base)
                } else {
                    else_branch.walk(env, base + then_branch.count_terminals())
                }
            }
        }
    }

    /// How many terminals (leaves and refutations) this subtree has, pre-order.
    pub fn count_terminals(&self) -> usize {
        match self {
            DecisionNode::Leaf(_) | DecisionNode::NoDependence => 1,
            DecisionNode::Branch {
                then_branch,
                else_branch,
                ..
            } => then_branch.count_terminals() + else_branch.count_terminals(),
        }
    }
}

/// A condition on the parameter environment.
///
/// The operand is a [`ParamExpr`] rather than a bare parameter name (the
/// reference sketches `Eq(String, i64)`); a name is `ParamExpr::Param`, so this
/// is a strict generalization that lets a leaf guard express `c·d = a0 - b0`
/// and bound tests like `N >= d + 1` directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamCond {
    /// `expr == c`
    Eq(ParamExpr, i64),
    /// `expr <= c`
    Le(ParamExpr, i64),
    /// `expr >= c`
    Ge(ParamExpr, i64),
    /// `expr % c == 0` (Euclidean remainder, `c > 0`)
    Mod(ParamExpr, i64),
}

impl ParamCond {
    /// Evaluate against `env`. `None` when an expression cannot be evaluated
    /// (a missing parameter or a non-exact division) -- the checker treats that
    /// as "no leaf applies", never as a silent false.
    pub fn eval(&self, env: &[(String, i64)]) -> Option<bool> {
        match self {
            ParamCond::Eq(e, c) => Some(e.eval(env)? == *c),
            ParamCond::Le(e, c) => Some(e.eval(env)? <= *c),
            ParamCond::Ge(e, c) => Some(e.eval(env)? >= *c),
            ParamCond::Mod(e, c) => {
                let m = match c.checked_abs() {
                    Some(0) | None => return None,
                    Some(m) => m,
                };
                let v = e.eval(env)?;
                Some(v.rem_euclid(m) == 0)
            }
        }
    }
}

impl DecisionTree {
    /// A one-leaf tree with constant witnesses: valid for every environment.
    pub fn leaf(i: &[i64], j: &[i64], distance: &[i64]) -> Self {
        DecisionTree {
            root: DecisionNode::Leaf(DecisionLeaf::constant(i, j, distance)),
        }
    }

    /// A tree whose root asserts that no dependence exists anywhere.
    pub fn no_dependence() -> Self {
        DecisionTree {
            root: DecisionNode::NoDependence,
        }
    }

    /// Wrap `then_` under `cond`, falling through to `else_`.
    pub fn branch(cond: ParamCond, then_: DecisionNode, else_: DecisionNode) -> DecisionNode {
        DecisionNode::Branch {
            condition: cond,
            then_branch: Box::new(then_),
            else_branch: Box::new(else_),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    /// `j[k] < i[k]` — lexically backward.
    Lt,
    /// `j[k] == i[k]` — same iteration.
    Eq,
    /// `j[k] > i[k]` — lexically forward.
    Gt,
    /// `j[k] != i[k]` — nonzero, direction unknown.
    Ne,
    /// Unconstrained: any signed distance satisfies it.
    Unknown,
}

impl Dir {
    /// The direction relation induced by a concrete signed distance.
    pub fn of_delta(delta: i64) -> Dir {
        match delta.cmp(&0) {
            std::cmp::Ordering::Less => Dir::Lt,
            std::cmp::Ordering::Equal => Dir::Eq,
            std::cmp::Ordering::Greater => Dir::Gt,
        }
    }

    /// Does a concrete signed distance satisfy this relation?
    pub fn holds(&self, delta: i64) -> bool {
        match self {
            Dir::Lt => delta < 0,
            Dir::Eq => delta == 0,
            Dir::Gt => delta > 0,
            Dir::Ne => delta != 0,
            Dir::Unknown => true,
        }
    }
}

/// Phase 1: the index of the leftmost nonzero component of a distance
/// vector — the loop nest level that carries the dependence (Wolfe 5.1).
/// Returns `distance.len()` when every component is zero, which means the
/// dependence is loop-independent.
pub fn dependence_level(distance: &[i64]) -> usize {
    distance.iter().position(|&d| d != 0).unwrap_or(distance.len())
}

impl Claim {
    /// Phase 1: derive the `Direction` claim of a concrete distance vector.
    pub fn direction_of(distance: &[i64]) -> Claim {
        Claim::Direction(distance.iter().map(|&d| Dir::of_delta(d)).collect())
    }

    /// Phase 1: derive the `Level` claim of a concrete distance vector.
    pub fn level_of(distance: &[i64]) -> Claim {
        Claim::Level(dependence_level(distance))
    }
}

/// M13: a single affine assertion on a single IV.
/// Semantics: the assertion holds for the access it is attached to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Assertion {
    pub kind: AssertionKind,
    pub iv: usize,
    pub bound: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AssertionKind {
    /// `iv < bound`
    Lt,
    /// `iv <= bound`
    Le,
    /// `iv > bound`
    Gt,
    /// `iv >= bound`
    Ge,
    /// `iv != bound`
    Ne,
}

impl Assertion {
    /// Does the given IV value satisfy this assertion?
    pub fn holds(&self, iv_value: i64) -> bool {
        match self.kind {
            AssertionKind::Lt => iv_value < self.bound,
            AssertionKind::Le => iv_value <= self.bound,
            AssertionKind::Gt => iv_value > self.bound,
            AssertionKind::Ge => iv_value >= self.bound,
            AssertionKind::Ne => iv_value != self.bound,
        }
    }
}

/// H-3: the transformation rule applied in a `Witness::Rewrite`.
/// Each variant corresponds to a specific normalization/flattening/transform
/// that the decision procedures perform. The checker re-applies the named
/// rule and requires the result to match the `rewritten` loop exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "rule")]
pub enum RewriteRule {
    /// M2g: affine-in-disguise normalization.
    /// Recognizes patterns like `(i << c)`, `i * c`, `i + c`, `(i << a) | b`,
    /// `(i + c) * a`, `(i / c) * c` and rewrites them to `index + offset` form.
    M2gNormalize,
    /// M2a: injectivity for `A[B[i]]` with affine indirection `B[i] = c*i + d`.
    /// Flattens `access.index + indirect.index`, `access.offset + indirect.offset`.
    M2aFlatten,
    /// M3: bounded recursion transform.
    /// Rewrites a self-recursive or mutually recursive function with a single
    /// tail call and a bounding guard into a standard `for` loop.
    M3Recursion,
    /// M4: guard-based IV domain narrowing.
    /// Narrows the effective IV domain of an access based on its guard condition.
    M4GuardNarrow,
    /// M5: bounded pointer chase expansion.
    /// Expands a loop with a `PointerChase` into a loop with explicit per-step
    /// accesses, removing the `chase` field.
    M5ChaseExpand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Witness {
    IntSolution { i: Vec<i64>, j: Vec<i64> },
    UnsatProof { constraints: Vec<CConstraint> },
    /// M9: a witness that carries a runtime-parameter binding. The
    /// checker substitutes `env` before evaluating `inner`.
    WithEnv {
        env: Vec<(String, i64)>,
        inner: Box<Witness>,
    },
    /// M13: assertions that constrain the IV domain. Each assertion
    /// must hold at the witness's IV values. The `inner` witness is
    /// verified under the assumption that the assertions hold.
    AssertionChain {
        assertions: Vec<Assertion>,
        inner: Box<Witness>,
    },
    /// General carrier: the certificate is emitted against a *rewritten*
    /// loop (e.g., M2g's normalization, M2a's flattening, M3's recursion
    /// transform, M5's chase expansion). The checker verifies `inner`
    /// against `loop` instead of the file on disk.
    ///
    /// The `loop_hash` of the outer certificate is the hash of the
    /// ORIGINAL file (so the checker's first check — hash matches the
    /// file — still passes). The `loop` field carries the rewritten
    /// form the witness was actually computed for.
    WithLoop {
        loop_: Box<crate::ir::Loop>,
        inner: Box<Witness>,
    },
    /// H-3: a certificate that records an explicit rewrite rule applied
    /// to the original loop. The checker re-applies the rule and requires
    /// the result to match the loop carried in the witness.
    ///
    /// This replaces the opaque `WithLoop` for M2g/M2a/M3/M5: instead of
    /// trusting that the carried loop is a valid rewrite, the checker
    /// re-runs the specific rule on the original loop (which hashes to
    /// `cert.loop_hash`) and requires byte-for-byte equality with the
    /// `rewritten` loop in the witness.
    ///
    /// The `original` loop is provided so the checker does not need to
    /// read the file again; its hash is verified against `cert.loop_hash`.
    Rewrite {
        /// The transformation rule that was applied.
        rule: RewriteRule,
        /// The original loop (hashes to cert.loop_hash).
        original: Box<crate::ir::Loop>,
        /// The loop after applying `rule` to `original`.
        rewritten: Box<crate::ir::Loop>,
        /// The inner witness, verified against `rewritten`.
        inner: Box<Witness>,
    },
    /// Phase 1: a cross-loop dependence witness (Wolfe 9.2, fusion). The
    /// two loops are adjacent and not nested in each other; `i` is an
    /// iteration vector of `first_loop`, `j` of `second_loop`, and the
    /// claimed cross-loop distance is `j - i`.
    ///
    /// The carried `first_loop` must hash to the certificate's
    /// `loop_hash`, so the checker never verifies against a loop that is
    /// not the one on disk. A level that needs to rewrite the first loop
    /// before analysis wraps this witness in `Witness::WithLoop`, which
    /// already carries the original-file hash discipline.
    CrossLoopSolution {
        first_loop: Box<crate::ir::Loop>,
        second_loop: Box<crate::ir::Loop>,
        i: Vec<i64>,
        j: Vec<i64>,
    },
    /// Phase 2: the witness selected by a `Claim::Parametric` decision tree.
    ///
    /// `env` binds the parameter names that appear in the tree's conditions
    /// and in the loop's parametric bounds; `leaf` names the terminal the
    /// emitting level resolved to. The checker does **not** trust `leaf`: it
    /// walks the tree at `env` itself, verifies the terminal it reaches, and
    /// only then requires `leaf` to agree. That ordering matters -- a forged
    /// index cannot promote a wrong terminal into an accepted certificate,
    /// because the terminal is chosen by the environment, not by the witness.
    ///
    /// Appended last so the bincode variant indices of every earlier witness
    /// shape are unchanged.
    ParametricSolution {
        env: Vec<(String, i64)>,
        leaf: usize,
    },
    /// Phase 3: a FUD chain, presented as evidence that a scalar use reaches
    /// (or does not reach) a scalar definition.
    ///
    /// The chain itself is **not** trusted: the checker rebuilds it from the
    /// `cfg` the certificate carries (re-running φ-placement and Algorithm
    /// Chain from scratch) and then verifies the asserted link against the
    /// rebuilt reaching-definition set. This mirrors the `WithLoop` /
    /// `CrossLoopSolution` discipline: the certificate carries the ground
    /// truth (the CFG), and the checker re-derives everything from it.
    FudChainEvidence {
        cfg: Box<crate::ir::cfg::CFG>,
        /// The certificate asserts something about this use …
        asserted_use: UseId,
        /// … and (for `ScalarReach`) this definition; `None` for
        /// `ScalarNoReach`.
        asserted_def: Option<DefId>,
    },
    /// Phase 4: evidence for a [`Claim::ScalarDependence`]. Carries the CFG the
    /// claim was computed from; the checker rebuilds the FUD chains and re-runs
    /// [`crate::fud::scalar_dd`] from scratch, then requires the asserted
    /// `(def, use_, distance)` triple to be present in the re-derived result.
    ScalarDependenceEvidence {
        cfg: Box<crate::ir::cfg::CFG>,
        /// The definition end of the asserted dependence.
        asserted_def: DefId,
        /// The use end of the asserted dependence.
        asserted_use: UseId,
    },
    /// Phase 4: evidence for a [`Claim::TripCount`]. Carries the CFG and names
    /// the induction variable whose trip count is claimed, plus the loop guard
    /// in `x < bound` form. The checker rebuilds the chains, re-runs
    /// [`crate::fud::induct`] to confirm the IV, recomputes
    /// [`crate::fud::trip_count`], and requires the result to match.
    ///
    /// `bound` and `bound_step` are inputs to the recomputation, not trusted
    /// facts: the IV's initial value and step come from the checker's own
    /// `induct` run, so a forged `bound` can at most make the certificate
    /// disagree with the recomputation, never forge agreement.
    ///
    /// H-4: when the trip count comes from a mutual recursion (M3 with a
    /// companion), `companion_step` records the companion's step so the
    /// checker can re-apply the mutual recursion unfolding logic and verify
    /// the effective step is the sum of both steps.
    TripCountEvidence {
        cfg: Box<crate::ir::cfg::CFG>,
        /// The induction variable's scalar variable index.
        iv_var: usize,
        /// The header block of the loop the IV drives.
        loop_: usize,
        /// The guard's comparison bound.
        bound: i64,
        /// The per-iteration increment of the bound (`0` when invariant).
        bound_step: i64,
        /// H-4: the companion's step for mutual recursion. `None` for
        /// single recursion; `Some(step)` when a companion contributes to
        /// the effective step. The checker verifies that the IV's derived
        /// step equals the main step plus this companion step.
        #[serde(default)]
        companion_step: Option<i64>,
    },
    /// Phase 6: evidence for a [`Claim::ArrayRegion`] or
    /// [`Claim::FunctionSummary`]. Carries the loop the region/summary was
    /// derived from; the checker re-derives the region from it rather than
    /// trusting any region the analyzer reported.
    ArrayRegionEvidence {
        loop_: Box<crate::ir::Loop>,
    },
    /// Phase 5: evidence for the §5.1.8/§5.1.9/§5.1.11 algorithm claims
    /// (`Reduction`, `Aligned`, `LoopKind`). Carries the loop the claim was
    /// computed from; the checker re-runs the corresponding analysis over it and
    /// requires the result to agree with the claim.
    Phase5Evidence {
        loop_: Box<crate::ir::Loop>,
    },
    /// Phase 7: evidence for a [`Claim::Alias`]. Carries the pointer program the
    /// claim was computed from — the ground truth — plus the [`AliasBasis`] that
    /// says which part of it the claim speaks about.
    ///
    /// The checker re-runs [`crate::points::analyze`] over `program` and
    /// re-derives the alias relation at the basis's program point; nothing the
    /// analyzer reported about the graph is trusted. A forged basis therefore
    /// cannot promote a wrong claim, because the derived answer is a function of
    /// the program, not of the witness.
    AliasEvidence {
        program: Box<crate::points::fixpoint::PtrProgram>,
        basis: crate::points::certificate::AliasBasis,
    },
    /// Phase 8: evidence for a [`Claim::Interprocedural`]. Carries the whole
    /// program the fact was computed from — the ground truth. The checker re-runs
    /// the relevant interprocedural analysis (purity, MOD, I/O, constant
    /// propagation, or summary) over it and re-derives the fact, so nothing the
    /// analyzer concluded crosses the certificate boundary.
    IpaEvidence {
        program: Box<crate::ipa::program::IpaProgram>,
    },
    /// Phase 5 §5.9 (Ch. 7.6): evidence for a [`Claim::RuntimeCheck`]. Names the
    /// access pair the test was built for; the checker reads that pair's index
    /// coefficient and element size **from the loop on disk** and rebuilds the
    /// predicate, so the witness carries an index, not a fact.
    RuntimeCheckEvidence {
        pair: (usize, usize),
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CConstraint {
    pub coeffs: Vec<i64>,
    pub constant: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_of_delta() {
        assert_eq!(Dir::of_delta(-3), Dir::Lt);
        assert_eq!(Dir::of_delta(0), Dir::Eq);
        assert_eq!(Dir::of_delta(7), Dir::Gt);
    }

    #[test]
    fn test_dir_holds() {
        assert!(Dir::Lt.holds(-1));
        assert!(!Dir::Lt.holds(0));
        assert!(Dir::Eq.holds(0));
        assert!(!Dir::Eq.holds(1));
        assert!(Dir::Gt.holds(1));
        assert!(!Dir::Gt.holds(-1));
        assert!(Dir::Ne.holds(5));
        assert!(Dir::Ne.holds(-5));
        assert!(!Dir::Ne.holds(0));
        assert!(Dir::Unknown.holds(0));
        assert!(Dir::Unknown.holds(-9));
    }

    #[test]
    fn test_dependence_level() {
        assert_eq!(dependence_level(&[-4]), 0);
        assert_eq!(dependence_level(&[0, 3]), 1);
        assert_eq!(dependence_level(&[0, 0, 5]), 2);
        // All-zero distance means loop-independent: level == depth.
        assert_eq!(dependence_level(&[0, 0]), 2);
        assert_eq!(dependence_level(&[]), 0);
    }

    #[test]
    fn test_claim_derivation() {
        assert!(matches!(Claim::direction_of(&[-4]), Claim::Direction(ref v) if v == &[Dir::Lt]));
        assert!(
            matches!(Claim::direction_of(&[0, 2, -1]), Claim::Direction(ref v) if v == &[Dir::Eq, Dir::Gt, Dir::Lt])
        );
        assert!(matches!(Claim::level_of(&[0, 3]), Claim::Level(1)));
        assert!(matches!(Claim::level_of(&[0, 0]), Claim::Level(2)));
    }

    #[test]
    fn test_claim_direction_serde_roundtrip() {
        let c = Claim::direction_of(&[0, 2, -1]);
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#"{"kind":"Direction","value":["Eq","Gt","Lt"]}"#);
        let back: Claim = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Claim::Direction(ref v) if v == &[Dir::Eq, Dir::Gt, Dir::Lt]));
    }

    #[test]
    fn test_claim_level_serde_roundtrip() {
        let s = serde_json::to_string(&Claim::Level(1)).unwrap();
        assert_eq!(s, r#"{"kind":"Level","value":1}"#);
        let back: Claim = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Claim::Level(1)));
    }

    #[test]
    fn test_claim_cross_loop_serde_roundtrip() {
        let s = serde_json::to_string(&Claim::CrossLoop(vec![-4])).unwrap();
        assert_eq!(s, r#"{"kind":"CrossLoop","value":[-4]}"#);
        let back: Claim = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Claim::CrossLoop(ref d) if d == &[-4]));
    }

    #[test]
    fn test_witness_cross_loop_serde_roundtrip() {
        let l: crate::ir::Loop = serde_json::from_str(
            r#"{"depth":1,"ivs":[{"name":"i","lower":0,"upper":64,"step":1}],
                "body":[{"kind":"write","base":"A","index":[1],"elem_size":8,"offset":0}]}"#,
        )
        .unwrap();
        let w = Witness::CrossLoopSolution {
            first_loop: Box::new(l.clone()),
            second_loop: Box::new(l),
            i: vec![4],
            j: vec![0],
        };
        let s = serde_json::to_string(&w).unwrap();
        assert!(s.contains(r#""type":"CrossLoopSolution""#));
        let back: Witness = serde_json::from_str(&s).unwrap();
        match back {
            Witness::CrossLoopSolution {
                first_loop,
                second_loop,
                i,
                j,
            } => {
                assert_eq!(i, vec![4]);
                assert_eq!(j, vec![0]);
                assert_eq!(first_loop.depth, 1);
                assert_eq!(second_loop.depth, 1);
                assert_eq!(first_loop.hash(), second_loop.hash());
            }
            other => panic!("wrong variant: {:?}", other),
        }
    }

    /// Deterministic xorshift64, matching the convention used by the
    /// per-level fuzzers (`decide::m6`, `decide::m13`, ...).
    fn xorshift_cert(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    /// Phase 1 fuzz: the pure claim derivations. Every `direction_of` /
    /// `level_of` / `dependence_level` result is cross-checked against a
    /// brute-force reference, and every derived claim must survive a serde
    /// round-trip byte-for-byte.
    #[test]
    fn fuzz_phase1_derivations() {
        let mut state: u64 = 0x0123_4567_89ab_cdef;
        let mut tested = 0u64;
        let mut bad = 0u64;
        let mut examples: Vec<String> = Vec::new();

        for _ in 0..3000 {
            // Random distance vector: depth 1..=4, components in -4..=4.
            // Small ranges keep the interesting cases (zeros, sign flips)
            // well represented.
            let depth = (xorshift_cert(&mut state) % 4) as usize + 1;
            let d: Vec<i64> = (0..depth)
                .map(|_| (xorshift_cert(&mut state) % 9) as i64 - 4)
                .collect();
            tested += 1;

            // `of_delta` always picks a relation the delta actually satisfies.
            for &delta in &d {
                if !Dir::of_delta(delta).holds(delta) {
                    bad += 1;
                    if examples.len() < 5 {
                        examples.push(format!("of_delta({}).holds({}) is false", delta, delta));
                    }
                }
            }

            // `holds` agrees with the brute-force definition for all relations.
            for &delta in &d {
                for r in [Dir::Lt, Dir::Eq, Dir::Gt, Dir::Ne, Dir::Unknown] {
                    let want = match r {
                        Dir::Lt => delta < 0,
                        Dir::Eq => delta == 0,
                        Dir::Gt => delta > 0,
                        Dir::Ne => delta != 0,
                        Dir::Unknown => true,
                    };
                    if r.holds(delta) != want {
                        bad += 1;
                        if examples.len() < 5 {
                            examples.push(format!("{:?}.holds({}) != {}", r, delta, want));
                        }
                    }
                }
            }

            // `direction_of` yields one relation per level, each of which
            // holds, and each of which is the *strongest* relation
            // (Lt/Eq/Gt — never the weakened Ne/Unknown).
            match Claim::direction_of(&d) {
                Claim::Direction(v) if v.len() == depth => {
                    for k in 0..depth {
                        if !v[k].holds(d[k]) || !matches!(v[k], Dir::Lt | Dir::Eq | Dir::Gt) {
                            bad += 1;
                            if examples.len() < 5 {
                                examples.push(format!("direction_of({:?})[{}] = {:?}", d, k, v[k]));
                            }
                        }
                    }
                }
                other => {
                    bad += 1;
                    if examples.len() < 5 {
                        examples.push(format!("direction_of({:?}) = {:?}", d, other));
                    }
                }
            }

            // `level_of` / `dependence_level` = index of the first nonzero
            // component, or `depth` when all components are zero.
            let want_level = d.iter().position(|&x| x != 0).unwrap_or(depth);
            if !matches!(Claim::level_of(&d), Claim::Level(l) if l == want_level) {
                bad += 1;
                if examples.len() < 5 {
                    examples.push(format!("level_of({:?}) != {}", d, want_level));
                }
            }
            if dependence_level(&d) != want_level {
                bad += 1;
                if examples.len() < 5 {
                    examples.push(format!("dependence_level({:?}) != {}", d, want_level));
                }
            }

            // Every derived claim survives a serde round-trip byte-for-byte.
            for c in [
                Claim::direction_of(&d),
                Claim::level_of(&d),
                Claim::CrossLoop(d.clone()),
            ] {
                let s = serde_json::to_string(&c).unwrap();
                let back: Claim = serde_json::from_str(&s).unwrap();
                let s2 = serde_json::to_string(&back).unwrap();
                if s != s2 {
                    bad += 1;
                    if examples.len() < 5 {
                        examples.push(format!("serde round-trip changed {}", s));
                    }
                }
            }
        }

        println!("Phase 1 derivation fuzz: tested={} bad={}", tested, bad);
        for ex in &examples {
            println!("  BAD {}", ex);
        }
        assert_eq!(bad, 0, "Phase 1 derivation fuzz found {} violations", bad);
    }
}