pub mod cfg;
pub mod dependence;
pub mod effect;
pub mod layout;
pub mod loopkind;
pub mod purity;
pub mod reduction;
pub mod scalar;
pub mod stmt;

pub use cfg::{Block, CFG, CfgIndex, DefSite, PhiSite, Stmt, UpsilonSite, UseSite};
pub use scalar::{
    DefId, PhiId, PhiTerm, ScalarDef, ScalarRhs, ScalarTy, ScalarUse, ScalarVar, UpsilonId,
    UpsilonTerm, UseId,
};
// Phase 9a: the loop-body statement form. Aliased because `Stmt` above is the
// scalar-CFG statement (`{ uses, defs }`); this is the frontend form that
// lowers into it.
pub use stmt::{MemoryContext, ScalarExpr, Stmt as LoopStmt, StmtKind};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Loop {
    pub depth: usize,
    pub ivs: Vec<IV>,
    pub body: Vec<Access>,
    /// When `Some(_)`, this "loop" is actually a bounded recursive
    /// function that M3 will transform into a real loop before M0 sees it.
    /// When present, `ivs` is a placeholder and MUST NOT be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursion: Option<Recursion>,
    /// When `Some(_)`, at least one access uses a chase variable whose
    /// value follows a bounded chain. M5 expands each such access into
    /// one copy per chase step. When present, accesses on
    /// `chase.accessed_base` have placeholder indices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chase: Option<PointerChase>,
    /// M13: assertions that constrain the IV domain for this loop.
    /// Empty (default) means no constraints. Assertions are checked by
    /// the checker when a certificate carries a `Witness::AssertionChain`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<crate::certificate::Assertion>,

    /// Phase 9a: scalar variables declared in the loop. The loop's IVs are
    /// always registered as scalars `0..depth`, so `scalars` normally begins
    /// with one entry per IV (see [`Loop::with_iv_scalars`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scalars: Vec<ScalarVar>,

    /// Phase 9a: the statement body. When `Some`, this is the CFG's source of
    /// truth. When `None`, [`crate::fud::build_cfg_from_loop`] synthesises a
    /// minimal body: one `i = i + step` at the latch per IV, and the existing
    /// `body: Vec<Access>` lowered to array statements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stmts: Option<Vec<crate::ir::stmt::Stmt>>,
}

/// Metadata describing a bounded pointer chase.
///
/// Example: `for (i = 0; i < N; i++) { p = next[p]; A[p] = A[p+1]; }`
/// with `next = [1, 2, 3, 4]`, `init = 0`, `max_steps = 4`.
///
/// M5 computes the chain `p0 = init, p_{k+1} = table[p_k]` up to
/// `max_steps` steps, refuses if any step is out of bounds, then
/// expands every access on `accessed_base` into one access per step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PointerChase {
    /// Name of the chase variable (`p`).
    pub var: String,
    /// The literal `next` table. Must be fully known.
    /// Kept for backward compatibility; use `table_expr` for non-literal tables.
    pub table: Vec<i64>,
    /// Optional scalar expression for the chase table.
    /// If `Some`, this is used instead of `table` and evaluated with `MemoryContext`.
    /// Enables chasing through non-literal tables (e.g., `Load { base, index }`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_expr: Option<ScalarExpr>,
    /// Initial value of `p`.
    pub init: i64,
    /// Maximum number of chase steps.
    pub max_steps: usize,
    /// Base array whose accesses use the chase variable as their index.
    pub accessed_base: String,
}

