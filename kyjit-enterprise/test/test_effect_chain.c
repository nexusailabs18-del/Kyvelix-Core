/*
 * test_effect_chain.c — Phase 1, Session 2
 *
 * Verifies load-after-store elimination works:
 *   store_field(p, off, value);
 *   x = load_field(p, off);
 * should compile to NO load instruction — the stored value is used
 * directly.
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

/* Build IR: store_field(param0, 0, 42); return load_field(param0, 0); */
static KyGraph *build_store_then_load(void) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *b = kyir_builder_create(g);
    kyir_builder_set_block(b, b0);

    KyNode *p = kyir_node_create(g, IR_PARAMETER);
    p->constant.i64 = 0;
    p->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, p);

    KyNode *val = kyir_const_i64(g, 42);
    kyir_block_add_node(b0, val);

    KyNode *st = kyir_build_store_field(b, p, 0, val);
    (void)st;
    KyNode *ld = kyir_build_load_field(b, p, 0);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, ld);
    kyir_node_add_use(ld, ret);
    kyir_block_add_node(b0, ret);

    kyir_builder_destroy(b);
    return g;
}

/* Build IR: store_field(param0, 0, 42); store_field(param0, 8, 99);
 *           return load_field(param0, 8);  -- should use 99 */
static KyGraph *build_double_store_load(void) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *b = kyir_builder_create(g);
    kyir_builder_set_block(b, b0);

    KyNode *p = kyir_node_create(g, IR_PARAMETER);
    p->constant.i64 = 0;
    p->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, p);

    KyNode *v1 = kyir_const_i64(g, 42);
    kyir_block_add_node(b0, v1);
    KyNode *v2 = kyir_const_i64(g, 99);
    kyir_block_add_node(b0, v2);

    kyir_build_store_field(b, p, 0, v1);
    kyir_build_store_field(b, p, 8, v2);
    KyNode *ld = kyir_build_load_field(b, p, 8);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, ld);
    kyir_node_add_use(ld, ret);
    kyir_block_add_node(b0, ret);

    kyir_builder_destroy(b);
    return g;
}

/* Build IR: store_field(param0, 0, 42); load_field(param0, 8);
 *           -- different offset, must NOT be eliminated. */
static KyGraph *build_different_offset(void) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);
    KyIRBuilder *b = kyir_builder_create(g);
    kyir_builder_set_block(b, b0);

    KyNode *p = kyir_node_create(g, IR_PARAMETER);
    p->constant.i64 = 0;
    p->type = KYIR_TYPE_INT64;
    kyir_block_add_node(b0, p);

    KyNode *v1 = kyir_const_i64(g, 42);
    kyir_block_add_node(b0, v1);

    kyir_build_store_field(b, p, 0, v1);
    KyNode *ld = kyir_build_load_field(b, p, 8);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, ld);
    kyir_node_add_use(ld, ret);
    kyir_block_add_node(b0, ret);

    kyir_builder_destroy(b);
    return g;
}

/* Run a compiled graph with one pointer argument */
static int64_t run1(KyGraph *g, void *arg, uint8_t *bytes_out, size_t *len_out) {
    kyir_debug_reset_counters();

    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) return -999999;

    size_t len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
    if (rc != 0) { munmap(code, 4096); return -999999; }

    if (bytes_out && len_out) {
        size_t n = (*len_out < 64) ? *len_out : 64;
        __builtin_memcpy(bytes_out, code, n);
        *len_out = len;
    }

    typedef int64_t (*Fn1)(void *);
    Fn1 f = (Fn1)code;
    int64_t r = f(arg);
    munmap(code, 4096);
    return r;
}

