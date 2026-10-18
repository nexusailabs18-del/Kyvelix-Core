# Phase 3 — FUD Chains: Status Report

Last updated: after Phase 3.1–3.7 (IR + CFG, dominators, φ-placement,
Algorithm Chain, FollowChain, FRUD chains, certificate vocabulary + checker
arm) and the Phase 2 finalization (M6 closed-form large-range path, M9 exact
congruence search).

This is the audit of `phase_3.txt` / `refernce.txt` Ch. 6 against the tree.
Every number below was produced by running the command shown, not by reading
code and assuming.

---

## 1. Summary

| § | Deliverable | Status |
|---|---|---|
| 3.1 | IR extensions (`scalar.rs`, `cfg.rs`) + CFG construction | ✅ |
| 3.2 | Dominators (Cooper-Harvey-Kennedy) + dominance frontiers | ✅ |
| 3.3 | φ-placement via `DF⁺` (PlacementDirect) | ✅ |
| 3.4 | Algorithm Chain (forward + **reverse pass**) | ✅ |
| 3.5 | Algorithm FollowChain (reaching definitions) | ✅ |
| 3.6 | FRUD chains (def → uses) | ✅ |
| 3.7 | `FudChainEvidence` witness, `ScalarReach`/`ScalarNoReach` claims, checker arm | ✅ |
| 3.8 | Fuzz tests + seed-and-revert validation | ✅ (12 fud fuzz tests) |

**Verified state:** `cargo test --release` → **362 tests, 0 failures**
(327 lib + 21 checker + 7 phase2_e2e + 7 phase3_e2e). All 19 level runners
green; real corpus 1,001 loops 100.0% coverage / 0 rejects; `tests/fuzz.sh
3000` clean.

---

## 2. The module structure (as built)

```
src/ir/scalar.rs        141   ScalarVar/Use/Def, PhiTerm, UpsilonTerm, ids
src/ir/cfg.rs           465   CFG/Block/Stmt + validate/index/reversed
src/fud/mod.rs           41   re-exports
src/fud/cfg.rs          266   build_cfg_from_loop / _from_stmts / cfg_matches_loop
src/fud/dominators.rs   542   CHK idom, DF, DF⁺
src/fud/placement.rs    456   φ-placement at DF⁺(D(M))
src/fud/chain.rs        508   Algorithm Chain (+ reverse pass)
src/fud/reaching.rs     354   Algorithm FollowChain
src/fud/frud.rs         302   FRUD chains (def → uses)
src/fud/testgen.rs      142   shared random-CFG generator (test-only)
                   total 3217
```

The CFG the current IR produces is the canonical single block
`entry → body → exit`. The data model and every algorithm are general over
arbitrary reducible CFGs, because Phase 9's frontend will emit branches and
the machinery must already be correct for them.

---

## 3. What is done, with evidence

### 3.1 IR + CFG

`ScalarVar`, `ScalarTy`, `ScalarUse`, `ScalarDef`, `PhiTerm`, `UpsilonTerm`,
positional ids (`UseId`/`DefId`/`PhiId`/`UpsilonId`), and the `CFG` with
`validate`, `index`, `reversed`.

- `single_block_shape_is_canonical`, `index_numbering_is_canonical`,
  `validate_catches_asymmetric_edges`, `validate_catches_var_out_of_range`,
  `reversed_swaps_edges_and_entry_exit`, `cfg_round_trips_through_json`,
  `phi_and_upsilon_round_trip_and_validate`.
- `cfg_single_block_is_trivial`, `cfg_from_ir_preserves_statement_order`,
  `cfg_succ_pred_are_consistent`, `cfg_matches_loop_accepts_matching_shape`,
  `cfg_matches_loop_rejects_wrong_statement_count`,
  `build_cfg_from_stmts_carries_uses_and_defs`.
- `fuzz_cfg_well_formed` (2,000 random loops),
  `fuzz_shuffled_defs_and_uses_positions` (1,000 random scalar programs).

### 3.2 Dominators + frontiers

`compute_dominators` (CHK iterative) and `compute_dominance_frontiers`
(`DF` by the runner walk, `DF⁺` as the transitive closure).

- `dom_entry_dominates_everything`, `dom_reflexive`, `dom_idom_is_immediate`,
  `df_diamond_and_loop`, `df_plus_is_transitive_closure_on_hand_cfg`.
- `fuzz_dominators_brute_force` (1,500 CFGs vs "is there a path avoiding X"),
  `fuzz_idom_vs_dom`, `fuzz_df_brute_force` (vs the `DF` definition),
  `fuzz_df_plus_is_transitive_closure`.

### 3.3 φ-placement via DF⁺

`place_phi_terms` = `DF⁺(D(M))` restricted to merge blocks, justified by
Wolfe §6.1.3 (`DF⁺(S) = J⁺(S)`).

- `phi_at_single_def_is_none`, `phi_at_two_defs_in_branches`,
  `phi_at_nested_branches`.
- `fuzz_phi_placement_vs_bruteforce` (800 DAGs vs a `DF⁺` oracle),
  `fuzz_dominant_frontier_equals_join_set` — computes `J⁺` from its
  definition (disjoint simple paths) and checks `DF⁺ = J⁺` directly,
  `fuzz_phi_no_duplicates`.

### 3.4 Algorithm Chain (the reverse pass)

`construct_chains` links every use to its reaching def or φ-term; the reverse
pass restores `CurrDef` on the way out of each dominator subtree.

