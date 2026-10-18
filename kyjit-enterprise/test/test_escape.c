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
    printf("KYJIT Escape Analysis + SROA Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Test 1: local object → NO_ESCAPE */
    printf("[Test] local object (no escape)\n");
    KyGraph *g = kyir_graph_create();

    /* Create: alloc = new object; store_field(alloc, 0, 5); return void */
    KyNode *alloc = kyir_node_create(g, IR_NEW_OBJECT);
    KyNode *v5 = kyir_const_i64(g, 5);
    KyNode *store = kyir_node_create(g, IR_STORE_FIELD);
    kyir_node_add_value_input(store, alloc);
    kyir_node_add_value_input(store, v5);
    kyir_node_add_use(alloc, store);

    KyConnectionGraph *cg = kyir_escape_build(g);
    TEST("cg non-NULL", cg != NULL);

    kyir_escape_analyze(cg, g);
    TEST("alloc is NO_ESCAPE",
         kyir_escape_get(cg, alloc) == KYIR_ESCAPE_NO_ESCAPE);

    kyir_escape_print(cg, g);

    kyir_escape_destroy(cg);
    kyir_graph_destroy(g);

    /* Test 2: returned object → RETURN_ESCAPE */
    printf("\n[Test] returned object (escape)\n");
    g = kyir_graph_create();

    alloc = kyir_node_create(g, IR_NEW_OBJECT);
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, alloc);
    kyir_node_add_use(alloc, ret);

    cg = kyir_escape_build(g);
    kyir_escape_analyze(cg, g);
    TEST("alloc is RETURN_ESCAPE",
         kyir_escape_get(cg, alloc) == KYIR_ESCAPE_RETURN_ESCAPE);

    kyir_escape_destroy(cg);
    kyir_graph_destroy(g);

    /* Test 3: SROA on local object */
    printf("\n[Test] SROA on local object\n");
    g = kyir_graph_create();

    alloc = kyir_node_create(g, IR_NEW_OBJECT);
    v5 = kyir_const_i64(g, 5);
    store = kyir_node_create(g, IR_STORE_FIELD);
    kyir_node_add_value_input(store, alloc);
    kyir_node_add_value_input(store, v5);
    kyir_node_add_use(alloc, store);

    cg = kyir_escape_build(g);
    kyir_escape_analyze(cg, g);

    KySROAResult sroa = kyir_sroa(g, cg);
    printf("    eliminated: %u allocations, %u loads, %u stores\n",
           sroa.allocations_eliminated, sroa.loads_replaced, sroa.stores_replaced);
    TEST("alloc eliminated", sroa.allocations_eliminated >= 1);

    kyir_escape_destroy(cg);
    kyir_graph_destroy(g);

    /* Test 4: escape print */
    printf("\n[Test] escape print\n");
    g = kyir_graph_create();
    alloc = kyir_node_create(g, IR_NEW_OBJECT);
    cg = kyir_escape_build(g);
    kyir_escape_analyze(cg, g);
    kyir_escape_print(cg, g);
    TEST("escape print ran", 1);
    kyir_escape_destroy(cg);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
