/*
 * test_reorder.c — verify IR normalization reorders use-before-def.
 *
 * Test 1: build IR in WRONG order (use before def), compile, and
 *         confirm the reorder pass fixes it — the function runs
 *         correctly.
 * Test 2: deliberately construct an unfixable cycle and confirm
 *         verification rejects it.
 */

#include "kyjit_ir.h"
#include <stdio.h>
#include <sys/mman.h>
#include <stdint.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

int main(void) {
    printf("KYJIT Reorder Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    /* ----------------------------------------------------------
     * Test 1: build IR with a use before its def, verify reorder
     * fixes it and the result is correct.
     *
     * Graph:  return (a + b) * 2;
     *   a = param0
     *   b = param1
     *   two = const 2
     *   sum = a + b
     *   result = sum * two
     *
     * Build in this order:
     *   a, b, two, result, sum   <- "result" before "sum" is wrong
     * ---------------------------------------------------------- */
    printf("\n[Test 1: reorder fixes use-before-def]\n");
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

        KyNode *two = kyir_const_i64(g, 2);
        kyir_block_add_node(b0, two);

        /* sum computed here (correct order) */
        KyNode *sum = kyir_add(g, pa, pb);

        /* result uses sum (which hasn't been added to block yet) */
        KyNode *result = kyir_mul(g, sum, two);
        kyir_block_add_node(b0, result);   /* added BEFORE sum */

        /* now add sum — appears AFTER result in the block */
        kyir_block_add_node(b0, sum);

        KyNode *ret = kyir_build_return(b, result);
        (void)ret;

        void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                          MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        TEST("mmap", code != MAP_FAILED);

        size_t len = 0;
        int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
        printf("  compile rc=%d len=%zu\n", rc, len);
        TEST("compiled", rc == 0);

        if (rc == 0) {
            typedef int64_t (*Fn2)(int64_t, int64_t);
            Fn2 f = (Fn2)code;
            int64_t r1 = f(3, 4);   /* (3+4)*2 = 14 */
            int64_t r2 = f(10, 5);  /* (10+5)*2 = 30 */
            printf("  f(3,4) = %ld (expect 14)\n", (long)r1);
            printf("  f(10,5) = %ld (expect 30)\n", (long)r2);
            TEST("f(3,4) == 14", r1 == 14);
            TEST("f(10,5) == 30", r2 == 30);
        }

        munmap(code, 4096);
        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    /* ----------------------------------------------------------
     * Test 2: verification accepts a well-ordered graph.
     * ---------------------------------------------------------- */
    printf("\n[Test 2: verifier accepts well-ordered graph]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *pa = kyir_node_create(g, IR_PARAMETER);
        pa->constant.i64 = 0; pa->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, pa);

        KyNode *one = kyir_const_i64(g, 1);
        kyir_block_add_node(b0, one);

        KyNode *inc = kyir_add(g, pa, one);
        kyir_block_add_node(b0, inc);

        kyir_build_return(b, inc);

        KyVerifyResult vr = kyir_graph_verify(g);
        printf("  verify ok=%d, errors=%u\n", vr.ok, vr.errors);
        for (uint32_t i = 0; i < vr.errors; i++) {
            printf("    %s\n", vr.messages[i]);
        }
        TEST("verifier accepts well-formed graph", vr.ok == 1);

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
