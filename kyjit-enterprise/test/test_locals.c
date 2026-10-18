/*
 * test_locals.c — verify IR_LOAD_LOCAL / IR_STORE_LOCAL and stack frames.
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

/* Build: store CONST to slot, load, return. */
static KyGraph *build_store_load(int64_t value, int32_t offset) {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *b0 = kyir_builder_new_block(b);
    kyir_builder_set_block(b, b0);

    KyNode *c = kyir_const_i64(g, value);
    kyir_block_add_node(b0, c);
    kyir_build_store_local(b, offset, c);

    KyNode *ld = kyir_build_load_local(b, offset);
    kyir_build_return(b, ld);
    return g;
}

/* Build: store two constants, load both, add, return. */
static KyGraph *build_two_slot_add(void) {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *b0 = kyir_builder_new_block(b);
    kyir_builder_set_block(b, b0);

    KyNode *c10 = kyir_const_i64(g, 10);
    kyir_block_add_node(b0, c10);
    kyir_build_store_local(b, -8, c10);

    KyNode *c20 = kyir_const_i64(g, 20);
    kyir_block_add_node(b0, c20);
    kyir_build_store_local(b, -16, c20);

    KyNode *l1 = kyir_build_load_local(b, -8);
    KyNode *l2 = kyir_build_load_local(b, -16);
    KyNode *sum = kyir_add(g, l1, l2);
    kyir_block_add_node(b0, sum);
    kyir_build_return(b, sum);
    return g;
}

/* Build: store 5, overwrite with 7, load, return. */
static KyGraph *build_overwrite(void) {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *b0 = kyir_builder_new_block(b);
    kyir_builder_set_block(b, b0);

    KyNode *c5 = kyir_const_i64(g, 5);
    kyir_block_add_node(b0, c5);
    kyir_build_store_local(b, -8, c5);

    KyNode *c7 = kyir_const_i64(g, 7);
    kyir_block_add_node(b0, c7);
    kyir_build_store_local(b, -8, c7);

    KyNode *ld = kyir_build_load_local(b, -8);
    kyir_build_return(b, ld);
    return g;
}

static int64_t compile_and_run(KyGraph *g) {
    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) return -1;
    size_t len = 0;
    if (kyx64_compile(g, (uint8_t *)code, 4096, &len) != 0) {
        munmap(code, 4096);
        return -1;
    }
    typedef int64_t (*Fn)(void);
    Fn f = (Fn)code;
    int64_t r = f();
    munmap(code, 4096);
    return r;
}

int main(void) {
    printf("KYJIT Locals Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    printf("\n[store 42 to -8, load]\n");
    {
        KyGraph *g = build_store_load(42, -8);
        int64_t r = compile_and_run(g);
        printf("  result = %ld (expect 42)\n", (long)r);
        TEST("store/load 42", r == 42);
        kyir_graph_destroy(g);
    }

    printf("\n[store 10 @-8, 20 @-16, add]\n");
    {
        KyGraph *g = build_two_slot_add();
        int64_t r = compile_and_run(g);
        printf("  result = %ld (expect 30)\n", (long)r);
        TEST("two-slot add = 30", r == 30);
        kyir_graph_destroy(g);
    }

    printf("\n[overwrite slot]\n");
    {
        KyGraph *g = build_overwrite();
        int64_t r = compile_and_run(g);
        printf("  result = %ld (expect 7)\n", (long)r);
        TEST("overwrite yields 7", r == 7);
        kyir_graph_destroy(g);
    }

    printf("\n[frame size]\n");
    {
        /* Not directly inspectable from a mmap compile; just check that
         * the two-slot case still returns 30. Frame layout is validated
         * by the fact that both slots don't collide. */
        KyGraph *g = build_two_slot_add();
        int64_t r = compile_and_run(g);
        TEST("frame layout ok (two slots, no collision)", r == 30);
        kyir_graph_destroy(g);
    }

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
