#include "kyjit_ir.h"
#include <stdio.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

int main(void) {
    printf("KYJIT SSA Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Test 1: phi node creation */
    printf("[Test] phi node creation\n");
    KyGraph *g = kyir_graph_create();
    KyNode *phi = kyir_phi_create(g, 2);
    TEST("phi non-NULL", phi != NULL);
    TEST("phi opcode", phi->opcode == IR_PHI);
    TEST("phi has 0 inputs initially", phi->num_values == 0);

    /* Add inputs */
    KyNode *c1 = kyir_const_i64(g, 1);
    KyNode *c2 = kyir_const_i64(g, 2);
    kyir_node_add_value_input(phi, c1);
    kyir_node_add_value_input(phi, c2);
    TEST("phi has 2 inputs now", phi->num_values == 2);
    TEST("phi input[0] = c1", phi->value_inputs[0] == c1);
    TEST("phi input[1] = c2", phi->value_inputs[1] == c2);

    kyir_graph_destroy(g);

    /* Test 2: dominance frontier */
    printf("\n[Test] dominance frontier\n");
    g = kyir_graph_create();
    /* Create a diamond CFG:
     *      b0
     *     /  \
     *    b1   b2
     *     \  /
     *      b3
     */
    KyBlock *b0 = kyir_block_create(g);
    KyBlock *b1 = kyir_block_create(g);
    KyBlock *b2 = kyir_block_create(g);
    KyBlock *b3 = kyir_block_create(g);

    kyir_block_add_successor(b0, b1);
    kyir_block_add_successor(b0, b2);
    kyir_block_add_successor(b1, b3);
    kyir_block_add_successor(b2, b3);
    kyir_block_add_predecessor(b1, b0);
    kyir_block_add_predecessor(b2, b0);
    kyir_block_add_predecessor(b3, b1);
    kyir_block_add_predecessor(b3, b2);

    KyDomTree *dt = kyir_domtree_build(g);
    TEST("domtree non-NULL", dt != NULL);

    KyDominanceFrontier *df = kyir_domfrontier_build(g, dt);
    TEST("df non-NULL", df != NULL);

    if (df) {
        /* b3 should be in DF(b1) and DF(b2) */
        int b3_in_df_b1 = 0, b3_in_df_b2 = 0;
        for (uint32_t i = 0; i < df[b1->id].num_blocks; i++) {
            if (df[b1->id].frontier[i] == b3) b3_in_df_b1 = 1;
        }
        for (uint32_t i = 0; i < df[b2->id].num_blocks; i++) {
            if (df[b2->id].frontier[i] == b3) b3_in_df_b2 = 1;
        }
        TEST("b3 in DF(b1)", b3_in_df_b1);
        TEST("b3 in DF(b2)", b3_in_df_b2);
    }

    kyir_domtree_destroy(dt);

    /* Test 3: SSA context */
    printf("\n[Test] SSA context\n");
    KySSAContext *ssa = kyir_ssa_create(g, dt);
    TEST("ssa non-NULL", ssa != NULL);
    if (ssa) {
        TEST("ssa has graph", ssa->graph == g);
        TEST("ssa has 64 var slots", ssa->num_vars == 64);
        kyir_ssa_print(ssa);
        kyir_ssa_destroy(ssa);
    }

    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
