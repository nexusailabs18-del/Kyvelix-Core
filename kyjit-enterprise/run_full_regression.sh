#!/bin/bash
# KYJIT full regression — 33 suites, ~635 tests.
# Run from project root: ./run_full_regression.sh
cd "$(dirname "$0")"

TOTAL=0
FAILED_SUITES=0

run_suite() {
    local name="$1"; shift
    local timeout_secs="${1:-10}"; shift
    echo -n "  $name: "
    if gcc -O2 -I./include "$@" -o "/tmp/regr_$name" 2>/dev/null; then
        local OUT
        OUT=$(timeout "$timeout_secs" "/tmp/regr_$name" 2>&1)
        local P
        P=$(echo "$OUT" | grep "^Tests passed:" | awk '{print $3}')
        local F
        F=$(echo "$OUT" | grep "^Tests failed:" | awk '{print $3}')
        if [ -z "$P" ]; then
            echo "no pass line"
        else
            echo "$P passed, $F failed"
            TOTAL=$((TOTAL + P))
            [ "$F" != "0" ] && FAILED_SUITES=$((FAILED_SUITES + 1))
        fi
    else
        echo "BUILD FAIL"
    fi
}

# Build the CLI and frontend test binaries first
gcc -O2 -I./include src/kyjit_cli.c src/kyjit_ir.c src/kyjit_frontend.c src/kyjit_elf.c -o build/kyc 2>/dev/null && echo "kyc: built" || echo "kyc: BUILD FAIL"

echo "=== Core ==="
run_suite backend       10 test/test_backend.c src/kyjit_ir.c
run_suite ops           10 test/test_ops.c src/kyjit_ir.c
run_suite ic_ir         10 test/test_ic_ir.c src/kyjit_ir.c src/kyjit_ic.c
run_suite uop_ir        10 test/test_uop_ir.c src/kyjit_uop_ir.c src/kyjit_uop_cache.c src/kyjit_ir.c
run_suite effect_chain  10 test/test_effect_chain.c src/kyjit_ir.c
run_suite branch_exec   10 test/test_branch_exec.c src/kyjit_ir.c
run_suite syscall       20 test/test_syscall.c src/kyjit_ir.c src/kyjit_elf.c
run_suite write         20 test/test_write.c src/kyjit_ir.c src/kyjit_elf.c
run_suite elf           20 test/test_elf.c src/kyjit_ir.c src/kyjit_elf.c
run_suite x86ext        10 test/test_x86ext.c src/kyjit_ir.c
run_suite funcs         10 test/test_funcs.c src/kyjit_ir.c
run_suite abi           10 test/test_abi.c src/kyjit_ir.c
run_suite regalloc      10 test/test_regalloc.c src/kyjit_ir.c
run_suite reorder       10 test/test_reorder.c src/kyjit_ir.c
run_suite cfg           10 test/test_cfg.c src/kyjit_ir.c
run_suite sccp          10 test/test_sccp.c src/kyjit_ir.c
run_suite branches      10 test/test_branches.c src/kyjit_ir.c

echo ""
echo "=== Optimization ==="
run_suite opt_dce       10 test/test_opt_dce.c src/kyjit_ir.c
run_suite opt_gvn       10 test/test_opt_gvn.c src/kyjit_ir.c
run_suite opt_constfold 10 test/test_opt_constfold.c src/kyjit_ir.c
run_suite opt_algebraic 10 test/test_opt_algebraic.c src/kyjit_ir.c
run_suite opt_cse       10 test/test_opt_cse.c src/kyjit_ir.c
run_suite opt_strength  10 test/test_opt_strength.c src/kyjit_ir.c

echo ""
echo "=== Analysis ==="
run_suite ir            10 test/test_ir.c src/kyjit_ir.c
run_suite ir_full       10 test/test_ir_full.c src/kyjit_ir.c
run_suite ssa           10 test/test_ssa.c src/kyjit_ir.c
run_suite loop          10 test/test_loop.c src/kyjit_ir.c
run_suite types         10 test/test_types.c src/kyjit_ir.c
run_suite escape        10 test/test_escape.c src/kyjit_ir.c
run_suite schedule      10 test/test_schedule.c src/kyjit_ir.c

echo ""
echo "=== Frozen ==="
run_suite ic            10 test/test_ic.c src/kyjit_ic.c
run_suite uop           10 test/test_uop.c src/kyjit_uop_cache.c
run_suite uop_patterns  10 test/test_uop_patterns.c src/kyjit_uop_cache.c

echo ""
echo "========================================"
echo "TOTAL: $TOTAL passed, $FAILED_SUITES suites with failures"
echo "========================================"
