/*
 * test_abi.c — SysV x86-64 ABI safety test (no inline asm).
 *
 * Strategy: force the C compiler to keep live values in callee-saved
 * registers across the JIT call by calling it in a loop with other
 * live state. If the JIT clobbers RBX/R12/etc, the compiler's
 * bookkeeping breaks and we get wrong results or a crash.
 *
 * This mirrors what test_backend.c did at -O2 that triggered the
 * original segfault — except now the test explicitly checks for it.
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

static KyGraph *build_add_graph(void) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyNode *arg0 = kyir_node_create(g, IR_PARAMETER);
    arg0->constant.i64 = 0; arg0->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, arg0);
    KyNode *arg1 = kyir_node_create(g, IR_PARAMETER);
    arg1->constant.i64 = 1; arg1->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, arg1);
    KyNode *sum = kyir_add(g, arg0, arg1);
    kyir_block_add_node(b0, sum);
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, sum);
    kyir_node_add_use(sum, ret);
    kyir_block_add_node(b0, ret);
    return g;
}

int main(void) {
    printf("KYJIT ABI Safety Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) { printf("mmap failed\n"); return 1; }

    KyGraph *g = build_add_graph();
    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    printf("  compiled %zu bytes, rc=%d\n", code_len, rc);
    TEST("compiled ok", rc == 0 && code_len > 0);

    typedef int64_t (*AddFn)(int64_t, int64_t);
    AddFn add = (AddFn)code;

    /* Sanity */
    int64_t r1 = add(5, 7);
    printf("  add(5,7) = %ld\n", (long)r1);
    TEST("add(5,7) == 12", r1 == 12);

    /* ============================================================
     * ABI stress: many calls in a loop with live state.
     *
     * GCC at -O2 will keep loop counters and accumulators in
     * callee-saved registers (RBX, R12-R15). If the JIT clobbers
     * them, the loop breaks or we crash.
     * ============================================================ */
    printf("\n[ABI stress: 1M calls with live loop state]\n");
    volatile int64_t sum = 0;
    int i;
    int64_t expected = 0;
    for (i = 0; i < 1000000; i++) {
        int64_t r = add(i & 0xFFFF, 1);
        sum += r;
        expected += (i & 0xFFFF) + 1;
    }
    printf("  sum = %ld (expect %ld)\n", (long)sum, (long)expected);
    TEST("loop sum correct (RBX-class preserved)", sum == expected);
    TEST("loop completed (no crash)", i == 1000000);

    /* ============================================================
     * Nested calls: JIT calls JIT.
     * ============================================================ */
    printf("\n[Nested JIT calls]\n");
    int64_t nested = add(add(5, 7), add(100, 200));
    printf("  add(add(5,7),add(100,200)) = %ld (expect 312)\n",
           (long)nested);
    TEST("nested JIT calls correct", nested == 312);

    kyir_graph_destroy(g);
    munmap(code, 4096);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
