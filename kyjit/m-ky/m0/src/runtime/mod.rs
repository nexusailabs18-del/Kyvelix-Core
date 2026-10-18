//! Phase 5 §5.9 — run-time dependence testing (Wolfe §7.6).
//!
//! **The certificate-verifiable half of this subsystem is now implemented; the
//! emitted-code half is still not, and for the reasons below.**
//!
//! ## What is implemented (the verifiable half)
//!
//! `claim::Claim::RuntimeCheck { legs }` / `Witness::RuntimeCheckEvidence`, the
//! `emit::runtime_check_certificate` producer, and the `m0-check` arm verify the
//! *analysis* §7.6 depends on: for the canonical shape `A[i] = A[i-K] + B[i]`,
//! the emitter derives the run-time predicate `Ɐ i. NonStraddling(i) ||
//! ByteEqual(i)` and the checker re-derives the same legs from the loop on disk.
//! What is certified is not "the accesses are independent" — they are not — but
//! "this is the run-time test that licenses the versioned loop". That is a claim
//! the replay architecture *can* check, and `tests/phase5_runtime.rs` proves it
//! end to end through the real binary. The predicate legs are
//! [`crate::certificate::RuntimeCheckLeg`].
//!
//! ## What is still not implemented, and why
//!
//! The spec's §5.9 also asks for the *emitted code*: a `test_loop`, a
//! `parallel_version` and a `serial_version` that a backend would run. UDDP has
//! no runtime (`remaining.md`: "Runtime (thread pool, GC)" is not done) and no
//! backend, so there is nothing to emit into and nothing that could call a
//! `test_loop`. The vocabulary for those three shapes is kept below
//! ([`RuntimeCheckKind`]) so a later backend has a name to target, but **no
//! analyzer constructs them and no checker arm accepts them** — a certificate
//! carrying an emitted-code shape with no runtime to observe it would be exactly
//! the unverifiable trust the certificate layer forbids.
//!
//! This is a narrower gap than the original note: the soundness obligation that
//! *can* be discharged statically (the test is the right one for the pair) is
//! discharged, certified and fuzz-checked. What remains is the data-conditional
//! half, which needs a runtime and a format extension no phase has built.
//!
//! ```text
//! RuntimeCheckCertificate {   // emitted-code shape — NOT implemented
//!     loop_hash, test_loop, parallel_version, serial_version,
//! }
//! RuntimeCheckKind::{DisjointArrays, NoDistanceK, GuardCheck}  // below
//! ```

/// The shape a future §7.6 generator would emit. Declared so the vocabulary exists
/// for the phase that builds the generator; **no analyzer constructs these and no
/// checker arm accepts them.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCheckKind {
    /// Check that no dependence exists: for all `i`, `j`: `A[i] != B[j]`.
    DisjointArrays,
    /// Check that a specific distance does not occur: `A[i] != A[i + K]`.
    NoDistanceK(usize),
    /// Assert that a loop-invariant guard holds for the whole loop.
    GuardCheck(String),
}
