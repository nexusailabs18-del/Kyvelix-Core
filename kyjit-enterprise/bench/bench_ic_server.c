/*
 * bench/bench_ic_server.c — Server-grade IC benchmark
 *
 * Measures IC speed across all states under realistic server workloads.
 *
 * Build:
 *   gcc -O3 -march=native -mfma -mavx2 -pthread \
 *       -I./include bench/bench_ic_server.c src/kyjit_ic.c -o bench/bench_ic_server
 *
 * Run:
 *   taskset -c 2 ./bench/bench_ic_server
 */

#define _GNU_SOURCE
#include "kyjit_ic.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>

/* ---------- timing ---------- */

static inline uint64_t rdtscp(void) {
    uint32_t lo, hi;
    __asm__ __volatile__("rdtscp" : "=a"(lo), "=d"(hi) :: "rcx");
    return ((uint64_t)hi << 32) | lo;
}

static inline uint64_t ns_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

/* ---------- benchmark config ---------- */

#define ITERS_PER_CASE 100000000ULL
#define WARMUP_ITERS   1000000ULL
#define NUM_ICS_SMALL  1000
#define NUM_ICS_LARGE  100000

static volatile uint64_t g_sink = 0;

/* ---------- 1. Scale test: 100K ICs ---------- */

static void test_scale(void) {
    printf("  \"scale_test\": {\n");

    uint64_t t0 = ns_now();
    KyIC *ics = calloc(NUM_ICS_LARGE, sizeof(KyIC));
    if (!ics) { printf("    \"error\": \"alloc failed\"\n  },\n"); return; }

    for (int i = 0; i < NUM_ICS_LARGE; i++) {
        ky_ic_init(&ics[i]);
        /* Each IC gets 1-32 shapes */
        uint32_t num_shapes = 1 + (i % 32);
        for (uint32_t j = 0; j < num_shapes; j++) {
            ky_ic_update(&ics[i], (i * 31 + j * 7) % 4096, (int32_t)(j * 4));
        }
    }

    uint64_t t1 = ns_now();
    uint64_t init_ns = t1 - t0;

    /* Verify each IC */
    t0 = ns_now();
    uint64_t mismatches = 0;
    for (int i = 0; i < NUM_ICS_LARGE; i++) {
        uint32_t num_shapes = 1 + (i % 32);
        for (uint32_t j = 0; j < num_shapes; j++) {
            uint32_t shape = (i * 31 + j * 7) % 4096;
            int32_t want = (int32_t)(j * 4);
            int32_t got = ky_ic_lookup_inline(&ics[i], shape);
            if (got != want) mismatches++;
        }
    }
    t1 = ns_now();

    printf("    \"num_ics\": %d,\n", NUM_ICS_LARGE);
    printf("    \"init_ns\": %llu,\n", (unsigned long long)init_ns);
    printf("    \"verify_ns\": %llu,\n", (unsigned long long)(t1 - t0));
    printf("    \"mismatches\": %llu\n", (unsigned long long)mismatches);

    for (int i = 0; i < NUM_ICS_LARGE; i++) ky_ic_destroy(&ics[i]);
    free(ics);

    printf("  },\n");
}

/* ---------- 2. Mono speed ---------- */

static void bench_mono(void) {
    KyIC ic;
    ky_ic_init(&ic);
    ky_ic_update(&ic, 42, 16);

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP_ITERS; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, 42);
    }

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < ITERS_PER_CASE; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, 42);
    }
    uint64_t t1 = ns_now();

    g_sink += acc;
    ky_ic_destroy(&ic);

    double ns_per_op = (double)(t1 - t0) / (double)ITERS_PER_CASE;
    printf("  \"mono\": { \"ns_per_op\": %.4f },\n", ns_per_op);
}

/* ---------- 3. Poly speed (4 shapes) ---------- */

