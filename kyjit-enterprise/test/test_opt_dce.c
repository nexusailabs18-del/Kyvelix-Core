/*
 * test_opt_dce.c — verify DCE removes dead nodes.
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

/* Count non-dead nodes */
static uint32_t count_alive(KyGraph *g) {
    uint32_t n = 0;
    for (uint32_t i = 0; i < g->node_count; i++) {
        if (g->nodes[i] && !(g->nodes[i]->flags & KYIR_FLAG_DEAD)) n++;
    }
    return n;
}

int main(void) {
    printf("KYJIT DCE Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    /* Test 1: two dead constants, one used by return.
     *
     *   a = const 5       <- dead
     *   b = const 7       <- dead
     *   c = const 42      <- used by return
     *   ret c
     */
    printf("\n[Test 1: two unused constants removed]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *a = kyir_const_i64(g, 5);
        kyir_block_add_node(b0, a);
        KyNode *bv = kyir_const_i64(g, 7);
        kyir_block_add_node(b0, bv);
        KyNode *c = kyir_const_i64(g, 42);
        kyir_block_add_node(b0, c);

        kyir_build_return(b, c);

        uint32_t before = count_alive(g);
        int changes = kyir_opt_dce(g);
        uint32_t after = count_alive(g);
        printf("  nodes before=%u, after=%u, changes=%d\n",
               before, after, changes);
        TEST("two nodes removed", before - after == 2);
        TEST("a is dead", (a->flags & KYIR_FLAG_DEAD) != 0);
        TEST("b is dead", (bv->flags & KYIR_FLAG_DEAD) != 0);
        TEST("c is alive", (c->flags & KYIR_FLAG_DEAD) == 0);

        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    /* Test 2: cascading DCE.
     *
     *   a = const 5
     *   b = const 7
     *   s = a + b         <- becomes dead when s's use is removed
     *   two = const 2     <- alive (used by return)
     *   ret two
     *
     * Removing s makes a, b unused, which then also die.
     */
    printf("\n[Test 2: cascading removal]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *a = kyir_const_i64(g, 5);
        kyir_block_add_node(b0, a);
        KyNode *bv = kyir_const_i64(g, 7);
        kyir_block_add_node(b0, bv);
        KyNode *s = kyir_add(g, a, bv);   /* used nowhere */
        kyir_block_add_node(b0, s);
        KyNode *two = kyir_const_i64(g, 2);
        kyir_block_add_node(b0, two);

        kyir_build_return(b, two);

        uint32_t before = count_alive(g);
        int changes = kyir_opt_dce(g);
        uint32_t after = count_alive(g);
        printf("  nodes before=%u, after=%u, changes=%d\n",
               before, after, changes);
        /* a, b, s removed = 3 nodes; two, start, end, memory_start, memory_end, return are alive */
        TEST("three nodes removed (cascading)", before - after == 3);
        TEST("a is dead", (a->flags & KYIR_FLAG_DEAD) != 0);
        TEST("b is dead", (bv->flags & KYIR_FLAG_DEAD) != 0);
        TEST("s is dead", (s->flags & KYIR_FLAG_DEAD) != 0);
        TEST("two is alive", (two->flags & KYIR_FLAG_DEAD) == 0);

        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    /* Test 3: side-effecting nodes NOT removed even if unused.
     *
     *   store(p, 0, 42)   <- unused result but has side effects
     *   ret const 0
     */
    printf("\n[Test 3: side-effecting node stays]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *p = kyir_node_create(g, IR_PARAMETER);
        p->constant.i64 = 0; p->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, p);

        KyNode *v = kyir_const_i64(g, 42);
        kyir_block_add_node(b0, v);

        KyNode *st = kyir_build_store_field(b, p, 0, v);
        TEST("store node created", st != NULL);

        KyNode *zero = kyir_const_i64(g, 0);
        kyir_block_add_node(b0, zero);

        kyir_build_return(b, zero);

        kyir_opt_dce(g);
        TEST("store survived DCE", st && (st->flags & KYIR_FLAG_DEAD) == 0);

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