/// Metadata describing a bounded recursive function that is
/// behaviourally equivalent to a loop. The frontend recognizes
///     fn f(i, n, acc) { if (i <op> n) return; return f(i + step, n, acc); }
/// and emits a Loop with `recursion = Some(_)`.
///
/// M3 verifies the termination condition and rewrites to a standard
/// loop with `ivs = [{ name, lower, upper, step }]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Recursion {
    /// Function name (informational).
    pub func: String,
    /// IV parameter name. Must match the name used by accesses in `body`.
    pub iv_name: String,
    /// Initial value at the outermost call.
    pub iv_init: i64,
    /// Step per recursive call. Non-zero. Sign must be compatible with `bound_op`.
    pub iv_step: i64,
    /// Bound value.
    pub iv_bound: i64,
    /// Comparison: recursion continues while `iv <op> iv_bound`.
    pub bound_op: CmpOp,
    /// Parameters other than the IV that the body reads/writes (accumulators,
    /// arrays passed by reference). Informational; M3 does not use them for
    /// soundness beyond confirming the IV is the only induction variable.
    #[serde(default)]
    pub other_params: Vec<String>,
    /// M3.5: an optional companion function for mutual recursion `f <-> g`.
    /// When present, the effective per-round step is `iv_step +
    /// companion.iv_step`, and the recursion terminates at `iv_bound`.
    /// Both functions must share the IV name and bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion: Option<Box<Recursion>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IV {
    pub name: String,
    pub lower: i64,
    pub upper: i64,
    pub step: i64,
    /// Comparison operator for the loop guard (default Lt for backward compatibility).
    /// Used by M3 to recover the exact guard semantics from the CFG.
    #[serde(default)]
    pub bound_op: CmpOp,
}

impl IV {
    /// Create a new IV with the default bound operator (Lt).
    pub fn new(name: impl Into<String>, lower: i64, upper: i64, step: i64) -> Self {
        Self {
            name: name.into(),
            lower,
            upper,
            step,
            bound_op: CmpOp::default(),
        }
    }

    /// Create a new IV with a specific bound operator.
    pub fn with_bound_op(name: impl Into<String>, lower: i64, upper: i64, step: i64, bound_op: CmpOp) -> Self {
        Self {
            name: name.into(),
            lower,
            upper,
            step,
            bound_op,
        }
    }
}

/// M12: a bound that is either a constant or a runtime parameter name.
/// Resolved against an environment by `DynamicIV::resolve`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum BoundValue {
    Const(i64),
    Param(String),
}

impl BoundValue {
    pub fn is_const(&self) -> bool {
        matches!(self, BoundValue::Const(_))
    }
    pub fn as_const(&self) -> Option<i64> {
        match self {
            BoundValue::Const(v) => Some(*v),
            BoundValue::Param(_) => None,
        }
    }
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<i64> {
        match self {
            BoundValue::Const(v) => Some(*v),
            BoundValue::Param(name) => {
                env.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
            }
        }
    }
}

/// M12: an IV whose bounds may be runtime parameters. Parallel to `IV`
/// (which stays `i64`). Resolved to a standard `IV` before dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicIV {
    pub name: String,
    pub lower: BoundValue,
    pub upper: BoundValue,
    pub step: i64,
}

impl DynamicIV {
    /// Resolve both bounds against `env`, producing a standard `IV`.
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<IV> {
        Some(IV::new(
            self.name.clone(),
            self.lower.resolve(env)?,
            self.upper.resolve(env)?,
            self.step,
        ))
    }
}

/// M12: a loop whose IV bounds are `BoundValue`s. Parallel to `Loop`.
/// `m12::decide_with_env` resolves this to a standard `Loop` and
/// delegates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicLoop {
    pub depth: usize,
    pub ivs: Vec<DynamicIV>,
    pub body: Vec<Access>,
}

impl DynamicLoop {
    pub fn has_parametric_bounds(&self) -> bool {
        self.ivs
            .iter()
            .any(|iv| !iv.lower.is_const() || !iv.upper.is_const())
    }

    /// Resolve to a standard `Loop`. Returns None if any bound is
    /// unresolvable.
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<Loop> {
        let mut ivs = Vec::with_capacity(self.ivs.len());
        for iv in &self.ivs {
            ivs.push(iv.resolve(env)?);
        }
        Some(Loop {
            depth: self.depth,
            ivs,
            body: self.body.clone(),
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,})
    }
}


use crate::param_expr::ParamExpr;

/// An offset value can be a constant integer or a parametric expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetValue {
    Const(i64),
    Param(ParamExpr),
}