static void bench_poly4(void) {
    KyIC ic;
    ky_ic_init(&ic);
    ky_ic_update(&ic, 10, 16);
    ky_ic_update(&ic, 20, 32);
    ky_ic_update(&ic, 30, 48);
    ky_ic_update(&ic, 40, 64);

    uint32_t shapes[4] = {10, 20, 30, 40};
    uint64_t acc = 0;

    for (uint64_t i = 0; i < WARMUP_ITERS; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shapes[i & 3]);
    }

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < ITERS_PER_CASE; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shapes[i & 3]);
    }
    uint64_t t1 = ns_now();

    g_sink += acc;
    ky_ic_destroy(&ic);

    double ns_per_op = (double)(t1 - t0) / (double)ITERS_PER_CASE;
    printf("  \"poly_4\": { \"ns_per_op\": %.4f },\n", ns_per_op);
}

/* ---------- 4. Mega speed (16 shapes) ---------- */

static void bench_mega16(void) {
    KyIC ic;
    ky_ic_init(&ic);

    uint32_t shapes[16];
    for (uint32_t j = 0; j < 16; j++) {
        shapes[j] = 100 + j * 17;
        ky_ic_update(&ic, shapes[j], (int32_t)(j * 8));
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP_ITERS; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shapes[i & 15]);
    }

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < ITERS_PER_CASE; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shapes[i & 15]);
    }
    uint64_t t1 = ns_now();

    g_sink += acc;
    ky_ic_destroy(&ic);

    double ns_per_op = (double)(t1 - t0) / (double)ITERS_PER_CASE;
    printf("  \"mega_16\": { \"ns_per_op\": %.4f },\n", ns_per_op);
}

/* ---------- 5. Overflow speed (100 shapes) ---------- */

static void bench_overflow(void) {
    KyIC ic;
    ky_ic_init(&ic);

    uint32_t shapes[100];
    for (uint32_t j = 0; j < 100; j++) {
        shapes[j] = 10000 + j * 31;
        ky_ic_update(&ic, shapes[j], (int32_t)(j * 8));
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP_ITERS; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shapes[i % 100]);
    }

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < ITERS_PER_CASE; i++) {
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shapes[i % 100]);
    }
    uint64_t t1 = ns_now();

    g_sink += acc;
    ky_ic_destroy(&ic);

    double ns_per_op = (double)(t1 - t0) / (double)ITERS_PER_CASE;
    printf("  \"overflow_100\": { \"ns_per_op\": %.4f },\n", ns_per_op);
}

/* ---------- 6. Concurrent test ---------- */

typedef struct {
    int thread_id;
    uint64_t iterations;
    uint64_t acc;
} ThreadCtx;

static void *thread_worker(void *arg) {
    ThreadCtx *ctx = (ThreadCtx *)arg;
    KyIC ic;
    ky_ic_init(&ic);

    /* Each thread has its own IC with local shapes */
    uint32_t base = (uint32_t)ctx->thread_id * 1000;
    for (uint32_t j = 0; j < 20; j++) {
        ky_ic_update(&ic, base + j * 7, (int32_t)(j * 4));
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < ctx->iterations; i++) {
        uint32_t shape = base + (uint32_t)(i % 20) * 7;
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ic, shape);
    }

    ctx->acc = acc;
    ky_ic_destroy(&ic);
    return NULL;
}

static void test_concurrent(void) {
    const int NUM_THREADS = 8;
    pthread_t threads[NUM_THREADS];
    ThreadCtx ctxs[NUM_THREADS];

    for (int t = 0; t < NUM_THREADS; t++) {
        ctxs[t].thread_id = t;
        ctxs[t].iterations = ITERS_PER_CASE / NUM_THREADS;
        ctxs[t].acc = 0;
    }

    uint64_t t0 = ns_now();
    for (int t = 0; t < NUM_THREADS; t++) {
        pthread_create(&threads[t], NULL, thread_worker, &ctxs[t]);
    }
    for (int t = 0; t < NUM_THREADS; t++) {
        pthread_join(threads[t], NULL);
    }
    uint64_t t1 = ns_now();

    uint64_t total_acc = 0;
    for (int t = 0; t < NUM_THREADS; t++) total_acc += ctxs[t].acc;
    g_sink += total_acc;

    double ns_per_op = (double)(t1 - t0) / (double)ITERS_PER_CASE;
    double total_ops = (double)ITERS_PER_CASE;

    printf("  \"concurrent_8threads\": {\n");
    printf("    \"ns_per_op\": %.4f,\n", ns_per_op);
    printf("    \"total_ops\": %.0f,\n", total_ops);
    printf("    \"mops_per_sec\": %.2f\n", total_ops / ((t1 - t0) / 1e9) / 1e6);
    printf("  },\n");
}

