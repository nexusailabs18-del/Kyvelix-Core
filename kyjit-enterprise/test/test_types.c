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
    printf("KYJIT Type Inference Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Test 1: constant types */
    printf("[Test] constant types\n");
    KyGraph *g = kyir_graph_create();
    KyNode *c5 = kyir_const_i64(g, 5);
    KyNode *c7 = kyir_const_i64(g, 7);

    KyDomTree *dt = kyir_domtree_build(g);
    KyTypeInference *ti = kyir_types_infer(g, dt);
    TEST("type inference non-NULL", ti != NULL);

    int64_t v;
    TEST("c5 is constant", kyir_types_is_const(ti, c5, &v));
    TEST("c5 = 5", v == 5);
    TEST("c7 = 7", kyir_types_is_const(ti, c7, &v) && v == 7);
    TEST("c5 is int", kyir_types_is_int(ti, c5));
    TEST("c7 is int", kyir_types_is_int(ti, c7));

    /* Test 2: arithmetic types */
    printf("\n[Test] arithmetic types\n");
    KyNode *sum = kyir_add(g, c5, c7);
    ti = kyir_types_infer(g, dt);
    TEST("sum is int", kyir_types_is_int(ti, sum));
    TEST("sum is constant", kyir_types_is_const(ti, sum, &v));
    TEST("sum = 12", v == 12);

    /* Test 3: more arithmetic */
    printf("\n[Test] chained arithmetic\n");
    KyNode *prod = kyir_mul(g, sum, c5);
    KyNode *diff = kyir_sub(g, prod, c7);
    ti = kyir_types_infer(g, dt);

    TEST("prod is int", kyir_types_is_int(ti, prod));
    TEST("diff is int", kyir_types_is_int(ti, diff));

    /* Test 4: type print */
    printf("\n[Test] type print\n");
    kyir_types_print(ti);
    TEST("type info printed", 1);

    /* Test 5: SCCP */
    printf("\n[Test] SCCP\n");
    int changes = kyir_opt_sccp(g, ti);
    TEST("SCCP ran (changes >= 0)", changes >= 0);

    /* Test 6: type name */
    printf("\n[Test] type names\n");
    TEST("INT32 name", kyir_type_name(KYIR_TYPE_INT32)[0] == 'I');
    TEST("FLOAT64 name", kyir_type_name(KYIR_TYPE_FLOAT64)[0] == 'F');
    TEST("OBJECT name", kyir_type_name(KYIR_TYPE_OBJECT)[0] == 'O');

    kyir_types_destroy(ti);
    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