impl Serialize for OffsetValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            // JSON: untagged — serialize as the inner value
            match self {
                OffsetValue::Const(v) => v.serialize(s),
                OffsetValue::Param(e) => e.serialize(s),
            }
        } else {
            // Binary (bincode): tagged
            match self {
                OffsetValue::Const(v) => {
                    s.serialize_newtype_variant("OffsetValue", 0, "Const", v)
                }
                OffsetValue::Param(e) => {
                    s.serialize_newtype_variant("OffsetValue", 1, "Param", e)
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for OffsetValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            // JSON: untagged — try Const first, then Param
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum JsonForm {
                Const(i64),
                Param(crate::param_expr::ParamExpr),
            }
            match JsonForm::deserialize(d)? {
                JsonForm::Const(v) => Ok(OffsetValue::Const(v)),
                JsonForm::Param(e) => Ok(OffsetValue::Param(e)),
            }
        } else {
            // Binary: tagged
            #[derive(Deserialize)]
            enum BinaryForm {
                Const(i64),
                Param(crate::param_expr::ParamExpr),
            }
            match BinaryForm::deserialize(d)? {
                BinaryForm::Const(v) => Ok(OffsetValue::Const(v)),
                BinaryForm::Param(e) => Ok(OffsetValue::Param(e)),
            }
        }
    }
}

impl OffsetValue {
    pub fn as_const(&self) -> Option<i64> {
        match self {
            OffsetValue::Const(v) => Some(*v),
            // A ParamExpr that is itself a Const is semantically a const.
            // The untagged JSON deserializer produces Param(Const(n)) for
            // {"Const": n}, so this fallthrough is essential.
            OffsetValue::Param(e) => e.as_const(),
        }
    }
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<i64> {
        match self {
            OffsetValue::Const(v) => Some(*v),
            OffsetValue::Param(e) => e.eval(env),
        }
    }
    pub fn is_const(&self) -> bool {
        self.as_const().is_some()
    }
}

impl Default for OffsetValue {
    fn default() -> Self { OffsetValue::Const(0) }
}


impl std::fmt::Display for OffsetValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffsetValue::Const(v) => write!(f, "{}", v),
            OffsetValue::Param(e) => write!(f, "{}", e.to_string_repr()),
        }
    }
}


impl OffsetValue {
    pub fn to_param_expr(&self) -> crate::param_expr::ParamExpr {
        use crate::param_expr::ParamExpr;
        match self {
            OffsetValue::Const(v) => ParamExpr::Const(*v),
            OffsetValue::Param(e) => e.clone(),
        }
    }
}

impl From<i64> for OffsetValue {
    fn from(v: i64) -> Self { OffsetValue::Const(v) }
}

/// M6: symbolic index expression tree for products (`i*j`, `i*i`, `i*i*j`).
/// Extended with `Load` for symbolic pointer chase (M11 / Phase 5.3).
/// Extended with `Div`, `Mod`, `Min`, `Max`, `Cmp` for non-affine indices (Item 2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexExpr {
    IV(usize),
    Const(i64),
    Add(Box<IndexExpr>, Box<IndexExpr>),
    Mul(Box<IndexExpr>, Box<IndexExpr>),
    /// Integer division: `a / b`. Requires `b != 0`, exact division (remainder == 0).
    Div(Box<IndexExpr>, Box<IndexExpr>),
    /// Euclidean remainder: `a % b`. Requires `b > 0`.
    Mod(Box<IndexExpr>, Box<IndexExpr>),
    /// Minimum of two expressions.
    Min(Box<IndexExpr>, Box<IndexExpr>),
    /// Maximum of two expressions.
    Max(Box<IndexExpr>, Box<IndexExpr>),
    /// Comparison: `a < b`, `a <= b`, `a > b`, `a >= b`, `a == b`, `a != b`.
    /// Evaluates to 1 if true, 0 if false.
    Cmp(Box<IndexExpr>, Box<IndexExpr>, CmpOp),
    /// Memory load: `base[index]` where `base` is a scalar index and `index` is an expression.
    /// Enables pointer chasing through non-literal tables (Phase 5.3).
    Load { base: usize, index: Box<IndexExpr> },
}

