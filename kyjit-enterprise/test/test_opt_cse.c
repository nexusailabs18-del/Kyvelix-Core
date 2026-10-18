/*
 * test_opt_cse.c — verify CSE across blocks.
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

int main(void) {
    printf("KYJIT CSE-across-blocks Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    /* Test 1: equivalent add in dominator block + dominated block,
     * should merge. */
    printf("\n[Test 1: CSE merges across dominating block]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyBlock *b1 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        /* edge_setup: domtree builder needs successors/predecessors to
         * see the b0 → b1 edge, otherwise both blocks are disconnected
         * and dominance queries return false. */
        kyir_block_add_successor(b0, b1);
        kyir_block_add_predecessor(b1, b0);

        KyNode *pa = kyir_node_create(g, IR_PARAMETER);
        pa->constant.i64 = 0; pa->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, pa);
        KyNode *pb = kyir_node_create(g, IR_PARAMETER);
        pb->constant.i64 = 1; pb->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, pb);

        /* s1 = a + b, in b0 */
        KyNode *s1 = kyir_add(g, pa, pb);
        kyir_block_add_node(b0, s1);

        /* s2 = a + b, in b1 */
        KyNode *s2 = kyir_add(g, pa, pb);
        kyir_block_add_node(b1, s2);

        /* Build domtree */
        KyDomTree *dt = kyir_domtree_build(g);
        TEST("domtree built", dt != NULL);

        if (dt) {
            int changes = kyir_opt_cse_across_blocks(g, dt);
            printf("  changes=%d\n", changes);
            TEST("merged across blocks", changes >= 1);
            TEST("s1 alive", (s1->flags & KYIR_FLAG_DEAD) == 0);
            TEST("s2 dead", (s2->flags & KYIR_FLAG_DEAD) != 0);
            kyir_domtree_destroy(dt);
        }

        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    /* Test 2: equivalent add in blocks that DON'T dominate each other.
     * Should NOT merge. */
    printf("\n[Test 2: no merge without dominance]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyBlock *b1 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *pa = kyir_node_create(g, IR_PARAMETER);
        pa->constant.i64 = 0; pa->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, pa);
        KyNode *pb = kyir_node_create(g, IR_PARAMETER);
        pb->constant.i64 = 1; pb->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, pb);

        KyNode *s1 = kyir_add(g, pa, pb);
        kyir_block_add_node(b0, s1);

        kyir_builder_set_block(b, b1);
        KyNode *s2 = kyir_add(g, pa, pb);
        kyir_block_add_node(b1, s2);

        /* b0 and b1 have no idom relationship (no successors/predecessors set) */
        KyDomTree *dt = kyir_domtree_build(g);
        TEST("domtree built", dt != NULL);

        if (dt) {
            int changes = kyir_opt_cse_across_blocks(g, dt);
            printf("  changes=%d\n", changes);
            TEST("no merge without dominance", changes == 0);
            TEST("s1 alive", (s1->flags & KYIR_FLAG_DEAD) == 0);
            TEST("s2 alive", (s2->flags & KYIR_FLAG_DEAD) == 0);
            kyir_domtree_destroy(dt);
        }

        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
