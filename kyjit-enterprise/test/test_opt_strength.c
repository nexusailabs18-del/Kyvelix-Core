/*
 * test_opt_strength.c — verify strength reduction (x * 2^n → x << n).
 */

#include "kyjit_ir.h"
#include <stdio.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

/* Build:  return param0 * const k;  run strength reduce.
 * Return 1 if the multiply node was replaced by SHL. */
static int test_mul_by_const(int64_t k, int swapped) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *b = kyir_builder_create(g);
    kyir_builder_set_block(b, b0);

    KyNode *pa = kyir_node_create(g, IR_PARAMETER);
    pa->constant.i64 = 0; pa->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, pa);

    KyNode *ck = kyir_const_i64(g, k);
    kyir_block_add_node(b0, ck);

    KyNode *mul = kyir_node_create(g, IR_MUL_I64);
    if (swapped) {
        kyir_node_add_value_input(mul, ck);
        kyir_node_add_value_input(mul, pa);
    } else {
        kyir_node_add_value_input(mul, pa);
        kyir_node_add_value_input(mul, ck);
    }
    mul->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, mul);

    kyir_build_return(b, mul);

    int changes = kyir_opt_strength_reduce(g);
    int dead = (mul->flags & KYIR_FLAG_DEAD) != 0;

    kyir_builder_destroy(b);
    kyir_graph_destroy(g);

    return (changes > 0) && dead;
}

/* Build without a constant multiplier — should NOT be replaced. */
static int test_mul_no_reduce(void) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *b = kyir_builder_create(g);
    kyir_builder_set_block(b, b0);

    KyNode *pa = kyir_node_create(g, IR_PARAMETER);
    pa->constant.i64 = 0; pa->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, pa);
    KyNode *pb = kyir_node_create(g, IR_PARAMETER);
    pb->constant.i64 = 1; pb->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, pb);

    KyNode *mul = kyir_node_create(g, IR_MUL_I64);
    kyir_node_add_value_input(mul, pa);
    kyir_node_add_value_input(mul, pb);
    mul->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, mul);

    kyir_build_return(b, mul);

    int changes = kyir_opt_strength_reduce(g);
    int dead = (mul->flags & KYIR_FLAG_DEAD) != 0;

    kyir_builder_destroy(b);
    kyir_graph_destroy(g);

    return (changes == 0) && !dead;
}

int main(void) {
    printf("KYJIT Strength Reduction Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    printf("\n[reduces powers of two]\n");
    TEST("x * 2  → x << 1",   test_mul_by_const(2, 0));
    TEST("x * 4  → x << 2",   test_mul_by_const(4, 0));
    TEST("x * 8  → x << 3",   test_mul_by_const(8, 0));
    TEST("x * 16 → x << 4",   test_mul_by_const(16, 0));
    TEST("x * 256 → x << 8",  test_mul_by_const(256, 0));
    TEST("x * 65536 → x << 16", test_mul_by_const(65536, 0));

    printf("\n[commutative: const * x]\n");
    TEST("2 * x  → x << 1",   test_mul_by_const(2, 1));
    TEST("8 * x  → x << 3",   test_mul_by_const(8, 1));

    printf("\n[does NOT reduce non-powers-of-two]\n");
    TEST("x * 3   not reduced", test_mul_no_reduce() &&
         !test_mul_by_const(3, 0));
    TEST("x * 5   not reduced", !test_mul_by_const(5, 0));
    TEST("x * 6   not reduced", !test_mul_by_const(6, 0));
    TEST("x * 100 not reduced", !test_mul_by_const(100, 0));

    printf("\n[does NOT reduce powers of one]\n");
    /* x * 1 → should be handled by algebraic, not strength reduction.
     * Either way, not reduced by strength_reduce (n_bits == 0 case). */
    TEST("x * 1   not strength-reduced", !test_mul_by_const(1, 0));

    printf("\n[does NOT reduce zero or negative]\n");
    TEST("x * 0   not strength-reduced", !test_mul_by_const(0, 0));
    TEST("x * -2  not strength-reduced", !test_mul_by_const(-2, 0));
    TEST("x * -4  not strength-reduced", !test_mul_by_const(-4, 0));

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
