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
    printf("KYJIT μOp Cache Test Suite v%s\n", KYUOP_VERSION_STRING);
    printf("========================================\n\n");

    /* Test 1: Create + destroy */
    printf("[Test] create_destroy\n");
    KyUopCache *cache = kyuop_create();
    TEST("create returns non-NULL", cache != NULL);
    TEST("initial block count is 0", kyuop_count(cache) == 0);
    TEST("initial size is 0", kyuop_size(cache) == 0);
    kyuop_destroy(cache);

    /* Test 2: Lookup on empty cache */
    printf("\n[Test] lookup_empty\n");
    cache = kyuop_create();
    uint64_t hash = kyuop_hash_pattern((const uint8_t *)"test", 4);
    KyUopBlock *b = kyuop_lookup(cache, hash);
    TEST("lookup on empty returns NULL", b == NULL);
    kyuop_destroy(cache);

    /* Test 3: Compile + lookup */
    printf("\n[Test] compile_lookup\n");
    cache = kyuop_create();
    const uint8_t bc[] = { UOP_ADD_I64, UOP_RETURN };
    uint64_t h = kyuop_hash_pattern(bc, sizeof(bc));
    KyUopBlock *block = kyuop_compile(cache, h, bc, sizeof(bc));
    TEST("compile returns non-NULL", block != NULL);
    TEST("block count is 1", kyuop_count(cache) == 1);
    TEST("size is 16 (7 emitted + padding)", kyuop_size(cache) == 16);
    KyUopBlock *found = kyuop_lookup(cache, h);
    TEST("lookup finds the block", found == block);
    TEST("hash matches", found->pattern_hash == h);
    kyuop_destroy(cache);

    /* Test 4: Execute cached block */
    printf("\n[Test] execute\n");
    cache = kyuop_create();
    const uint8_t bc2[] = { UOP_ADD_I64, UOP_RETURN };
    uint64_t h2 = kyuop_hash_pattern(bc2, sizeof(bc2));
    kyuop_compile(cache, h2, bc2, sizeof(bc2));
    int64_t result = kyuop_execute(cache, h2, 5, 7);
    TEST("execute add(5,7) = 12", result == 12);
    result = kyuop_execute(cache, h2, 100, 200);
    TEST("execute add(100,200) = 300", result == 300);
    kyuop_destroy(cache);

    /* Test 5: Hash quality */
    printf("\n[Test] hash\n");
    const uint8_t a[] = { 1, 2, 3 };
    const uint8_t bc_b[] = { 1, 2, 4 };
    uint64_t ha = kyuop_hash_pattern(a, 3);
    uint64_t hb = kyuop_hash_pattern(bc_b, 3);
    TEST("different patterns → different hashes", ha != hb);
    TEST("hash is non-zero", ha != 0);
    uint64_t ha2 = kyuop_hash_pattern(a, 3);
    TEST("hash is deterministic", ha == ha2);

    /* Test 6: Stats */
    printf("\n[Test] stats\n");
    cache = kyuop_create();
    const uint8_t bc3[] = { UOP_MUL_I64 };
    uint64_t h3 = kyuop_hash_pattern(bc3, 1);
    kyuop_compile(cache, h3, bc3, 1);
    kyuop_lookup(cache, h3);
    kyuop_lookup(cache, h3);
    KyUopStats stats;
    kyuop_get_stats(cache, &stats);
    TEST("stats: compilations >= 1", stats.compilations >= 1);
    TEST("stats: hits >= 2", stats.hits >= 2);
    kyuop_destroy(cache);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}