impl IndexExpr {
    /// Evaluate the expression at concrete IV and scalar values.
    /// Returns `None` on overflow, out-of-range index, division by zero,
    /// non-exact division, missing memory context for Load, or invalid comparison.
    pub fn eval(
        &self,
        iv_values: &[i64],
        scalars: &[i64],
        memory: Option<&crate::ir::stmt::MemoryContext>,
    ) -> Option<i64> {
        match self {
            IndexExpr::IV(k) => iv_values.get(*k).copied(),
            IndexExpr::Const(c) => Some(*c),
            IndexExpr::Add(a, b) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                x.checked_add(y)
            }
            IndexExpr::Mul(a, b) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                x.checked_mul(y)
            }
            IndexExpr::Div(a, b) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                if y == 0 { return None; }
                if x % y != 0 { return None; } // Require exact division
                Some(x / y)
            }
            IndexExpr::Mod(a, b) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                if y <= 0 { return None; }
                Some(x.rem_euclid(y))
            }
            IndexExpr::Min(a, b) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                Some(x.min(y))
            }
            IndexExpr::Max(a, b) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                Some(x.max(y))
            }
            IndexExpr::Cmp(a, b, op) => {
                let x = a.eval(iv_values, scalars, memory)?;
                let y = b.eval(iv_values, scalars, memory)?;
                let res = match op {
                    CmpOp::Lt => x < y,
                    CmpOp::Le => x <= y,
                    CmpOp::Gt => x > y,
                    CmpOp::Ge => x >= y,
                };
                Some(if res { 1 } else { 0 })
            }
            IndexExpr::Load { base, index } => {
                let mem = memory?;
                let base_val = scalars.get(*base).copied()?;
                let idx = index.eval(iv_values, scalars, memory)?;
                mem.read(base_val, idx)
            }
        }
    }

    /// Rough size of the enumerated value set for a depth-1 IV range.
    /// Returns `None` if the expression is nonlinear in a way that
    /// isn't a product (currently only IV, Const, Add, Mul, Div, Mod, Min, Max, Cmp, Load are supported).
    pub fn max_degree(&self) -> u32 {
        match self {
            IndexExpr::IV(_) => 1,
            IndexExpr::Const(_) => 0,
            IndexExpr::Add(a, b) => a.max_degree().max(b.max_degree()),
            IndexExpr::Mul(a, b) => a.max_degree() + b.max_degree(),
            IndexExpr::Div(a, b) => a.max_degree() + b.max_degree(),
            IndexExpr::Mod(a, b) => a.max_degree() + b.max_degree(),
            IndexExpr::Min(a, b) => a.max_degree().max(b.max_degree()),
            IndexExpr::Max(a, b) => a.max_degree().max(b.max_degree()),
            IndexExpr::Cmp(a, b, _) => a.max_degree().max(b.max_degree()),
            IndexExpr::Load { index, .. } => index.max_degree(),
        }
    }
}

/// M7: a single-IV hash function of the form
///     ((iv * c) >> shift) & mask
/// where `>>` is logical right shift on unsigned 64-bit, and `&` is
/// bitwise AND. All arithmetic wraps at 64 bits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HashExpr {
    /// Multiplier (wraps at 2^64).
    pub c: i64,
    /// Logical right shift amount (0..=63).
    pub shift: u32,
    /// Bitwise AND mask (applied after shift).
    pub mask: i64,
}

impl HashExpr {
    /// Evaluate the hash at a concrete IV value.
    /// Uses unsigned 64-bit arithmetic for the multiply and shift, then
    /// masks. Returns the result as `i64` (same bit pattern).
    pub fn eval(&self, iv_value: i64) -> i64 {
        let x = (iv_value as u64).wrapping_mul(self.c as u64);
        let shifted = if self.shift >= 64 { 0 } else { x >> self.shift };
        let masked = shifted & (self.mask as u64);
        masked as i64
    }
}

