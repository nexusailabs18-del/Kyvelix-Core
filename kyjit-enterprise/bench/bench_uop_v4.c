/*
 * bench/bench_uop_v4.c — Pattern benchmark for v4.0
 *
 * Tests v4.0's pattern recognition on:
 *   - ADD + SUB (cancellation)
 *   - INC + DEC (cancellation)
 *   - INC × 4
 *   - LOAD_CONST + LOAD_CONST (last wins)
 *   - LOAD_CONST + ADD
 *   - LOAD_CONST + MUL
 */

#define _GNU_SOURCE
#include "kyjit_uop_cache.h"
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static inline uint64_t ns_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

#define ITERS 10000000ULL
#define RUNS  5

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

static volatile int64_t g_sink = 0;

typedef int64_t (*KyUopFn)(int64_t, int64_t);

static double bench_pattern(const char *name, const uint8_t *bc, size_t len,
                            int64_t a, int64_t b, int64_t expected)
{
    KyUopCache *cache = kyuop_create();
    uint64_t h = kyuop_hash_pattern(bc, len);
    KyUopBlock *blk = kyuop_compile(cache, h, bc, len);
    if (!blk) { kyuop_destroy(cache); return -1; }

    KyUopFn fn = (KyUopFn)blk->native_code;

    /* Verify correctness */
    int64_t r = fn(a, b);
    if (r != expected) {
        printf("    // %s: FAILED correctness (got %lld, want %lld)\n",
               name, (long long)r, (long long)expected);
    }

    double samples[RUNS];
    for (int i = 0; i < RUNS; i++) {
        int64_t acc = 0;
        for (uint64_t j = 0; j < 1000000ULL; j++) acc += fn(a, b);
        uint64_t t0 = ns_now();
        for (uint64_t j = 0; j < ITERS; j++) acc += fn(a, b);
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[i] = (double)(t1 - t0) / (double)ITERS;
    }

    kyuop_destroy(cache);
    qsort(samples, RUNS, sizeof(double), cmp_double);
    double ns = samples[RUNS / 2];
    printf("    { \"name\": \"%s\", \"ns\": %.4f, \"ok\": %s }\n",
           name, ns, (r == expected) ? "true" : "false");
    return ns;
}

int main(void) {
    printf("{\n");
    printf("  \"benchmark\": \"kyjit_uop_v4\",\n");
    printf("  \"version\": \"%s\",\n", KYUOP_VERSION_STRING);
    printf("  \"hardware\": \"Intel i5-1240P\",\n");
    printf("  \"iters\": %llu,\n", (unsigned long long)ITERS);
    printf("  \"patterns\": [\n");

    /* Pattern 1: ADD, SUB → nop */
    {
        uint8_t bc[] = { UOP_ADD_I64, UOP_SUB_I64, UOP_RETURN };
        bench_pattern("add_sub_cancel", bc, sizeof(bc), 5, 7, 5);
    }

    /* Pattern 2: INC, DEC → nop */
    {
        uint8_t bc[] = { UOP_INC_I64, UOP_DEC_I64, UOP_RETURN };
        bench_pattern("inc_dec_cancel", bc, sizeof(bc), 5, 0, 5);
    }

    /* Pattern 3: INC × 4 → add rax, 4 */
    {
        uint8_t bc[] = { UOP_INC_I64, UOP_INC_I64, UOP_INC_I64, UOP_INC_I64, UOP_RETURN };
        bench_pattern("inc_x4", bc, sizeof(bc), 5, 0, 9);
    }

    /* Pattern 4: LOAD_CONST, LOAD_CONST → last wins */
    {
        uint8_t bc[19];
        bc[0] = UOP_LOAD_CONST;
        int64_t v1 = 100;
        memcpy(bc + 1, &v1, 8);
        bc[9] = UOP_LOAD_CONST;
        int64_t v2 = 200;
        memcpy(bc + 10, &v2, 8);
        bc[18] = UOP_RETURN;
        bench_pattern("load_const_last", bc, 19, 5, 7, 200);
    }

    /* Pattern 5: LOAD_CONST, ADD → lea + add */
    {
        uint8_t bc[11];
        bc[0] = UOP_LOAD_CONST;
        int64_t v = 42;
        memcpy(bc + 1, &v, 8);
        bc[9] = UOP_ADD_I64;
        bc[10] = UOP_RETURN;
        bench_pattern("load_const_add", bc, 11, 5, 7, 42 + 7);
    }

    /* Pattern 6: LOAD_CONST, MUL → imul imm */
    {
        uint8_t bc[11];
        bc[0] = UOP_LOAD_CONST;
        int64_t v = 3;
        memcpy(bc + 1, &v, 8);
        bc[9] = UOP_MUL_I64;
        bc[10] = UOP_RETURN;
        bench_pattern("load_const_mul", bc, 11, 5, 7, 3 * 7);
    }

    printf("    { \"name\": \"_end\", \"ns\": 0.0, \"ok\": true }\n");
    printf("  ],\n");
    printf("  \"date\": \"2026-09-19\"\n");
    printf("}\n");
    return 0;
}
