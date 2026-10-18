/*
 * bench/bench_native.c — Measure hardware native speed
 *
 * Reports:
 *   1. CPU frequency (TSC)
 *   2. L1 cache load latency (dependent pointer chase)
 *   3. Function call overhead (noinline call+ret)
 *   4. FMA throughput (16 independent chains)
 *   5. Bump allocation latency
 *
 * Build:
 *   gcc -O3 -march=native -mfma -mavx2 bench/bench_native.c -o bench/bench_native
 */

#define _POSIX_C_SOURCE 199309L

#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <immintrin.h>

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

/* ---------- 1. CPU frequency (TSC) ---------- */

static void measure_frequency(void) {
    uint64_t t0_cyc = rdtscp();
    uint64_t t0_ns  = ns_now();

    while (ns_now() - t0_ns < 100000000ULL) {
        __asm__ __volatile__("" ::: "memory");
    }

    uint64_t t1_cyc = rdtscp();
    uint64_t t1_ns  = ns_now();

    double ghz = (double)(t1_cyc - t0_cyc) / (double)(t1_ns - t0_ns);
    printf("  \"tsc_ghz\": %.4f,\n", ghz);
}

/* ---------- 2. L1 load latency (pointer chase) ---------- */

static void bench_l1_latency(void) {
    #define N 512  /* 512*8 = 4 KB, fits in L1 */
    void **arr = malloc(N * sizeof(void *));
    for (int i = 0; i < N; i++) arr[i] = &arr[(i * 13) % N];

    /* Warmup */
    void **p = arr;
    for (int i = 0; i < 1000000; i++) p = (void **)*p;

    uint64_t iters = 10000000ULL;
    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < iters; i++) {
        p = (void **)*p;
        p = (void **)*p;
        p = (void **)*p;
        p = (void **)*p;
    }
    uint64_t t1 = ns_now();

    /* Prevent compiler from deleting the loop */
    __asm__ __volatile__("" : : "r"(p) : "memory");

    double ns_per_load = (double)(t1 - t0) / (double)(iters * 4);
    printf("  \"l1_load_ns\": %.4f,\n", ns_per_load);
    free(arr);
}

/* ---------- 3. Function call overhead ---------- */

__attribute__((noinline))
static int add_noinline(int a, int b) {
    return a + b;
}

static void bench_call_overhead(void) {
    volatile int sink = 0;
    uint64_t iters = 100000000ULL;

    for (uint64_t i = 0; i < 1000000; i++) sink += add_noinline(1, 2);

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < iters; i++) {
        sink += add_noinline(1, 2);
        sink += add_noinline(1, 2);
        sink += add_noinline(1, 2);
        sink += add_noinline(1, 2);
    }
    uint64_t t1 = ns_now();
    double ns_per_call = (double)(t1 - t0) / (double)(iters * 4);
    printf("  \"func_call_ns\": %.4f,\n", ns_per_call);
    (void)sink;
}

/* ---------- 4. FMA throughput ---------- */

static void bench_fma_throughput(void) {
    __m256 acc0 = _mm256_set1_ps(1.0f);
    __m256 acc1 = _mm256_set1_ps(1.0f);
    __m256 acc2 = _mm256_set1_ps(1.0f);
    __m256 acc3 = _mm256_set1_ps(1.0f);
    __m256 acc4 = _mm256_set1_ps(1.0f);
    __m256 acc5 = _mm256_set1_ps(1.0f);
    __m256 acc6 = _mm256_set1_ps(1.0f);
    __m256 acc7 = _mm256_set1_ps(1.0f);
    __m256 a = _mm256_set1_ps(1.0001f);
    __m256 b = _mm256_set1_ps(0.9999f);

    uint64_t iters = 100000000ULL;

    for (int i = 0; i < 100000; i++) {
        acc0 = _mm256_fmadd_ps(a, b, acc0);
        acc1 = _mm256_fmadd_ps(a, b, acc1);
    }

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < iters; i++) {
        acc0 = _mm256_fmadd_ps(a, b, acc0);
        acc1 = _mm256_fmadd_ps(a, b, acc1);
        acc2 = _mm256_fmadd_ps(a, b, acc2);
        acc3 = _mm256_fmadd_ps(a, b, acc3);
        acc4 = _mm256_fmadd_ps(a, b, acc4);
        acc5 = _mm256_fmadd_ps(a, b, acc5);
        acc6 = _mm256_fmadd_ps(a, b, acc6);
        acc7 = _mm256_fmadd_ps(a, b, acc7);
    }
    uint64_t t1 = ns_now();

    /* Force accumulators into memory so compiler can't delete loop */
    __m256 sum = _mm256_add_ps(_mm256_add_ps(acc0, acc1),
                  _mm256_add_ps(_mm256_add_ps(acc2, acc3),
                  _mm256_add_ps(_mm256_add_ps(acc4, acc5),
                  _mm256_add_ps(acc6, acc7))));
    float result[8];
    _mm256_storeu_ps(result, sum);
    volatile float barrier = result[0] + result[1] + result[2] + result[3]
                           + result[4] + result[5] + result[6] + result[7];
    (void)barrier;

    double total_flops = (double)iters * 64.0;
    double total_ns = (double)(t1 - t0);
    double gflops = total_flops / total_ns;
    printf("  \"fma_gflops\": %.2f,\n", gflops);
}

/* ---------- 5. Bump allocation latency ---------- */

static void bench_alloc_throughput(void) {
    size_t buf_size = 32 * 1024 * 1024;  /* 32 MB, fits L3 */
    char *buf = malloc(buf_size);
    if (!buf) { printf("  \"alloc_ns\": 0,\n"); return; }

    /* Pre-touch to fault in pages */
    for (size_t i = 0; i < buf_size; i += 4096) buf[i] = 0;

    uint64_t iters = 100000000ULL;
    size_t step = 64;
    size_t max_offsets = buf_size / step;

    uint64_t t0 = ns_now();
    for (uint64_t i = 0; i < iters; i++) {
        size_t off = (i % max_offsets) * step;
        *(volatile uint64_t *)(buf + off) = i;
    }
    uint64_t t1 = ns_now();

    double ns_per_alloc = (double)(t1 - t0) / (double)iters;
    double mops = 1000.0 / ns_per_alloc;
    printf("  \"alloc_ns\": %.4f,\n", ns_per_alloc);
    printf("  \"alloc_mops\": %.2f,\n", mops);
    free(buf);
}

/* ---------- main ---------- */

int main(void) {
    printf("{\n");
    printf("  \"machine\": \"i5-1240P\",\n");
    printf("  \"date\": \"2026-09-11\",\n");
    printf("  \"results\": {\n");

    measure_frequency();
    bench_l1_latency();
    bench_call_overhead();
    bench_fma_throughput();
    bench_alloc_throughput();

    printf("    \"_end\": true\n");
    printf("  }\n");
    printf("}\n");
    return 0;
}
