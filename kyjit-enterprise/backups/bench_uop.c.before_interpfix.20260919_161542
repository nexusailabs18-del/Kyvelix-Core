/*
 * bench/bench_uop.c — μOp cache benchmark (corrected)
 *
 * Fixes:
 *   1. Interpreter uses non-constant input (no folding)
 *   2. Lookup measured without volatile stores in loop
 *   3. Execute measured with proper indirect call
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

#define ITERS     50000000ULL
#define WARMUP    1000000ULL
#define RUNS      5

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

/* Prevents compiler from folding */
static volatile int64_t g_sink = 0;

/* ---------- 1. Lookup latency ---------- */

static double bench_lookup(void) {
    KyUopCache *cache = kyuop_create();
    const uint8_t bc[] = { UOP_ADD_I64, UOP_RETURN };
    uint64_t h = kyuop_hash_pattern(bc, 2);
    kyuop_compile(cache, h, bc, 2);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        /* Accumulate into local, store once at end */
        uintptr_t acc = 0;
        for (uint64_t i = 0; i < WARMUP; i++) {
            KyUopBlock *b = kyuop_lookup(cache, h);
            acc += (uintptr_t)b;
        }
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) {
            KyUopBlock *b = kyuop_lookup(cache, h);
            acc += (uintptr_t)b;
        }
        uint64_t t1 = ns_now();
        g_sink = (int64_t)acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    kyuop_destroy(cache);
    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

/* ---------- 2. Execute latency ---------- */

static double bench_execute(void) {
    KyUopCache *cache = kyuop_create();
    const uint8_t bc[] = { UOP_ADD_I64, UOP_RETURN };
    uint64_t h = kyuop_hash_pattern(bc, 2);
    kyuop_compile(cache, h, bc, 2);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t acc = 0;
        for (uint64_t i = 0; i < WARMUP; i++) {
            acc += kyuop_execute(cache, h, 5, 7);
        }
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) {
            acc += kyuop_execute(cache, h, 5, 7);
        }
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    kyuop_destroy(cache);
    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

/* ---------- 3. Interpreter baseline (non-foldable) ---------- */

static int64_t interp_execute(const uint8_t *bc, size_t len, int64_t a, int64_t b) {
    int64_t result = 0;
    for (size_t i = 0; i < len; ) {
        uint8_t op = bc[i];
        switch (op) {
            case UOP_ADD_I64: result = a + b; i++; break;
            case UOP_SUB_I64: result = a - b; i++; break;
            case UOP_MUL_I64: result = a * b; i++; break;
            case UOP_LOAD_CONST: {
                if (i + 9 <= len) {
                    memcpy(&result, bc + i + 1, 8);
                    i += 9;
                } else {
                    i++;
                }
                break;
            }
            case UOP_RETURN:  return result;
            default: i++;
        }
    }
    return result;
}

static double bench_interpreter(void) {
    /* Vary the bytecode so the compiler can't fold */
    uint8_t bc[11];
    bc[0] = UOP_ADD_I64;
    bc[1] = UOP_LOAD_CONST;
    /* value varies per run */
    bc[10] = UOP_RETURN;

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t val = 100 + r;
        memcpy(bc + 2, &val, 8);

        int64_t acc = 0;
        for (uint64_t i = 0; i < WARMUP; i++) {
            acc += interp_execute(bc, 11, 5, 7);
        }
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) {
            acc += interp_execute(bc, 11, 5, 7);
        }
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

/* ---------- 4. Direct native call (no lookup) ---------- */

static double bench_direct_call(void) {
    KyUopCache *cache = kyuop_create();
    const uint8_t bc[] = { UOP_ADD_I64, UOP_RETURN };
    uint64_t h = kyuop_hash_pattern(bc, 2);
    KyUopBlock *blk = kyuop_compile(cache, h, bc, 2);

    typedef int64_t (*Fn)(int64_t, int64_t);
    Fn fn = (Fn)blk->native_code;

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t acc = 0;
        for (uint64_t i = 0; i < WARMUP; i++) {
            acc += fn(5, 7);
        }
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) {
            acc += fn(5, 7);
        }
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    kyuop_destroy(cache);
    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

/* ---------- main ---------- */

int main(void) {
    printf("{\n");
    printf("  \"benchmark\": \"kyjit_uop_cache\",\n");
    printf("  \"hardware\": \"Intel i5-1240P\",\n");
    printf("  \"iters\": %llu,\n", (unsigned long long)ITERS);

    double lookup_ns = bench_lookup();
    double execute_ns = bench_execute();
    double interp_ns = bench_interpreter();
    double direct_ns = bench_direct_call();

    printf("  \"results_ns_per_op\": {\n");
    printf("    \"lookup\":          %.4f,\n", lookup_ns);
    printf("    \"execute\":         %.4f,\n", execute_ns);
    printf("    \"direct_call\":     %.4f,\n", direct_ns);
    printf("    \"interpreter\":     %.4f\n", interp_ns);
    printf("  },\n");
    printf("  \"speedup_execute_vs_interp\": %.2f,\n", interp_ns / execute_ns);
    printf("  \"speedup_direct_vs_interp\":  %.2f,\n", interp_ns / direct_ns);
    printf("  \"date\": \"2026-09-19\"\n");
    printf("}\n");

    return 0;
}