/// M8: a linear recurrence of order `coeffs.len()`, seeded by
/// `initial` (the first `coeffs.len()` values), evaluated at each IV
/// value of a depth-1 loop:
///     x[k] = Σ coeffs[j] * x[k - 1 - j]   for k >= coeffs.len()
/// with `initial = [x[0], x[1], ..., x[coeffs.len()-1]]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecurrenceId {
    pub initial: Vec<i64>,
    pub coeffs: Vec<i64>,
}

impl RecurrenceId {
    /// Evaluate the recurrence at index `k`.
    /// `k < initial.len()` returns the seed.
    /// `k >= initial.len()` iterates from the seed forward.
    /// Returns `None` on overflow or malformed recurrence.
    pub fn eval(&self, k: usize) -> Option<i64> {
        let n = self.coeffs.len();
        if self.initial.len() != n || n == 0 {
            return None;
        }
        if k < n {
            return Some(self.initial[k]);
        }
        let mut seq: Vec<i64> = self.initial.clone();
        for idx in n..=k {
            let mut v: i64 = 0;
            for j in 0..n {
                let c = self.coeffs[j];
                let prev = seq[idx - 1 - j];
                v = v.checked_add(c.checked_mul(prev)?)?;
            }
            seq.push(v);
        }
        Some(seq[k])
    }
}

/// M10: a functional-graph fixed-point table. The subscript is the
/// fixed point reached by starting at `iv[0]` and repeatedly applying
/// `next[x]` until `next[x] == x`, or detecting a cycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FixedPointTable {
    pub next: Vec<i64>,
}

impl FixedPointTable {
    /// Follow `next` from `start` until a fixed point is reached or a
    /// cycle is detected. Returns `Some(fixed_point)` on success, or
    /// `None` if `start` is out of bounds or a cycle (non-trivial) is
    /// encountered before reaching a fixed point.
    pub fn eval(&self, start: i64) -> Option<i64> {
        if start < 0 || (start as usize) >= self.next.len() {
            return None;
        }
        let mut x = start;
        let mut steps = 0usize;
        // Bound iterations at table length; if we exceed that, we're in
        // a cycle.
        while steps <= self.next.len() {
            if x < 0 || (x as usize) >= self.next.len() {
                return None;
            }
            let nx = self.next[x as usize];
            if nx == x {
                return Some(x);
            }
            x = nx;
            steps += 1;
        }
        None
    }
}

/// M6: how the index of an access is computed. Default is `Affine`
/// (existing M0–M5, M9 behavior). `Product` is M6's extension.
/// `Hash` is M7's extension.
/// `Recurrence` is M8's extension.
/// `FixedPoint` is M10's extension.
/// `Range` is the non-affine gray zone (Item 3).
/// `Polyhedral` is the multi-dimensional dependence polyhedron (Item 8).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexKind {
    /// Standard: index = `access.index · iv + offset`.
    Affine,
    /// M6: index = `eval(expr, iv) + offset`.
    Product(IndexExpr),
    /// M7: index = `hash(iv[0]) + offset`. Depth-1 only.
    Hash(HashExpr),
    /// M8: index = `recurrence.eval(iv[0]) + offset`. Depth-1 only.
    Recurrence(RecurrenceId),
    /// M10: index = `fixed_point.eval(iv[0]) + offset`. Depth-1 only.
    FixedPoint(FixedPointTable),
    /// M11: index = symbolic fixed point of `next_expr` starting from `iv[0]`.
    /// Evaluated up to `max_steps`. If no fixed point is reached, returns None.
    SymbolicFixedPoint { next_expr: IndexExpr, max_steps: usize },
    /// Item 3: non-affine gray zone - index is a bounded range expression.
    /// The checker verifies range disjointness instead of exact locations.
    Range(RangeExpr),
    /// Item 8: full multi-dimensional dependence polyhedron.
    /// Carries the constraint matrix for the dependence system.
    Polyhedral {
        /// Constraint matrix A such that A * [i, j] >= b
        a: Vec<Vec<i64>>,
        b: Vec<i64>,
        /// Dimension of iteration vectors
        dim: usize,
    },
}

