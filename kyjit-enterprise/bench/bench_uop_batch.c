/*
 * bench/bench_uop_batch.c — Batch execution benchmark
 *
 * Demonstrates: batching N ops into one native block amortizes
 * the indirect call cost over N operations.
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

#define ITERS 10000000ULL
#define RUNS  5

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

static volatile int64_t g_sink = 0;

typedef int64_t (*KyUopFn)(int64_t, int64_t);

/* Batch size N: run N adds in one block */
static double bench_batch(int batch_size) {
    KyUopCache *cache = kyuop_create();

    /* Build bytecode: [ADD, ADD, ..., ADD, RETURN] */
    uint8_t bc[256];
    if (batch_size > 254) batch_size = 254;
    for (int i = 0; i < batch_size; i++) bc[i] = UOP_ADD_I64;
    bc[batch_size] = UOP_RETURN;

    uint64_t h = kyuop_hash_pattern(bc, batch_size + 1);
    KyUopBlock *blk = kyuop_compile(cache, h, bc, batch_size + 1);
    if (!blk) { kyuop_destroy(cache); return -1; }

    KyUopFn fn = (KyUopFn)blk->native_code;

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t acc = 0;
        for (uint64_t i = 0; i < 1000000ULL; i++) {
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

/* Interpreter for the same batch */
__attribute__((noinline, optimize("O0")))
static int64_t interp_run(volatile const uint8_t *bc, size_t len, int64_t a, int64_t b) {
    int64_t result = 0;
    for (size_t i = 0; i < len; ) {
        uint8_t op = bc[i];
        switch (op) {
            case UOP_ADD_I64: result += a + b; i++; break;
            case UOP_RETURN: return result;
            default: i++;
        }
    }
    return result;
}

static double bench_interp_batch(int batch_size) {
    uint8_t bc[256];
    if (batch_size > 254) batch_size = 254;
    for (int i = 0; i < batch_size; i++) bc[i] = UOP_ADD_I64;
    bc[batch_size] = UOP_RETURN;

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        int64_t acc = 0;
        for (uint64_t i = 0; i < 1000000ULL; i++) {
            acc += interp_run(bc, batch_size + 1, 5, 7);
        }
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) {
            acc += interp_run(bc, batch_size + 1, 5, 7);
        }
        uint64_t t1 = ns_now();
        g_sink = acc;
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }

    qsort(samples, RUNS, sizeof(double), cmp_double);
    return samples[RUNS / 2];
}

int main(void) {
    int batches[] = { 1, 2, 4, 8, 16, 32, 64, 128 };
    int n = sizeof(batches) / sizeof(batches[0]);

    printf("{\n");
    printf("  \"benchmark\": \"kyjit_uop_batch\",\n");
    printf("  \"hardware\": \"Intel i5-1240P\",\n");
    printf("  \"iters\": %llu,\n", (unsigned long long)ITERS);
    printf("  \"results\": [\n");

    for (int i = 0; i < n; i++) {
        int bs = batches[i];
        double uop_ns = bench_batch(bs);
        double interp_ns = bench_interp_batch(bs);

        double uop_per_op = uop_ns / bs;
        double interp_per_op = interp_ns / bs;

        printf("    {\n");
        printf("      \"batch_size\": %d,\n", bs);
        printf("      \"uop_total_ns\": %.4f,\n", uop_ns);
        printf("      \"uop_per_op_ns\": %.4f,\n", uop_per_op);
        printf("      \"interp_total_ns\": %.4f,\n", interp_ns);
        printf("      \"interp_per_op_ns\": %.4f,\n", interp_per_op);
        printf("      \"speedup_total\": %.2f,\n", interp_ns / uop_ns);
        printf("      \"speedup_per_op\": %.2f\n", interp_per_op / uop_per_op);
        printf("    }%s\n", (i < n - 1) ? "," : "");
    }

    printf("  ],\n");
    printf("  \"date\": \"2026-09-19\"\n");
    printf("}\n");

    return 0;
}
