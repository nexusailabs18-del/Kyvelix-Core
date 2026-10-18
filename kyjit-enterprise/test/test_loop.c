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
    printf("KYJIT Loop Analysis Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Build a simple loop: b0 -> b1 -> b2 -> b1 (back edge) */
    printf("[Test] simple loop\n");
    KyGraph *g = kyir_graph_create();

    KyBlock *b0 = kyir_block_create(g);  /* preheader */
    KyBlock *b1 = kyir_block_create(g);  /* loop header */
    KyBlock *b2 = kyir_block_create(g);  /* loop body */
    KyBlock *b3 = kyir_block_create(g);  /* exit */

    /* Edges: b0 -> b1, b1 -> b2, b2 -> b1, b2 -> b3 */
    kyir_block_add_successor(b0, b1);
    kyir_block_add_successor(b1, b2);
    kyir_block_add_successor(b2, b1);
    kyir_block_add_successor(b2, b3);

    kyir_block_add_predecessor(b1, b0);
    kyir_block_add_predecessor(b1, b2);
    kyir_block_add_predecessor(b2, b1);
    kyir_block_add_predecessor(b3, b2);

    /* Build dominator tree */
    KyDomTree *dt = kyir_domtree_build(g);
    TEST("domtree non-NULL", dt != NULL);

    /* Build loop info */
    KyLoopInfo *li = kyir_loopinfo_build(g, dt);
    TEST("loopinfo non-NULL", li != NULL);

    if (li) {
        TEST("found 1 loop", li->num_loops == 1);
        if (li->num_loops > 0) {
            KyLoop *loop = li->loops[0];
            TEST("loop header = b1", loop->header == b1);
            TEST("loop latch = b2", loop->latch == b2);
            TEST("loop body has 2 blocks", loop->num_body == 2);
            TEST("loop preheader = b0", loop->preheader == b0);
            TEST("loop has 1 exit", loop->num_exits == 1);
            TEST("loop exit = b3", loop->num_exits > 0 && loop->exits[0] == b3);
            TEST("loop is innermost", loop->is_innermost == 1);
            TEST("loop contains b1", kyir_loop_contains(loop, b1));
            TEST("loop contains b2", kyir_loop_contains(loop, b2));
            TEST("loop doesn't contain b0", !kyir_loop_contains(loop, b0));
            TEST("loop doesn't contain b3", !kyir_loop_contains(loop, b3));
        }
        kyir_loopinfo_print(li);
    }

    /* Test LICM */
    printf("\n[Test] LICM\n");
    /* Add a loop-invariant computation inside the loop */
    KyNode *c1 = kyir_const_i64(g, 10);
    KyNode *c2 = kyir_const_i64(g, 20);
    KyNode *inv = kyir_add(g, c1, c2);
    kyir_block_add_node(b1, c1);
    kyir_block_add_node(b1, c2);
    kyir_block_add_node(b1, inv);

    KyLICMResult licm = kyir_licm(g, li, dt);
    printf("    hoisted %u nodes\n", licm.nodes_hoisted);
    TEST("LICM ran", licm.loops_processed > 0);

    kyir_loopinfo_destroy(li);
    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