impl Default for IndexKind {
    fn default() -> Self { IndexKind::Affine }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Access {
    pub kind: AccessKind,
    pub base: String,
    /// Affine coefficients over the IVs for the OUTER index expression.
    ///
    /// Convention A (M2a): when `indirect` is `Some(_)`, `index` describes
    /// the affine arithmetic *around* the indirect term, NOT B's coefficients.
    /// Example: `A[B[i] + 2]` with `B[i] = 3i + 1` has
    ///     index  = [0]   (no outer IV term; the "+2" is the outer offset)
    ///     offset = 2
    ///     indirect = { base: "B", index: [3], offset: 1, ... }
    /// Flattening = coefficient addition:
    ///     flat.index  = index + indirect.index = [3]
    ///     flat.offset = offset + indirect.offset = 3
    /// giving `A[3i + 3]`, which is standard M0 input.
    pub index: Vec<i64>,
    pub elem_size: i64,
    #[serde(default)]
    pub offset: OffsetValue,
    /// Surface expression for this access (e.g. "(i << 1) | 1").
    /// When `Some(_)`, `index` and `offset` are stale placeholders and
    /// MUST NOT be used by M0. The dispatcher routes such loops to M2g,
    /// which normalizes `expr` into `index`/`offset` and clears the field
    /// before M0 sees the loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    /// Indirection: when this access reads/writes `A[B[i]]` or `A[B[i] + k]`,
    /// `indirect` describes the inner `B[i]`. See Convention A on `index`.
    /// Cleared by M2a once injectivity is proven and the access is flattened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indirect: Option<Indirect>,
    /// Affine guard: the access executes only when all `guard.constraints`
    /// hold. Each constraint is a constant-bounded comparison on a single IV
    /// (`<`, `<=`, `>`, `>=`). `==`, `!=`, IV-vs-IV, and disjunctions are
    /// out of scope — those are Unknown.
    ///
    /// M4 narrows the effective IV domain by intersecting this guard with
    /// the base IV range and with the other access's guard, then hands the
    /// narrowed loop to M0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<Guard>,
    /// Bounded-range index expression (`A[i % C]`, `A[min(i,K)]`,
    /// `A[max(i,K)]`). When `Some(_)`, `index`/`offset` are stale
    /// placeholders. M2b decides directly on the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeExpr>,
    /// M6: how the index is computed. `Affine` (default) uses
    /// `index · iv + offset`. `Product(e)` uses `eval(e, iv) + offset`.
    #[serde(default, skip_serializing_if = "is_default_index_kind")]
    pub index_kind: IndexKind,
}

fn is_default_index_kind(k: &IndexKind) -> bool {
    matches!(k, IndexKind::Affine)
}

/// A conjunction of constant-bounded comparisons on the IVs.
/// Semantics: the access executes only when ALL comparisons hold.
///
/// Example: `if (i < 32 && j >= 4) A[i][j] = ...`
///   guard = Guard { constraints: [
///       Cmp { iv: 0, op: Lt, bound: 32 },
///       Cmp { iv: 1, op: Ge, bound: 4 },
///   ]}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Guard {
    pub constraints: Vec<Cmp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cmp {
    /// Index into `Loop.ivs`, i.e. which IV this comparison is on.
    pub iv: usize,
    pub op: CmpOp,
    /// Constant bound. Parametric bounds are M1's job, not M4's.
    pub bound: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CmpOp {
    #[default]
    Lt,
    Le,
    Gt,
    Ge,
}

/// Bounded-range index expression: the access covers a set of locations
/// that is not affine, but whose range is a bounded interval computable
/// from the IV bounds.
///
/// When `Access.range` is `Some(_)`, `index`/`offset` are stale placeholders
/// and MUST NOT be read by M0/M2a/M4. M2b decides directly on the range and
/// emits certificates that the checker verifies via range membership.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RangeExpr {
    /// `A[i % C]` — locations are `{0, 1, ..., C-1}` for any IV domain.
    /// Requires `C > 0`.
    Mod { iv: usize, modulus: i64 },
    /// `A[i % p]` with runtime modulus `p` (M9). The modulus is a
    /// `ParamExpr` resolved against an environment. When the environment
    /// binds `p`, this behaves identically to `Mod { modulus: p }`.
    /// Requires `p > 0` after resolution.
    ModParam { iv: usize, modulus: crate::param_expr::ParamExpr },
    /// `A[min(i, K)]` — locations are `[iv.lower, min(iv.upper - 1, K)]`.
    /// Requires `K >= iv.lower`.
    Min { iv: usize, cap: i64 },
    /// `A[max(i, K)]` — locations are `[max(iv.lower, K), iv.upper - 1]`.
    Max { iv: usize, floor: i64 },
}

/// Indirection descriptor: the inner `B[i]` of an access like `A[B[i] + k]`.
///
/// M2a v1 supports only affine `B`: `B[i] = c * i + d` with `c != 0`.
/// The `table` field is reserved for a future M2a.5 path (literal table)
/// and MUST be `None` in v1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Indirect {
    /// Name of the indirection array, e.g. "B" in A[B[i]].
    pub base: String,
    /// Affine coefficients of `B` over the IVs. Same arity as `Access.index`.
    pub index: Vec<i64>,
    /// Constant term of `B`.
    #[serde(default)]
    pub offset: i64,
    /// Reserved for M2a.5 (literal-table injectivity). Must be None in v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<Vec<i64>>,
    /// IV domain over which injectivity is claimed: [domain_lo, domain_hi).
    /// For depth-1 loops this is the IV range; for higher depth it is the
    /// outer-product range. Explicit so the check does not need to re-derive
    /// bounds from the IV list (which may be parametric in M1).
    pub domain_lo: i64,
    pub domain_hi: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccessKind {
    Read,
    Write,
}

impl Loop {
    /// Register the IVs as scalars `0..depth`, after any already present.
    /// Returns the same loop with `scalars` populated. Idempotent: an IV whose
    /// name is already declared is not added twice.
    pub fn with_iv_scalars(mut self) -> Loop {
        for iv in &self.ivs {
            if !self.scalars.iter().any(|s| s.name == iv.name) {
                self.scalars.push(ScalarVar {
                    name: iv.name.clone(),
                    ty: ScalarTy::I64,
                });
            }
        }
        self
    }

