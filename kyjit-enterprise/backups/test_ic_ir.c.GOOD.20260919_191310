/*
 * test_ic_ir.c — IC → IR → x86-64 integration test.
 *
 * Demonstrates the complete property-access compilation path:
 *   1. Shape check via KyIC gives us a validated field offset
 *   2. IR graph: load_field(param0, offset)
 *   3. Backend compiles: mov rax, [rdi + offset]
 *   4. Execute against a real struct, verify value
 */

#include "kyjit_ir.h"
#include "kyjit_ic.h"
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <stdint.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

/* A two-field struct with known layout: */
struct Point {
    int64_t x;   /* offset 0  */
    int64_t y;   /* offset 8  */
};

/* ============================================================
 * Test 1: raw load_field — build IR by hand, compile, execute.
 * ============================================================ */
static int test_raw_load_field(void) {
    printf("\n[Test 1] raw load_field(param0, offsetof(y))\n");

    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    /* param0: the object pointer in rdi */
    KyNode *obj = kyir_node_create(g, IR_PARAMETER);
    obj->constant.i64 = 0;
    obj->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, obj);

    /* load_field(obj, offsetof(y)=8) */
    KyNode *ld = kyir_node_create(g, IR_LOAD_FIELD);
    kyir_node_add_value_input(ld, obj);
    kyir_node_add_use(obj, ld);
    ld->constant.i64 = 8;
    ld->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, ld);

    /* return ld */
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, ld);
    kyir_node_add_use(ld, ret);
    kyir_block_add_node(b0, ret);

    /* Compile */
    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    printf("  compiled %zu bytes, rc=%d\n", code_len, rc);
    TEST("compiled ok", rc == 0 && code_len > 0);

    /* Execute */
    typedef int64_t (*FieldFn)(void *);
    FieldFn f = (FieldFn)code;

    struct Point p = { .x = 10, .y = 20 };
    int64_t got = f(&p);
    printf("  f(&{x=10,y=20}) = %ld (expected y=20)\n", (long)got);
    TEST("loaded y == 20", got == 20);

    struct Point q = { .x = 111, .y = 222 };
    int64_t got2 = f(&q);
    printf("  f(&{x=111,y=222}) = %ld (expected 222)\n", (long)got2);
    TEST("loaded y == 222", got2 == 222);

    munmap(code, 4096);
    kyir_graph_destroy(g);
    return 0;
}

/* ============================================================
 * Test 2: builder API — kyir_build_load_field convenience.
 * ============================================================ */
static int test_builder_api(void) {
    printf("\n[Test 2] kyir_build_load_field via KyIRBuilder\n");

    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *builder = kyir_builder_create(g);
    kyir_builder_set_block(builder, b0);

    KyNode *obj = kyir_node_create(g, IR_PARAMETER);
    obj->constant.i64 = 0;
    obj->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, obj);

    KyNode *ld = kyir_build_load_field(builder, obj, 0);  /* x */
    TEST("builder returned node", ld != NULL);

    KyNode *ret = kyir_build_return(builder, ld);
    TEST("return node created", ret != NULL);

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    TEST("compiled ok", rc == 0);

    typedef int64_t (*FieldFn)(void *);
    FieldFn f = (FieldFn)code;

    struct Point p = { .x = 1234, .y = 9999 };
    int64_t got = f(&p);
    printf("  f(&{x=1234,y=9999}) = %ld (expected x=1234)\n", (long)got);
    TEST("loaded x == 1234", got == 1234);

    munmap(code, 4096);
    kyir_builder_destroy(builder);
    kyir_graph_destroy(g);
    return 0;
}

/* ============================================================
 * Test 3: real IC → IR integration.
 *
 * Train an inline cache with a shape→offset mapping, read the
 * offset back out, and use it as the load_field offset. This
 * demonstrates what a real JIT does after an IC hit.
 * ============================================================ */
static int test_ic_integration(void) {
    printf("\n[Test 3] real KyIC → IR offset\n");

    /* Train an IC: shape 42 maps to offset of 'y' (8). */
    KyIC ic;
    ky_ic_init(&ic);
    ky_ic_update_inline(&ic, 42, 8);

    int32_t ic_offset = ky_ic_lookup_inline(&ic, 42);
    printf("  IC lookup(shape=42) -> offset %d\n", ic_offset);
    TEST("IC returned offset 8", ic_offset == 8);

    /* Now compile IR using that offset — this is the runtime path
     * a JIT would take after seeing a matching shape. */
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    KyNode *obj = kyir_node_create(g, IR_PARAMETER);
    obj->constant.i64 = 0;
    obj->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, obj);

    KyNode *ld = kyir_node_create(g, IR_LOAD_FIELD);
    kyir_node_add_value_input(ld, obj);
    kyir_node_add_use(obj, ld);
    ld->constant.i64 = ic_offset;   /* <-- from the IC */
    ld->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, ld);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, ld);
    kyir_node_add_use(ld, ret);
    kyir_block_add_node(b0, ret);

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    TEST("compiled from IC offset", rc == 0);

    typedef int64_t (*FieldFn)(void *);
    FieldFn f = (FieldFn)code;

    struct Point p = { .x = 7, .y = 42 };
    int64_t got = f(&p);
    printf("  f(&{x=7,y=42}) = %ld (expected 42)\n", (long)got);
    TEST("IC-derived offset loads correct value", got == 42);

    munmap(code, 4096);
    kyir_graph_destroy(g);
    return 0;
}

/* ============================================================
 * Test 4: store_field then load_field roundtrip.
 * ============================================================ */
static int test_store_then_load(void) {
    printf("\n[Test 4] store_field then load_field roundtrip\n");

    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    /* param0 = object pointer */
    KyNode *obj = kyir_node_create(g, IR_PARAMETER);
    obj->constant.i64 = 0;
    obj->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, obj);

    /* const 7777 */
    KyNode *val = kyir_const_i64(g, 7777);
    kyir_block_add_node(b0, val);

    /* store_field(obj, 8, 7777) */
    KyNode *st = kyir_node_create(g, IR_STORE_FIELD);
    kyir_node_add_value_input(st, obj);
    kyir_node_add_value_input(st, val);
    kyir_node_add_use(obj, st);
    kyir_node_add_use(val, st);
    st->constant.i64 = 8;
    kyir_block_add_node(b0, st);

    /* load_field(obj, 8) */
    KyNode *ld = kyir_node_create(g, IR_LOAD_FIELD);
    kyir_node_add_value_input(ld, obj);
    kyir_node_add_use(obj, ld);
    ld->constant.i64 = 8;
    ld->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, ld);

    /* return ld */
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, ld);
    kyir_node_add_use(ld, ret);
    kyir_block_add_node(b0, ret);

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    printf("  compiled %zu bytes, rc=%d\n", code_len, rc);
    TEST("compiled ok", rc == 0);

    typedef int64_t (*StoreLoadFn)(void *);
    StoreLoadFn f = (StoreLoadFn)code;

    struct Point p = { .x = 0, .y = 0 };
    int64_t got = f(&p);
    printf("  after f(&p): p.y = %ld (via return %ld), expected 7777\n",
           (long)p.y, (long)got);
    TEST("loaded value is 7777", got == 7777);
    TEST("memory actually modified (p.y == 7777)", p.y == 7777);

    munmap(code, 4096);
    kyir_graph_destroy(g);
    return 0;
}

int main(void) {
    printf("KYJIT IC→IR Integration Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    test_raw_load_field();
    test_builder_api();
    test_ic_integration();
    test_store_then_load();

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