/* ---------- 7. Realistic server mix ---------- */

static void bench_realistic_mix(void) {
    /* Distribution of ICs by size:
     *   70% mono (1 shape)
     *   20% poly (4 shapes)
     *   9%  mega (16 shapes)
     *   1%  overflow (100 shapes)
     */
    const int N = 1000;
    KyIC *ics = calloc(N, sizeof(KyIC));
    uint32_t shapes_per_ic[N];
    uint32_t shape_storage[N][100];

    for (int i = 0; i < N; i++) {
        ky_ic_init(&ics[i]);
        uint32_t n;
        uint32_t pct = (uint32_t)(i % 100);
        if (pct < 70)      n = 1;
        else if (pct < 90) n = 4;
        else if (pct < 99) n = 16;
        else               n = 100;

        shapes_per_ic[i] = n;
        for (uint32_t j = 0; j < n; j++) {
            shape_storage[i][j] = (uint32_t)(i * 1000 + j * 13);
            ky_ic_update(&ics[i], shape_storage[i][j], (int32_t)(j * 8));
        }
    }

    uint64_t acc = 0;
    for (uint64_t i = 0; i < WARMUP_ITERS; i++) {
        int idx = (int)(i % N);
        uint32_t n = shapes_per_ic[idx];
        uint32_t shape = shape_storage[idx][i % n];
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ics[idx], shape);
    }

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < ITERS_PER_CASE; i++) {
        int idx = (int)(i % N);
        uint32_t n = shapes_per_ic[idx];
        uint32_t shape = shape_storage[idx][(uint32_t)(i % n)];
        acc += (uint64_t)(uint32_t)ky_ic_lookup_inline(&ics[idx], shape);
    }
    uint64_t t1 = ns_now();

    g_sink += acc;
    for (int i = 0; i < N; i++) ky_ic_destroy(&ics[i]);
    free(ics);

    double ns_per_op = (double)(t1 - t0) / (double)ITERS_PER_CASE;
    printf("  \"realistic_mix\": { \"ns_per_op\": %.4f },\n", ns_per_op);
}

/* ---------- 8. Memory footprint ---------- */

static void test_memory(void) {
    printf("  \"memory\": {\n");
    printf("    \"sizeof_KyIC\": %zu,\n", sizeof(KyIC));
    printf("    \"sizeof_KyICMega\": %zu,\n", sizeof(KyICMega));
    printf("    \"sizeof_KyICOverflow\": %zu,\n", sizeof(KyICOverflow));
    printf("    \"num_ics_small\": %d,\n", NUM_ICS_SMALL);
    printf("    \"num_ics_large\": %d,\n", NUM_ICS_LARGE);

    uint64_t small_footprint = (uint64_t)NUM_ICS_SMALL * sizeof(KyIC);
    uint64_t large_footprint = (uint64_t)NUM_ICS_LARGE * sizeof(KyIC);
    uint64_t overflow_footprint = sizeof(KyICOverflow);

    printf("    \"footprint_bytes_1k\": %llu,\n", (unsigned long long)small_footprint);
    printf("    \"footprint_bytes_100k\": %llu,\n", (unsigned long long)large_footprint);
    printf("    \"overflow_shared_bytes\": %llu\n", (unsigned long long)overflow_footprint);
    printf("  },\n");
}

/* ---------- main ---------- */

int main(void) {
    printf("{\n");
    printf("  \"benchmark\": \"kyjit_ic_server\",\n");
    printf("  \"hardware\": \"%s\",\n", "Intel i5-1240P (12C/16T)");
    printf("  \"iters_per_case\": %llu,\n", (unsigned long long)ITERS_PER_CASE);
    printf("  \"results\": {\n");

    test_scale();
    bench_mono();
    bench_poly4();
    bench_mega16();
    bench_overflow();
    test_concurrent();
    bench_realistic_mix();
    test_memory();

    printf("    \"_end\": true\n");
    printf("  }\n");
    printf("}\n");

    return 0;
}