    /// Does this loop carry scalar statements (explicit or synthesizable)?
    pub fn has_scalars(&self) -> bool {
        !self.ivs.is_empty() || !self.scalars.is_empty()
    }

    /// The scalar index of the k-th IV. By convention, IVs are registered
    /// first, so the index is `k`.
    pub fn iv_scalar_index(&self, k: usize) -> usize {
        k
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.depth == 0 || self.depth > 4 {
            return Err(format!("depth {} out of range [1,4]", self.depth));
        }
        if self.ivs.len() != self.depth {
            return Err(format!("depth {} but {} IVs", self.depth, self.ivs.len()));
        }
        // When `recursion` is Some, `ivs` are placeholders — M3 will
        // replace them from the recursion metadata before M0 sees the
        // loop. Skip IV range validation in that case.
        if self.recursion.is_none() {
            for iv in &self.ivs {
                if iv.step <= 0 {
                    return Err(format!("IV {} step must be > 0", iv.name));
                }
                if iv.lower >= iv.upper {
                    return Err(format!("IV {} lower >= upper", iv.name));
                }
            }
        }
        for a in &self.body {
            if a.index.len() != self.depth {
                return Err(format!(
                    "access index len {} != depth {}",
                    a.index.len(),
                    self.depth
                ));
            }
            if a.elem_size <= 0 {
                return Err("elem_size must be > 0".into());
            }
        }
        Ok(())
    }

    pub fn canonical_json(&self) -> String {
        let mut ivs = self.ivs.clone();
        ivs.sort_by(|a, b| a.name.cmp(&b.name));
        let canon = Loop {
            depth: self.depth,
            ivs,
            body: self.body.clone(),
            recursion: self.recursion.clone(),
            chase: self.chase.clone(),
            assertions: vec![],
            scalars: self.scalars.clone(),
            stmts: self.stmts.clone(),
        };
        serde_json::to_string(&canon).unwrap()
    }