- `chain_linear_is_simple`, `chain_use_before_def_is_uninitialized`,
  `chain_reverse_pass_restores` (the classic bug), `chain_merge_goes_through_phi`,
  `chain_branch_takes_correct_def`.
- `fuzz_chain_single_block_vs_bruteforce` (3,000 single-block loops vs a
  scan-in-order oracle), `fuzz_chain_dag_def_link_dominates_use`
  (1,500 DAGs; a reaching def's block must dominate the use's block).

**Seed-and-revert validation:** disabling the reverse pass makes
`chain_reverse_pass_restores` and `fuzz_chain_dag_def_link_dominates_use`
both FAIL; restoring it returns them to 0. (Performed this session.)

### 3.5 FollowChain

`find_reaching_defs` walks the chain to the *full* set of reaching defs,
recurring through φ-incoming links with a per-use marked set (cycle guard).
Scalars are killing, so a direct def link terminates.

- `reaching_single_def`, `reaching_killed_by_overwrite`,
  `reaching_through_phi`, `reaching_branch_conditional`.
- `fuzz_reaching_vs_bruteforce` (1,500 pure-block DAGs vs a path oracle,
  0 mismatches).

### 3.6 FRUD chains

`construct_frud_chains` builds the anti-dependence direction (def → uses) as
the transpose of the reaching-defs relation — provably equal to Wolfe
§6.7.5's reverse-Chain pass for the killing (scalar) definitions Phase 3
handles. The direct reverse pass becomes necessary only for non-killing array
definitions (Phase 6); the transpose is retained as the oracle.

- `frud_linear`, `frud_def_reaches_all_later_uses_until_redefined`,
  `frud_merge_links_both_defs`, `frud_branch_links_local_def`.
- `fuzz_frud_vs_bruteforce` (1,500 pure-block DAGs vs a path oracle,
  0 mismatches).

### 3.7 Certificate vocabulary + checker arm

- `Claim::ScalarReach { use_id, def_id }` and `Claim::ScalarNoReach { use_id }`.
- `Witness::FudChainEvidence { cfg, asserted_use, asserted_def }`.
- `src/checker/main.rs`: the `FudChainEvidence` arm **rebuilds** the chain
  from the carried CFG (`build_chain_with_phi` + `find_reaching_defs`) and
  verifies the asserted link against the rebuilt reaching set. It also
  validates the CFG, checks id bounds, and enforces variable agreement. It
  never trusts the analyzer's chain — the same "trust nothing" discipline as
  `WithLoop` and `CrossLoopSolution`.
- `tests/phase3_e2e.rs` (7 cases through the real `m0-check` binary):
  - ACCEPT: `scalar_reach_is_accepted`, `scalar_no_reach_is_accepted`.
  - REJECT: `cross_variable_reach_is_rejected`,
    `scalar_no_reach_where_reachable_is_rejected`,
    `scalar_reach_where_uninitialized_is_rejected`,
    `mismatched_claim_and_evidence_are_rejected`,
    `malformed_carried_cfg_is_rejected`.

---

## 4. Phase 2 finalization (closed this session)

`refernce.txt` §2.8 left two performance items open; both are now closed.
See `PHASE2_STATUS.md` §2.8 and §8 for the full write-up.

- **M6** (`src/decide/m6.rs`): beyond the 4,096-pair enumeration cap, a
  closed-form path reduces the quadratic Diophantine `P(t) = Q(u)` to a
  linear one by substitution and solves the other side exactly (division /
  quadratic formula). E2E `p003`/`p004`; fuzz
  `fuzz_m6_large_witness_vs_enumeration`.
- **M9** (`src/decide/m2b.rs`): the capped scan and the residue-below-`lower`
  bug are gone; realized sets are closed-form and the smallest common element
  is exact (extended-gcd Diophantine for progression ∩ progression). E2E
  `n003`; fuzz `fuzz_exact_search_vs_brute` (which also exposed and fixed a
  pre-existing saturated-`Min`/`Max` over-conservatism).

---

## 5. How to reproduce all of this

```bash
cd m0

# 362 tests
cargo test --release

# Level / bug / real suites
bash tests/run_all.sh            # 6/0 checker, 0 UNKNOWN
bash tests/run_real.sh           # 1001 loops, 100% coverage, 0 rejects
bash tests/run_negative.sh       # checker sound on all negative cases
bash tests/run_m6_tests.sh       # 5/5 (incl. large-range p003/p004)
bash tests/run_m9_tests.sh       # 4/4 (incl. n003)

# Phase 3 E2E through the real checker binary
cargo test --release --test phase3_e2e     # 7/7

# Fuzz
bash tests/fuzz.sh 3000          # RESULT: fuzz clean
```

---

## 6. Honest statement of scope

Phase 3 builds the **data structure**. It does not yet *consume* it:

- IV detection, constant propagation, and scalar dependence are **Phase 4**.
- The CFG is single-block because the current IR has no branches; the
  φ-placement / Chain / FollowChain machinery is built and fuzzed against
  branching CFGs so Phase 9's frontend can use it directly.
- `cfg_matches_loop` checks only the carried CFG's *shape* against the loop
  file (the current IR cannot express scalar statements). The checker still
  rebuilds the chain from the carried CFG, so a forged chain is impossible;
  a full structural cross-check is a Phase 9 item.

The one consumer Phase 3 should wire (a top-level `decide_with_scalars`)
is deferred to Phase 4, as `phase_3.txt` §3.9 specifies: Phase 3 exposes the
API; Phase 4 calls it.
