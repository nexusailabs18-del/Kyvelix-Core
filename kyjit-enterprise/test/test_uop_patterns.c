#include "kyjit_uop_cache.h"
#include <stdio.h>
#include <string.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

int main(void) {
    printf("KYJIT μOp Pattern Test v%s\n", KYUOP_VERSION_STRING);
    printf("========================================\n\n");

    KyUopCache *cache = kyuop_create();

    /* Test ADD */
    printf("[Test] ADD_I64\n");
    const uint8_t bc_add[] = { UOP_ADD_I64, UOP_RETURN };
    uint64_t h_add = kyuop_hash_pattern(bc_add, sizeof(bc_add));
    kyuop_compile(cache, h_add, bc_add, sizeof(bc_add));
    TEST("add(5, 7) = 12", kyuop_execute(cache, h_add, 5, 7) == 12);
    TEST("add(-3, 10) = 7", kyuop_execute(cache, h_add, -3, 10) == 7);
    TEST("add(0, 0) = 0", kyuop_execute(cache, h_add, 0, 0) == 0);

    /* Test SUB */
    printf("\n[Test] SUB_I64\n");
    const uint8_t bc_sub[] = { UOP_SUB_I64, UOP_RETURN };
    uint64_t h_sub = kyuop_hash_pattern(bc_sub, sizeof(bc_sub));
    kyuop_compile(cache, h_sub, bc_sub, sizeof(bc_sub));
    TEST("sub(10, 3) = 7", kyuop_execute(cache, h_sub, 10, 3) == 7);
    TEST("sub(0, 5) = -5", kyuop_execute(cache, h_sub, 0, 5) == -5);

    /* Test MUL */
    printf("\n[Test] MUL_I64\n");
    const uint8_t bc_mul[] = { UOP_MUL_I64, UOP_RETURN };
    uint64_t h_mul = kyuop_hash_pattern(bc_mul, sizeof(bc_mul));
    kyuop_compile(cache, h_mul, bc_mul, sizeof(bc_mul));
    TEST("mul(6, 7) = 42", kyuop_execute(cache, h_mul, 6, 7) == 42);
    TEST("mul(-4, 5) = -20", kyuop_execute(cache, h_mul, -4, 5) == -20);

    /* Test DIV */
    printf("\n[Test] DIV_I64\n");
    const uint8_t bc_div[] = { UOP_DIV_I64, UOP_RETURN };
    uint64_t h_div = kyuop_hash_pattern(bc_div, sizeof(bc_div));
    kyuop_compile(cache, h_div, bc_div, sizeof(bc_div));
    TEST("div(20, 4) = 5", kyuop_execute(cache, h_div, 20, 4) == 5);
    TEST("div(100, 7) = 14", kyuop_execute(cache, h_div, 100, 7) == 14);

    /* Test INC */
    printf("\n[Test] INC_I64\n");
    const uint8_t bc_inc[] = { UOP_INC_I64, UOP_RETURN };
    uint64_t h_inc = kyuop_hash_pattern(bc_inc, sizeof(bc_inc));
    kyuop_compile(cache, h_inc, bc_inc, sizeof(bc_inc));
    TEST("inc(41, 0) = 42", kyuop_execute(cache, h_inc, 41, 0) == 42);

    /* Test DEC */
    printf("\n[Test] DEC_I64\n");
    const uint8_t bc_dec[] = { UOP_DEC_I64, UOP_RETURN };
    uint64_t h_dec = kyuop_hash_pattern(bc_dec, sizeof(bc_dec));
    kyuop_compile(cache, h_dec, bc_dec, sizeof(bc_dec));
    TEST("dec(43, 0) = 42", kyuop_execute(cache, h_dec, 43, 0) == 42);

    /* Test LOAD_CONST */
    printf("\n[Test] LOAD_CONST\n");
    uint8_t bc_const[10];
    bc_const[0] = UOP_LOAD_CONST;
    int64_t val = 12345;
    memcpy(bc_const + 1, &val, 8);
    bc_const[9] = UOP_RETURN;
    uint64_t h_const = kyuop_hash_pattern(bc_const, 10);
    kyuop_compile(cache, h_const, bc_const, 10);
    TEST("load_const(12345) = 12345", kyuop_execute(cache, h_const, 0, 0) == 12345);

    /* Test CMP_EQ */
    printf("\n[Test] CMP_EQ\n");
    const uint8_t bc_cmp[] = { UOP_CMP_EQ, UOP_RETURN };
    uint64_t h_cmp = kyuop_hash_pattern(bc_cmp, sizeof(bc_cmp));
    kyuop_compile(cache, h_cmp, bc_cmp, sizeof(bc_cmp));
    TEST("cmp_eq(5, 5) = 1", kyuop_execute(cache, h_cmp, 5, 5) == 1);
    TEST("cmp_eq(5, 6) = 0", kyuop_execute(cache, h_cmp, 5, 6) == 0);

    /* Stats */
    printf("\n[Test] stats\n");
    KyUopStats stats;
    kyuop_get_stats(cache, &stats);
    printf("  Blocks: %u\n", stats.block_count);
    printf("  Arena used: %zu bytes\n", stats.arena_used);
    printf("  Compilations: %llu\n", (unsigned long long)stats.compilations);
    printf("  Hits: %llu\n", (unsigned long long)stats.hits);

    kyuop_destroy(cache);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
