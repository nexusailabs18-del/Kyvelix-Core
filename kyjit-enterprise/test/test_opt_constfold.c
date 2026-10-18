/*
 * test_opt_constfold.c — verify constant folding covers all ops.
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

/* Build:  return OP(const_a, const_b);   then fold and check result. */
static int fold_binary(KyIROpcode op, int64_t a, int64_t b, int64_t expected) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *bi = kyir_builder_create(g);
    kyir_builder_set_block(bi, b0);

    KyNode *ca = kyir_const_i64(g, a);
    kyir_block_add_node(b0, ca);
    KyNode *cb = kyir_const_i64(g, b);
    kyir_block_add_node(b0, cb);

    KyNode *expr = kyir_node_create(g, op);
    kyir_node_add_value_input(expr, ca);
    kyir_node_add_value_input(expr, cb);
    kyir_node_add_use(ca, expr);
    kyir_node_add_use(cb, expr);
    expr->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, expr);

    kyir_build_return(bi, expr);

    kyir_opt_constfold(g);

    /* Find the result: should be a new const node */
    int found = 0;
    int64_t got = 0;
    for (uint32_t i = 0; i < g->node_count; i++) {
        KyNode *n = g->nodes[i];
        if (!n || (n->flags & KYIR_FLAG_DEAD)) continue;
        if (kyir_node_is_constant(n) && n != ca && n != cb) {
            got = kyir_node_const_i64(n);
            found = 1;
            break;
        }
    }

    int ok = found && (got == expected);
    kyir_builder_destroy(bi);
    kyir_graph_destroy(g);
    return ok;
}

static int fold_unary(KyIROpcode op, int64_t a, int64_t expected) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *bi = kyir_builder_create(g);
    kyir_builder_set_block(bi, b0);

    KyNode *ca = kyir_const_i64(g, a);
    kyir_block_add_node(b0, ca);

    KyNode *expr = kyir_node_create(g, op);
    kyir_node_add_value_input(expr, ca);
    kyir_node_add_use(ca, expr);
    expr->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, expr);

    kyir_build_return(bi, expr);

    kyir_opt_constfold(g);

    int found = 0;
    int64_t got = 0;
    for (uint32_t i = 0; i < g->node_count; i++) {
        KyNode *n = g->nodes[i];
        if (!n || (n->flags & KYIR_FLAG_DEAD)) continue;
        if (kyir_node_is_constant(n) && n != ca) {
            got = kyir_node_const_i64(n);
            found = 1;
            break;
        }
    }

    int ok = found && (got == expected);
    kyir_builder_destroy(bi);
    kyir_graph_destroy(g);
    return ok;
}

int main(void) {
    printf("KYJIT Constfold Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    /* Binary arithmetic */
    printf("\n[arithmetic]\n");
    TEST("5 + 7 = 12",  fold_binary(IR_ADD_I64, 5, 7, 12));
    TEST("10 - 3 = 7",  fold_binary(IR_SUB_I64, 10, 3, 7));
    TEST("6 * 7 = 42",  fold_binary(IR_MUL_I64, 6, 7, 42));
    TEST("100 / 7 = 14", fold_binary(IR_DIV_I64, 100, 7, 14));
    TEST("100 % 7 = 2", fold_binary(IR_MOD_I64, 100, 7, 2));

    /* Division by zero NOT folded */
    printf("\n[div by zero not folded]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *bi = kyir_builder_create(g);
        kyir_builder_set_block(bi, b0);
        KyNode *ca = kyir_const_i64(g, 10);
        kyir_block_add_node(b0, ca);
        KyNode *cb = kyir_const_i64(g, 0);
        kyir_block_add_node(b0, cb);
        KyNode *expr = kyir_node_create(g, IR_DIV_I64);
        kyir_node_add_value_input(expr, ca);
        kyir_node_add_value_input(expr, cb);
        kyir_node_add_use(ca, expr);
        kyir_node_add_use(cb, expr);
        expr->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, expr);
        kyir_build_return(bi, expr);
        int changes = kyir_opt_constfold(g);
        TEST("div by 0 not folded", changes == 0);
        kyir_builder_destroy(bi);
        kyir_graph_destroy(g);
    }

    /* Bitwise */
    printf("\n[bitwise]\n");
    TEST("0xF0 & 0x3C = 0x30", fold_binary(IR_AND, 0xF0, 0x3C, 0x30));
    TEST("0xF0 | 0x0F = 0xFF", fold_binary(IR_OR,  0xF0, 0x0F, 0xFF));
    TEST("0xFF ^ 0x0F = 0xF0", fold_binary(IR_XOR, 0xFF, 0x0F, 0xF0));

    /* Shifts */
    printf("\n[shifts]\n");
    TEST("1 << 4 = 16",  fold_binary(IR_SHL, 1, 4, 16));
    TEST("256 >> 4 = 16", fold_binary(IR_SHR, 256, 4, 16));
    TEST("-16 >> 2 = -4", fold_binary(IR_SAR, -16, 2, -4));

    /* Comparisons */
    printf("\n[comparisons]\n");
    TEST("5 == 5 → 1", fold_binary(IR_CMP_EQ, 5, 5, 1));
    TEST("5 == 6 → 0", fold_binary(IR_CMP_EQ, 5, 6, 0));
    TEST("5 != 6 → 1", fold_binary(IR_CMP_NE, 5, 6, 1));
    TEST("3 < 7 → 1",  fold_binary(IR_CMP_LT, 3, 7, 1));
    TEST("3 <= 3 → 1", fold_binary(IR_CMP_LE, 3, 3, 1));
    TEST("7 > 3 → 1",  fold_binary(IR_CMP_GT, 7, 3, 1));
    TEST("3 >= 3 → 1", fold_binary(IR_CMP_GE, 3, 3, 1));

    /* Unary */
    printf("\n[unary]\n");
    TEST("-(42) = -42", fold_unary(IR_NEG_I64, 42, -42));
    TEST("~0 = -1",     fold_unary(IR_NOT, 0, -1));

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
