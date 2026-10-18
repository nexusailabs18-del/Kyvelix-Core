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
    printf("KYJIT IR Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Test 1: graph creation */
    printf("[Test] graph_create\n");
    KyGraph *g = kyir_graph_create();
    TEST("graph non-NULL", g != NULL);
    TEST("start node exists", g->start != NULL);
    TEST("end node exists", g->end != NULL);
    TEST("node count = 2", g->node_count == 2);
    TEST("start opcode", g->start->opcode == IR_START);
    TEST("end opcode", g->end->opcode == IR_END);

    /* Test 2: constant creation */
    printf("\n[Test] constants\n");
    KyNode *c5 = kyir_const_i64(g, 5);
    KyNode *c7 = kyir_const_i64(g, 7);
    TEST("c5 non-NULL", c5 != NULL);
    TEST("c5 value = 5", c5->constant.i64 == 5);
    TEST("c7 value = 7", c7->constant.i64 == 7);
    TEST("c5 is constant", (c5->flags & KYIR_FLAG_CONSTANT) != 0);
    TEST("c5 type INT64", c5->type == KYIR_TYPE_INT64);
    TEST("node count = 4", g->node_count == 4);

    /* Test 3: add node */
    printf("\n[Test] add node\n");
    KyNode *add = kyir_add(g, c5, c7);
    TEST("add non-NULL", add != NULL);
    TEST("add opcode", add->opcode == IR_ADD_I64);
    TEST("add has 2 value inputs", add->num_values == 2);
    TEST("add input[0] = c5", add->value_inputs[0] == c5);
    TEST("add input[1] = c7", add->value_inputs[1] == c7);
    TEST("c5 has 1 use", c5->num_uses == 1);
    TEST("c7 has 1 use", c7->num_uses == 1);
    TEST("c5 use is add", c5->uses[0] == add);

    /* Test 4: chaining */
    printf("\n[Test] chained computation\n");
    KyNode *mul = kyir_mul(g, add, c7);
    TEST("mul non-NULL", mul != NULL);
    TEST("mul has 2 inputs", mul->num_values == 2);
    TEST("mul input[0] = add", mul->value_inputs[0] == add);
    TEST("add has 1 use now", add->num_uses == 1);

    KyNode *sub = kyir_sub(g, mul, c5);
    TEST("sub non-NULL", sub != NULL);
    TEST("node count = 7", g->node_count == 7);

    /* Test 5: use counting */
    printf("\n[Test] use tracking\n");
    KyNode *c1 = kyir_const_i64(g, 1);
    KyNode *a1 = kyir_add(g, c1, c5);
    KyNode *a2 = kyir_add(g, c1, c7);
    (void)a1; (void)a2;
    TEST("c1 has 2 uses", c1->num_uses == 2);
    TEST("c5 has 3 uses", c5->num_uses == 3);

    /* Test 6: graph print */
    printf("\n[Test] graph print\n");
    kyir_graph_print(g);
    TEST("graph has 10 nodes", g->node_count == 10);

    /* Cleanup */
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
