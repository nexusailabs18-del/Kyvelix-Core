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
    printf("KYJIT Full IR Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Test 1: Blocks + CFG */
    printf("[Test] blocks\n");
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyBlock *b1 = kyir_block_create(g);
    KyBlock *b2 = kyir_block_create(g);
    TEST("block count = 3", g->block_count == 3);
    TEST("b0 non-NULL", b0 != NULL);
    TEST("b1 non-NULL", b1 != NULL);

    kyir_block_add_successor(b0, b1);
    kyir_block_add_successor(b0, b2);
    kyir_block_add_predecessor(b1, b0);
    kyir_block_add_predecessor(b2, b0);
    TEST("b0 has 2 successors", b0->num_successors == 2);
    TEST("b1 has 1 predecessor", b1->num_predecessors == 1);

    /* Test 2: Nodes in blocks */
    printf("\n[Test] nodes in blocks\n");
    KyNode *c1 = kyir_const_i64(g, 1);
    KyNode *c2 = kyir_const_i64(g, 2);
    kyir_block_add_node(b0, c1);
    kyir_block_add_node(b0, c2);
    TEST("b0 has 2 nodes", b0->num_nodes == 2);
    TEST("c1->block == b0", c1->block == b0);

    /* Test 3: Dominator tree */
    printf("\n[Test] dominator tree\n");
    KyDomTree *dt = kyir_domtree_build(g);
    TEST("domtree non-NULL", dt != NULL);
    if (dt) {
        TEST("b0 dominates b1", kyir_domtree_dominates(dt, b0, b1));
        TEST("b0 dominates b2", kyir_domtree_dominates(dt, b0, b2));
        TEST("b1 doesn't dominate b2", !kyir_domtree_dominates(dt, b1, b2));
        TEST("b1 dominates itself", kyir_domtree_dominates(dt, b1, b1));
        KyBlock *lca = kyir_domtree_lca(dt, b1, b2);
        TEST("LCA(b1, b2) = b0", lca == b0);
        kyir_domtree_destroy(dt);
    }

    kyir_graph_destroy(g);

    /* Test 4: Constant folding */
    printf("\n[Test] constant folding\n");
    g = kyir_graph_create();
    KyNode *a = kyir_const_i64(g, 5);
    KyNode *b = kyir_const_i64(g, 7);
    KyNode *add = kyir_add(g, a, b);
    int changes = kyir_opt_constfold(g);
    TEST("constfold returns > 0", changes > 0);
    TEST("add is now dead", (add->flags & KYIR_FLAG_DEAD) != 0);
    kyir_graph_destroy(g);

    /* Test 5: Algebraic simplify */
    printf("\n[Test] algebraic simplify\n");
    g = kyir_graph_create();
    KyNode *x = kyir_const_i64(g, 42);
    KyNode *zero = kyir_const_i64(g, 0);
    KyNode *x_plus_0 = kyir_add(g, x, zero);
    changes = kyir_opt_algebraic(g);
    TEST("algebraic returns > 0", changes > 0);
    TEST("x+0 is dead", (x_plus_0->flags & KYIR_FLAG_DEAD) != 0);
    kyir_graph_destroy(g);

    /* Test 6: DCE */
    printf("\n[Test] dead code elimination\n");
    g = kyir_graph_create();
    KyNode *c3 = kyir_const_i64(g, 3);
    KyNode *c4 = kyir_const_i64(g, 4);
    KyNode *unused = kyir_add(g, c3, c4);
    (void)unused;
    changes = kyir_opt_dce(g);
    TEST("dce returns > 0", changes > 0);
    TEST("unused is dead", (unused->flags & KYIR_FLAG_DEAD) != 0);
    kyir_graph_destroy(g);

    /* Test 7: Full optimize pipeline */
    printf("\n[Test] optimize pipeline\n");
    g = kyir_graph_create();
    KyNode *v1 = kyir_const_i64(g, 10);
    KyNode *v2 = kyir_const_i64(g, 20);
    KyNode *s = kyir_add(g, v1, v2);
    kyir_opt_constfold(g);
    /* s should now be replaced by a constant 30 */
    TEST("s is dead after constfold", (s->flags & KYIR_FLAG_DEAD) != 0);
    kyir_graph_destroy(g);

    /* Test 8: Graph verifier */
    printf("\n[Test] graph verifier\n");
    g = kyir_graph_create();
    KyNode *p = kyir_const_i64(g, 1);
    KyNode *q = kyir_const_i64(g, 2);
    kyir_add(g, p, q);
    KyVerifyResult v = kyir_graph_verify(g);
    TEST("graph verifies OK", v.ok == 1);
    TEST("no errors", v.errors == 0);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
