#include "kyjit_uop_cache.h"
#include <stdio.h>
#include <string.h>

int main(void) {
    printf("KYJIT μOp Cache Scale Test\n");
    printf("========================================\n\n");

    KyUopCache *cache = kyuop_create();
    if (!cache) { printf("FAIL: create failed\n"); return 1; }

    /* Insert 1000 unique patterns */
    const int N = 1000;
    int compiled = 0;

    for (int i = 0; i < N; i++) {
        uint8_t bc[16];
        bc[0] = UOP_ADD_I64 + (i % 6);  /* rotate through ops */
        bc[1] = UOP_LOAD_CONST;
        int64_t val = i * 1000;
        memcpy(bc + 2, &val, 8);
        bc[10] = UOP_RETURN;

        uint64_t h = kyuop_hash_pattern(bc, 11);
        KyUopBlock *b = kyuop_compile(cache, h, bc, 11);
        if (b) compiled++;
    }

    printf("Compiled: %d / %d\n", compiled, N);
    printf("Blocks: %u\n", kyuop_count(cache));
    printf("Arena used: %zu bytes\n", kyuop_size(cache));

    /* Lookup all patterns */
    int hits = 0, misses = 0;
    for (int i = 0; i < N; i++) {
        uint8_t bc[16];
        bc[0] = UOP_ADD_I64 + (i % 6);
        bc[1] = UOP_LOAD_CONST;
        int64_t val = i * 1000;
        memcpy(bc + 2, &val, 8);
        bc[10] = UOP_RETURN;

        uint64_t h = kyuop_hash_pattern(bc, 11);
        KyUopBlock *b = kyuop_lookup(cache, h);
        if (b) hits++; else misses++;
    }

    printf("Lookup hits: %d / %d\n", hits, N);
    printf("Lookup misses: %d\n", misses);

    KyUopStats stats;
    kyuop_get_stats(cache, &stats);
    printf("\nStats:\n");
    printf("  compilations: %llu\n", (unsigned long long)stats.compilations);
    printf("  hits:         %llu\n", (unsigned long long)stats.hits);
    printf("  misses:       %llu\n", (unsigned long long)stats.misses);
    printf("  evictions:    %llu\n", (unsigned long long)stats.evictions);

    kyuop_destroy(cache);

    printf("\n========================================\n");
    if (compiled == N && hits == N) {
        printf("RESULT: PASS\n");
        return 0;
    } else {
        printf("RESULT: FAIL\n");
        return 1;
    }
}
