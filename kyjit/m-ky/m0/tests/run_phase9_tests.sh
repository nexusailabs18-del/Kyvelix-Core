#!/usr/bin/env bash
# Phase 9 gate — every §9 feature, standalone and end to end.
#
# Mirrors run_phase8_tests.sh: one PASS/FAIL line per check, a non-zero exit when
# anything failed, and the repo root as the working directory (the binaries and
# the fixtures are both addressed relative to it).
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

PASS=0
FAIL=0

check() {
    local name="$1"
    shift
    if "$@" > /tmp/p9-gate.out 2>&1; then
        echo "PASS: $name"
        PASS=$((PASS + 1))
    else
        echo "FAIL: $name"
        tail -20 /tmp/p9-gate.out
        FAIL=$((FAIL + 1))
    fi
}

echo "== Phase 9 §9.1-§9.14 =="

# §9.1-§9.3 lexer / parser / AST, and the recursion guard.
check "lib frontend (lexer, parser, ctrl_edges, loopkinds)" \
    cargo test --release --lib --quiet frontend::
# §9.3 fixtures: the five kernels parse and their ASTs are what §9.4 expects.
check "kernels parse (§9.3)" \
    cargo test --release --quiet --test phase9_kernels
# §9.1 parser diagnostics.
check "parser surface (§9.1)" \
    cargo test --release --quiet --test phase9_parser
# §9.4-§9.7, §9.14 lowering, with the refusals named.
check "lowering (§9.4-§9.7, §9.14)" \
    cargo test --release --quiet --test phase9_lower
# §9.12 differential fuzz, with the generator as its own oracle.
check "lowering fuzz (§9.12)" \
    cargo test --release --quiet --test phase9_fuzz
# §9.11 control dependence.
check "control edges unit tests (§9.11)" \
    cargo test --release --lib --quiet ctrl_edges::
# §9.10 loop kinds.
check "loop kinds unit tests (§9.10)" \
    cargo test --release --lib --quiet loopkinds::
# §9.12 end to end: source -> IR -> m0-decide -> m0-check.
check "source to certificate e2e (§9.12)" \
    cargo test --release --quiet --test phase9_e2e
# §9a: the two loop-level claims §4.3 adds, replayed by the real checker.
check "checker evidence claims (§9a)" \
    cargo test --release --quiet --test phase9a_evidence

echo
echo "PASS: $PASS  FAIL: $FAIL"
[ "$FAIL" -eq 0 ]
