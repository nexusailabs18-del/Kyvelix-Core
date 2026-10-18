/*
 * bench/bench_ic_all.c — Definitive speed test for all IC states
 *
 * Measures: mono, poly_2, poly_4, mega_8, mega_16, overflow_100, miss
 * Uses rdtscp, 100M iterations per case, 5 runs, reports best/median
 *
 * Build:
 *   gcc -O3 -march=native -mfma -mavx2 -I./include \
 *       bench/bench_ic_all.c src/kyjit_ic.c -o bench/bench_ic_all
 *
 * Run:
 *   taskset -c 2 ./bench/bench_ic_all
 */

#define _GNU_SOURCE
#include "kyjit_ic.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <time.h>

static inline uint64_t ns_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static volatile uint64_t g_sink = 0;

#define ITERS 100000000ULL
#define RUNS  5
#define WARMUP 1000000ULL

typedef struct { double min, median, max; } Stats;

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

static Stats compute_stats(double *samples, int n) {
    qsort(samples, n, sizeof(double), cmp_double);
    Stats s;
    s.min = samples[0];
    s.median = samples[n / 2];
    s.max = samples[n - 1];
    return s;
}

/* ---------- lookup cases ---------- */

static void bench_mono(void) {
    KyIC ic;
    ky_ic_init(&ic);
    ky_ic_update(&ic, 42, 16);

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, 42);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, 42);
        uint64_t t1 = ns_now();
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }
    g_sink += acc;
    ky_ic_destroy(&ic);

    Stats s = compute_stats(samples, RUNS);
    printf("  \"mono\":      { \"best\": %.4f, \"median\": %.4f, \"worst\": %.4f },\n",
           s.min, s.median, s.max);
}

static void bench_poly(const char *name, uint32_t n) {
    KyIC ic;
    ky_ic_init(&ic);
    uint32_t shapes[4];
    for (uint32_t i = 0; i < n; i++) {
        shapes[i] = 100 + i * 17;
        ky_ic_update(&ic, shapes[i], (int32_t)(i * 8));
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, shapes[i % n]);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, shapes[i % n]);
        uint64_t t1 = ns_now();
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }
    g_sink += acc;
    ky_ic_destroy(&ic);

    Stats s = compute_stats(samples, RUNS);
    printf("  \"%s\":     { \"best\": %.4f, \"median\": %.4f, \"worst\": %.4f },\n",
           name, s.min, s.median, s.max);
}

static void bench_mega(const char *name, uint32_t n) {
    KyIC ic;
    ky_ic_init(&ic);
    uint32_t shapes[32];
    for (uint32_t i = 0; i < n; i++) {
        shapes[i] = 1000 + i * 31;
        ky_ic_update(&ic, shapes[i], (int32_t)(i * 4));
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, shapes[i % n]);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, shapes[i % n]);
        uint64_t t1 = ns_now();
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }
    g_sink += acc;
    ky_ic_destroy(&ic);

    Stats s = compute_stats(samples, RUNS);
    printf("  \"%s\":   { \"best\": %.4f, \"median\": %.4f, \"worst\": %.4f },\n",
           name, s.min, s.median, s.max);
}

static void bench_overflow(uint32_t n) {
    KyIC ic;
    ky_ic_init(&ic);
    uint32_t *shapes = malloc(n * sizeof(uint32_t));
    for (uint32_t i = 0; i < n; i++) {
        shapes[i] = 10000 + i * 7;
        ky_ic_update(&ic, shapes[i], (int32_t)(i * 4));
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, shapes[i % n]);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, shapes[i % n]);
        uint64_t t1 = ns_now();
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }
    g_sink += acc;

    char name[32];
    snprintf(name, sizeof(name), "overflow_%u", n);
    Stats s = compute_stats(samples, RUNS);
    printf("  \"%s\": { \"best\": %.4f, \"median\": %.4f, \"worst\": %.4f },\n",
           name, s.min, s.median, s.max);

    free(shapes);
    ky_ic_destroy(&ic);
}

static void bench_miss(void) {
    KyIC ic;
    ky_ic_init(&ic);
    ky_ic_update(&ic, 42, 16);

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, 999);

    double samples[RUNS];
    for (int r = 0; r < RUNS; r++) {
        uint64_t t0 = ns_now();
        for (uint64_t i = 0; i < ITERS; i++) acc += (uint32_t)ky_ic_lookup_inline(&ic, 999);
        uint64_t t1 = ns_now();
        samples[r] = (double)(t1 - t0) / (double)ITERS;
    }
    g_sink += acc;
    ky_ic_destroy(&ic);

    Stats s = compute_stats(samples, RUNS);
    printf("  \"miss\":      { \"best\": %.4f, \"median\": %.4f, \"worst\": %.4f },\n",
           s.min, s.median, s.max);
}

/* ---------- main ---------- */

int main(void) {
    printf("{\n");
    printf("  \"benchmark\": \"kyjit_ic_all\",\n");
    printf("  \"hardware\": \"Intel i5-1240P\",\n");
    printf("  \"iters\": %llu,\n", (unsigned long long)ITERS);
    printf("  \"runs\": %d,\n", RUNS);
    printf("  \"results_ns_per_op\": {\n");

    bench_mono();
    bench_poly("poly_2", 2);
    bench_poly("poly_4", 4);
    bench_mega("mega_8", 8);
    bench_mega("mega_16", 16);
    bench_overflow(32);
    bench_overflow(64);
    bench_overflow(100);
    bench_miss();

    printf("    \"_end\": true\n");
    printf("  }\n");
    printf("}\n");

    return 0;
}
