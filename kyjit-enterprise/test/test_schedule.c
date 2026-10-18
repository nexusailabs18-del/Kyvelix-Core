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
    printf("KYJIT Scheduling Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Build a simple graph: constants + adds */
    printf("[Test] basic scheduling\n");
    KyGraph *g = kyir_graph_create();

    KyBlock *b0 = kyir_block_create(g);
    KyNode *c5 = kyir_const_i64(g, 5);
    KyNode *c7 = kyir_const_i64(g, 7);
    KyNode *sum = kyir_add(g, c5, c7);
    KyNode *prod = kyir_mul(g, sum, c5);

    kyir_block_add_node(b0, c5);
    kyir_block_add_node(b0, c7);
    kyir_block_add_node(b0, sum);
    kyir_block_add_node(b0, prod);

    KyDomTree *dt = kyir_domtree_build(g);
    TEST("domtree non-NULL", dt != NULL);

    KySchedule *s = kyir_schedule_build(g, dt);
    TEST("schedule non-NULL", s != NULL);

    if (s) {
        TEST("scheduled nodes > 0", s->num_scheduled > 0);

        /* Apply to graph */
        int rc = kyir_schedule_apply(g, s);
        TEST("apply returns 0", rc == 0);

        /* Verify all nodes have blocks */
        int all_have_blocks = 1;
        for (uint32_t i = 0; i < g->node_count; i++) {
            if (g->nodes[i] && !g->nodes[i]->block) {
                all_have_blocks = 0;
                break;
            }
        }
        TEST("all nodes assigned to blocks", all_have_blocks);

        kyir_schedule_print(s);
        kyir_schedule_destroy(s);
    }

    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    /* Test 2: scheduling with branches */
    printf("\n[Test] scheduling with branches\n");
    g = kyir_graph_create();

    b0 = kyir_block_create(g);
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

    KyNode *x = kyir_const_i64(g, 10);
    KyNode *y = kyir_const_i64(g, 20);
    KyNode *z = kyir_add(g, x, y);
    kyir_block_add_node(b0, x);
    kyir_block_add_node(b0, y);
    kyir_block_add_node(b0, z);

    dt = kyir_domtree_build(g);
    s = kyir_schedule_build(g, dt);
    TEST("schedule with branches non-NULL", s != NULL);

    if (s) {
        TEST("scheduled nodes > 0", s->num_scheduled > 0);
        kyir_schedule_destroy(s);
    }

    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