    pub fn hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.canonical_json().as_bytes());
        format!("{:x}", h.finalize())
    }
}
// ---- Binary format (bincode) — Lever 1 ----

impl Loop {
    /// Serialize to bincode (fast binary format).
    pub fn to_bincode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("bincode serialize")
    }

    /// Deserialize from bincode.
    pub fn from_bincode(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

// ============================================================================
// Arena-borrowed types (Option A — zero-allocation parsing)
// ============================================================================

/// A loop whose data is borrowed from an arena.
/// All strings and vectors live in the arena's buffer.
#[derive(Debug, Clone, Copy)]
pub struct LoopRef<'a> {
    pub depth: usize,
    pub ivs: &'a [IVRef<'a>],
    pub body: &'a [AccessRef<'a>],
}

#[derive(Debug, Clone, Copy)]
pub struct IVRef<'a> {
    pub name: &'a str,
    pub lower: i64,
    pub upper: i64,
    pub step: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct AccessRef<'a> {
    pub kind: AccessKind,
    pub base: &'a str,
    pub index: &'a [i64],
    pub elem_size: i64,
    pub offset: i64,
}

impl<'a> LoopRef<'a> {
    pub fn validate(&self) -> Result<(), String> {
        if self.depth == 0 || self.depth > 4 {
            return Err(format!("depth {} out of range [1,4]", self.depth));
        }
        if self.ivs.len() != self.depth {
            return Err(format!("depth {} but {} IVs", self.depth, self.ivs.len()));
        }
        for iv in self.ivs {
            if iv.step <= 0 {
                return Err(format!("IV {} step must be > 0", iv.name));
            }
            if iv.lower >= iv.upper {
                return Err(format!("IV {} lower >= upper", iv.name));
            }
        }
        for a in self.body {
            if a.index.len() != self.depth {
                return Err(format!("access index len {} != depth {}", a.index.len(), self.depth));
            }
            if a.elem_size <= 0 {
                return Err("elem_size must be > 0".into());
            }
        }
        Ok(())
    }
}

// ============================================================================
// M1 — Parametric bounds
// ============================================================================

/// A bound can be a constant or a parameter name (resolved at runtime).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Bound {
    Const(i64),
    Param(String),
}

impl Bound {
    pub fn is_const(&self) -> bool {
        matches!(self, Bound::Const(_))
    }
    pub fn as_const(&self) -> Option<i64> {
        match self {
            Bound::Const(v) => Some(*v),
            Bound::Param(_) => None,
        }
    }
    /// Resolve to a concrete i64 using the provided environment.
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<i64> {
        match self {
            Bound::Const(v) => Some(*v),
            Bound::Param(name) => env.iter().find(|(n, _)| n == name).map(|(_, v)| *v),
        }
    }
}

/// A loop with parametric bounds (M1 form).
/// This is separate from Loop so M0's parser is unaffected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopParam {
    pub depth: usize,
    pub ivs: Vec<IVParam>,
    pub body: Vec<Access>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IVParam {
    pub name: String,
    pub lower: Bound,
    pub upper: Bound,
    pub step: i64,
}

impl LoopParam {
    /// Resolve all parametric bounds to constants given an environment.
    /// Returns a Loop (M0 form) if all bounds resolve.
    pub fn resolve(&self, env: &[(String, i64)]) -> Option<Loop> {
        let mut ivs = Vec::with_capacity(self.ivs.len());
        for iv in &self.ivs {
            let lower = iv.lower.resolve(env)?;
            let upper = iv.upper.resolve(env)?;
            ivs.push(IV::new(iv.name.clone(), lower, upper, iv.step));
        }
        Some(Loop {
            depth: self.depth,
            ivs,
            body: self.body.clone(),
            recursion: None,
            chase: None,
            assertions: vec![],
         scalars: vec![], stmts: None,})
    }

    /// Does this loop have any parametric bounds?
    pub fn is_parametric(&self) -> bool {
        self.ivs.iter().any(|iv| !iv.lower.is_const() || !iv.upper.is_const())
    }
}
