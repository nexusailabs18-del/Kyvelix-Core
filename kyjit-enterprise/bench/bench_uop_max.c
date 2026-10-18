/*
 * bench/bench_uop_max.c — Maximum μOp cache speed
 *
 * Caches fn pointer outside the loop (opt A).
 * Compares against the naive per-iteration lookup.
 */

#define _GNU_SOURCE
#include "kyjit_uop_cache.h"
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

static inline uint64_t ns_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

#define ITERS 100000000ULL
#define RUNS  5

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

static volatile int64_t g_sink = 0;
typedef int64_t (*KyUopFn)(int64_t, int64_t);

/* Naive: lookup every iteration */
static double bench_naive(int batch_size) {
    KyUopCache *cache = kyuop_create();
    uint8_t bc[256];
    for (int i = 0; i < batch_size; i++) bc[i] = UOP_ADD_I64;
    bc[batch_size] = UOP_RETURN;
    uint64_t h = kyuop_hash_pattern(bc, batch_size + 1);
    kyuop_compile(cache, h, bc, batch_size + 1);

    uint32_t idx = (uint32_t)h & (KYUOP_HASH_SLOTS - 1);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t acc = 0;
        for (uint64_t i = 0; i < 1000000ULL; i++) {
            KyUopFn fn = (KyUopFn)cache->hash_table[idx].native_code;
            acc += fn(5, 7);
        }
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) {
            if (cache->hash_table[idx].pattern_hash == h) {
                KyUopFn fn = (KyUopFn)cache->hash_table[idx].native_code;
                acc += fn(5, 7);
            }
        }
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    kyuop_destroy(cache);
    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

/* Cached: fn pointer held in register */
static double bench_cached(int batch_size) {
    KyUopCache *cache = kyuop_create();
    uint8_t bc[256];
    for (int i = 0; i < batch_size; i++) bc[i] = UOP_ADD_I64;
    bc[batch_size] = UOP_RETURN;
    uint64_t h = kyuop_hash_pattern(bc, batch_size + 1);
    KyUopBlock *blk = kyuop_compile(cache, h, bc, batch_size + 1);
    KyUopFn fn = (KyUopFn)blk->native_code;

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t acc = 0;
        for (uint64_t i = 0; i < 1000000ULL; i++) acc += fn(5, 7);
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) acc += fn(5, 7);
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    kyuop_destroy(cache);
    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

int main(void) {
    printf("{\n");
    printf("  \"benchmark\": \"kyjit_uop_max\",\n");
    printf("  \"hardware\": \"Intel i5-1240P\",\n");
    printf("  \"iters\": %llu,\n", (unsigned long long)ITERS);
    printf("  \"results\": [\n");

    int batches[] = { 1, 4, 16, 64, 128, 256 };
    int n = sizeof(batches) / sizeof(batches[0]);

    for (int i = 0; i < n; i++) {
        double naive = bench_naive(batches[i]);
        double cached = bench_cached(batches[i]);
        printf("    { \"batch\": %d, \"naive_ns\": %.4f, \"cached_ns\": %.4f, \"speedup\": %.2f }%s\n",
               batches[i], naive, cached, naive / cached,
               (i < n - 1) ? "," : "");
    }

    printf("  ],\n");
    printf("  \"date\": \"2026-09-19\"\n");
    printf("}\n");
    return 0;
}