int main(void) {
    printf("KYJIT Effect Chain Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    /* Test 1: store then load to same address */
    printf("\n[Test 1: store_field(p, 0, 42); return load_field(p, 0);]\n");
    {
        int64_t buf[4] = {0, 0, 0, 0};
        KyGraph *g = build_store_then_load();
        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  result = %ld (expect 42)\n", (long)r);
        printf("  mem[0] = %ld (expect 42, store should have run)\n", (long)buf[0]);
        printf("  bytes (%zu): ", len);
        for (size_t i = 0; i < len && i < 40; i++) printf("%02x ", bytes[i]);
        printf("\n");

        TEST("compiled and ran", r != -999999);
        TEST("result == 42", r == 42);
        TEST("store wrote memory", buf[0] == 42);
        TEST("load was eliminated (counter)", kyir_debug_loads_eliminated() == 1);

        /* Now verify NO load instruction: look for the pattern of a load
         * mov reg, [reg + 0] = REX.W 8B <modrm>. We don't search for it
         * specifically (hard to distinguish from other movs), but if the
         * result is 42 and the store wrote memory, then even if a load
         * happened, it's correct. Real proof of elimination needs a
         * different mechanism — that comes in Session 3's byte-level
         * verifier.
         * For Session 2, we verify SEMANTIC correctness. */

        kyir_graph_destroy(g);
    }

    /* Test 2: two stores, load from second */
    printf("\n[Test 2: store(0,42); store(8,99); return load(8);]\n");
    {
        int64_t buf[4] = {0, 0, 0, 0};
        KyGraph *g = build_double_store_load();
        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  result = %ld (expect 99)\n", (long)r);
        printf("  mem[0] = %ld, mem[1] = %ld (expect 42, 99)\n",
               (long)buf[0], (long)buf[1]);

        TEST("result == 99", r == 99);
        TEST("both stores wrote memory", buf[0] == 42 && buf[1] == 99);

        kyir_graph_destroy(g);
    }

    /* Test 3: store at offset 0, load at offset 8 — MUST NOT eliminate */
    printf("\n[Test 3: store(0,42); return load(8); — different offset]\n");
    {
        int64_t buf[4] = {0xDEAD, 0xBEEF, 0, 0};
        KyGraph *g = build_different_offset();
        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  result = 0x%lx (expect 0xBEEF)\n", (unsigned long)r);
        TEST("result == 0xBEEF", r == 0xBEEF);
        kyir_graph_destroy(g);
    }

    /* Test 4: store same address twice — first store should be dead */
    printf("\n[Test 4: store(0,42); store(0,99); return load(0);]\n");
    {
        int64_t buf[4] = {0, 0, 0, 0};
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *p = kyir_node_create(g, IR_PARAMETER);
        p->constant.i64 = 0;
        p->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, p);

        KyNode *v1 = kyir_const_i64(g, 42);
        kyir_block_add_node(b0, v1);
        KyNode *v2 = kyir_const_i64(g, 99);
        kyir_block_add_node(b0, v2);

        kyir_build_store_field(b, p, 0, v1);
        kyir_build_store_field(b, p, 0, v2);
        KyNode *ld = kyir_build_load_field(b, p, 0);

        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, ld);
        kyir_node_add_use(ld, ret);
        kyir_block_add_node(b0, ret);

        kyir_builder_destroy(b);

        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  result = %ld (expect 99)\n", (long)r);
        printf("  mem[0] = %ld (expect 99, second store won)\n", (long)buf[0]);
        printf("  bytes (%zu): ", len);
        for (size_t i = 0; i < len && i < 40; i++) printf("%02x ", bytes[i]);
        printf("\n");

        /* Count store instructions: `mov [reg], reg` = 48 89 <modrm>.
         * Look for pattern `48 89` followed by modrm with mod=10 (disp32).
         * Actually store is `REX.W 89 <modrm>` where modrm reg field is source. */
        int store_count = 0;
        for (size_t i = 0; i + 2 < len; i++) {
            if (bytes[i] == 0x48 && bytes[i+1] == 0x89) {
                uint8_t modrm = bytes[i+2];
                if ((modrm & 0xC0) == 0x80) store_count++;   /* mod=10 */
            }
        }
        printf("  store instructions found: %d (expect 1)\n", store_count);

        TEST("result == 99", r == 99);
        TEST("only one store emitted", store_count == 1);
        TEST("load eliminated", kyir_debug_loads_eliminated() == 1);

        kyir_graph_destroy(g);
    }

    /* Test 5: load between stores — both must survive */
    printf("\n[Test 5: store(0,1); load(0); store(0,2); load(0);]\n");
    {
        int64_t buf[4] = {0, 0, 0, 0};
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *p = kyir_node_create(g, IR_PARAMETER);
        p->constant.i64 = 0;
        p->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, p);

        KyNode *v1 = kyir_const_i64(g, 1);
        kyir_block_add_node(b0, v1);
        KyNode *v2 = kyir_const_i64(g, 2);
        kyir_block_add_node(b0, v2);

        kyir_build_store_field(b, p, 0, v1);
        KyNode *ld1 = kyir_build_load_field(b, p, 0);
        kyir_build_store_field(b, p, 0, v2);
        KyNode *ld2 = kyir_build_load_field(b, p, 0);

        /* return ld1 + ld2*100 — encodes both loads into the result */
        KyNode *c100 = kyir_const_i64(g, 100);
        kyir_block_add_node(b0, c100);
        KyNode *scaled = kyir_mul(g, ld2, c100);
        kyir_block_add_node(b0, scaled);
        KyNode *sum = kyir_add(g, ld1, scaled);
        kyir_block_add_node(b0, sum);

        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, sum);
        kyir_node_add_use(sum, ret);
        kyir_block_add_node(b0, ret);

        kyir_builder_destroy(b);

        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  result = %ld (expect 1 + 2*100 = 201)\n", (long)r);
        printf("  mem[0] = %ld (expect 2, last store)\n", (long)buf[0]);

        TEST("result == 201", r == 201);

        kyir_graph_destroy(g);
    }

    /* Test 6: two loads same address, no intervening store -> GVN */
    printf("\n[Test 6: load(0); load(0) — GVN should eliminate second]\n");
    {
        int64_t buf[4] = {777, 0, 0, 0};
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *p = kyir_node_create(g, IR_PARAMETER);
        p->constant.i64 = 0;
        p->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, p);

        KyNode *ld1 = kyir_build_load_field(b, p, 0);
        KyNode *ld2 = kyir_build_load_field(b, p, 0);

        KyNode *c10 = kyir_const_i64(g, 10);
        kyir_block_add_node(b0, c10);
        KyNode *scaled = kyir_mul(g, ld2, c10);
        kyir_block_add_node(b0, scaled);
        KyNode *sum = kyir_add(g, ld1, scaled);
        kyir_block_add_node(b0, sum);

        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, sum);
        kyir_node_add_use(sum, ret);
        kyir_block_add_node(b0, ret);

        kyir_builder_destroy(b);

        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  buf[0] = 777, expect 777 + 777*10 = 8547\n");
        printf("  result = %ld\n", (long)r);
        printf("  loads eliminated: %u (expect 1)\n",
               kyir_debug_loads_eliminated());

        TEST("result == 8547", r == 8547);
        TEST("one load eliminated (GVN)", kyir_debug_loads_eliminated() == 1);

        kyir_graph_destroy(g);
    }

    /* Test 7: load, store to different address, load same address.
     * GVN should still fire — the store is to a different address. */
    printf("\n[Test 7: load(0); store(8); load(0) — GVN still fires]\n");
    {
        int64_t buf[4] = {111, 0, 0, 0};
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *p = kyir_node_create(g, IR_PARAMETER);
        p->constant.i64 = 0;
        p->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, p);

        KyNode *ld1 = kyir_build_load_field(b, p, 0);
        KyNode *v = kyir_const_i64(g, 999);
        kyir_block_add_node(b0, v);
        kyir_build_store_field(b, p, 8, v);
        KyNode *ld2 = kyir_build_load_field(b, p, 0);

        KyNode *c10 = kyir_const_i64(g, 10);
        kyir_block_add_node(b0, c10);
        KyNode *scaled = kyir_mul(g, ld2, c10);
        kyir_block_add_node(b0, scaled);
        KyNode *sum = kyir_add(g, ld1, scaled);
        kyir_block_add_node(b0, sum);

        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, sum);
        kyir_node_add_use(sum, ret);
        kyir_block_add_node(b0, ret);

        kyir_builder_destroy(b);

        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        printf("  buf[0] = 111, expect 111 + 111*10 = 1221\n");
        printf("  result = %ld\n", (long)r);
        printf("  loads eliminated: %u (expect 1, GVN still fires)\n",
               kyir_debug_loads_eliminated());

        TEST("result == 1221", r == 1221);
        TEST("one load eliminated (GVN across different-addr store)",
             kyir_debug_loads_eliminated() == 1);

        kyir_graph_destroy(g);
    }

    /* Test 8: load, store to SAME address, load — GVN must NOT fire */
    printf("\n[Test 8: load(0); store(0, 555); load(0) — GVN must NOT fire]\n");
    {
        int64_t buf[4] = {111, 0, 0, 0};
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *p = kyir_node_create(g, IR_PARAMETER);
        p->constant.i64 = 0;
        p->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, p);

        KyNode *ld1 = kyir_build_load_field(b, p, 0);
        KyNode *v = kyir_const_i64(g, 555);
        kyir_block_add_node(b0, v);
        kyir_build_store_field(b, p, 0, v);
        KyNode *ld2 = kyir_build_load_field(b, p, 0);

        KyNode *c1000 = kyir_const_i64(g, 1000);
        kyir_block_add_node(b0, c1000);
        KyNode *scaled = kyir_mul(g, ld2, c1000);
        kyir_block_add_node(b0, scaled);
        KyNode *sum = kyir_add(g, ld1, scaled);
        kyir_block_add_node(b0, sum);

        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, sum);
        kyir_node_add_use(sum, ret);
        kyir_block_add_node(b0, ret);

        kyir_builder_destroy(b);

        uint8_t bytes[64]; size_t len = 64;
        int64_t r = run1(g, buf, bytes, &len);
        /* ld1 = 111 (from memory); store 555; ld2 = 555 (must come from
         * memory, not from ld1, because the store changed it).
         * Expected: 111 + 555*1000 = 555111 */
        printf("  buf[0] = 111, then store 555, then load\n");
        printf("  expect 111 + 555*1000 = 555111\n");
        printf("  result = %ld\n", (long)r);

        TEST("result == 555111 (GVN did not cross same-addr store)",
             r == 555111);

        kyir_graph_destroy(g);
    }

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
