/*
 * test_opt_gvn.c — verify GVN merges equivalent expressions.
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

static uint32_t count_alive(KyGraph *g) {
    uint32_t n = 0;
    for (uint32_t i = 0; i < g->node_count; i++) {
        if (g->nodes[i] && !(g->nodes[i]->flags & KYIR_FLAG_DEAD)) n++;
    }
    return n;
}

int main(void) {
    printf("KYJIT GVN Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    /* Test 1: two adds with same inputs, same block, should merge. */
    printf("\n[Test 1: identical adds in same block]\n");
    {
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

        KyNode *s1 = kyir_add(g, pa, pb);
        kyir_block_add_node(b0, s1);
        KyNode *s2 = kyir_add(g, pa, pb);
        kyir_block_add_node(b0, s2);

        printf("  before: %u nodes alive\n", count_alive(g));
        int changes = kyir_opt_gvn(g);
        printf("  GVN changes=%d, after: %u nodes alive\n",
               changes, count_alive(g));
        TEST("GVN merged the duplicate", changes >= 1);
        TEST("s1 alive", (s1->flags & KYIR_FLAG_DEAD) == 0);
        TEST("s2 dead", (s2->flags & KYIR_FLAG_DEAD) != 0);

        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    /* Test 2: commutative — add(a,b) and add(b,a) merge. */
    printf("\n[Test 2: commutative add merge]\n");
    {
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

        KyNode *s1 = kyir_add(g, pa, pb);
        kyir_block_add_node(b0, s1);
        KyNode *s2 = kyir_add(g, pb, pa);   /* swapped */
        kyir_block_add_node(b0, s2);

        int changes = kyir_opt_gvn(g);
        printf("  changes=%d\n", changes);
        TEST("commutative merge happened", changes >= 1);
        TEST("s1 alive", (s1->flags & KYIR_FLAG_DEAD) == 0);
        TEST("s2 dead", (s2->flags & KYIR_FLAG_DEAD) != 0);

        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    /* Test 3: different blocks → NOT merged (no dominance info) */
    printf("\n[Test 3: different blocks NOT merged]\n");
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

        int changes = kyir_opt_gvn(g);
        printf("  changes=%d\n", changes);
        TEST("no merge across blocks", changes == 0);
        TEST("s1 alive", (s1->flags & KYIR_FLAG_DEAD) == 0);
        TEST("s2 alive", (s2->flags & KYIR_FLAG_DEAD) == 0);

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
