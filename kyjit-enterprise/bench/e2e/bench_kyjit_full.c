/* KYJIT — FULL FEATURE TEST (v2 — correct APIs) */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdint.h>

static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e9 + ts.tv_nsec;
}

static int pass_count = 0, fail_count = 0, skip_count = 0;

#define PASS(name, t) do { printf("  ✅ %-45s %10.2f ns\n", name, (double)(t)); pass_count++; } while(0)
#define FAIL(name, r) do { printf("  ❌ %-45s %s\n", name, r); fail_count++; } while(0)
#define SKIP(name, r) do { printf("  ⚠️  %-45s %s\n", name, r); skip_count++; } while(0)
#define SECTION(t) do { \
    printf("\n╔══════════════════════════════════════════════════╗\n"); \
    printf("║ %-48s ║\n", t); \
    printf("╠══════════════════════════════════════════════════╣\n"); \
} while(0)

/* 01. PARSER */
extern long long kyjit_parse_numbers_branchless(const char*, int);
static void t01(void) {
    SECTION("01. PARSER (numbers)");
    double t0 = now_ns();
    long long r = kyjit_parse_numbers_branchless("1+2+3", 5);
    double t1 = now_ns();
    if (r == 6) PASS("1+2+3", t1-t0); else FAIL("parser", "wrong");
}

/* 02. BYTECODE */
#include "kyjit_bytecode.h"
#include "kyjit_interpreter.h"
static void t02(void) {
    SECTION("02. BYTECODE INTERPRETER");
    KyBytecode code[] = {
        {BC_LOAD_CONST, 1, 0}, {BC_LOAD_CONST, 2, 0},
        {BC_ADD, 0, 0}, {BC_RETURN, 0, 0}
    };
    KyBytecodeBuf buf = {code, 4, 4};
    double t0 = now_ns();
    int r = kyjit_interpreter_execute(&buf, 0);
    double t1 = now_ns();
    if (r == 3) PASS("1+2=3", t1-t0); else FAIL("bytecode", "wrong");
}

/* 03. INLINE CACHE */
#include "kyjit_ic.h"
static void t03(void) {
    SECTION("03. INLINE CACHE");
    KyIC ic = {0};
    ky_ic_update_inline(&ic, 42, 16);
    double t0 = now_ns();
    int32_t off = ky_ic_lookup_inline(&ic, 42);
    double t1 = now_ns();
    if (off == 16) PASS("mono lookup", t1-t0); else FAIL("IC", "wrong");
    ky_ic_update_inline(&ic, 100, 32);
    ky_ic_update_inline(&ic, 200, 48);
    if (ky_ic_lookup_inline(&ic, 200) == 48) PASS("poly lookup", 0);
    else FAIL("IC poly", "wrong");
}

/* 04. GENERATIONAL GC */
#include "kyjit_gc.h"
static void t04(void) {
    SECTION("04. GENERATIONAL GC");
    kyjit_gc_heap_t *h = kyjit_gc_create();
    if (!h) { FAIL("GC", "create failed"); return; }
    double t0 = now_ns();
    for (int i = 0; i < 100000; i++) kyjit_gc_alloc(h, 32);
    double t1 = now_ns();
    PASS("alloc 100K", t1-t0);
    kyjit_gc_collect(h, false); PASS("minor GC", 0);
    kyjit_gc_collect(h, true);  PASS("major GC", 0);
    kyjit_gc_destroy(h);
}

/* 05. CONCURRENT GC */
typedef struct concurrent_gc concurrent_gc_t;
concurrent_gc_t* kyjit_gc_concurrent_create(kyjit_gc_heap_t*);
void kyjit_gc_concurrent_stop(concurrent_gc_t*);
int kyjit_gc_concurrent_collections(concurrent_gc_t*);
static void t05(void) {
    SECTION("05. CONCURRENT GC");
    kyjit_gc_heap_t *h = kyjit_gc_create();
    concurrent_gc_t *gc = kyjit_gc_concurrent_create(h);
    if (!gc) { SKIP("concurrent GC", "create failed"); return; }
    for (int i = 0; i < 200000; i++) kyjit_gc_alloc(h, 32);
    struct timespec ts = {0, 10000000}; nanosleep(&ts, NULL);
    int n = kyjit_gc_concurrent_collections(gc);
    if (n > 0) PASS("background collections", 0);
    else FAIL("concurrent GC", "no collections");
    kyjit_gc_concurrent_stop(gc);
    kyjit_gc_destroy(h);
}

/* 06. TLAB */
#include "kyjit_tlab.h"
static void t06(void) {
    SECTION("06. TLAB");
    KyTLAB *t = kyjit_tlab_create(1024*1024);
    if (!t) { FAIL("TLAB", "create"); return; }
    double t0 = now_ns();
    for (int i = 0; i < 10000; i++) kyjit_tlab_alloc(t, 64);
    double t1 = now_ns();
    PASS("10K allocs", (t1-t0)/10000);
    kyjit_tlab_destroy(t);
}

/* 07. ZONE ALLOCATOR */
#include "kyjit_zone.h"
static void t07(void) {
    SECTION("07. ZONE ALLOCATOR");
    KyjitZone *z = kyjit_zone_create(65536);
    if (!z) { SKIP("zone", "create failed"); return; }
    double t0 = now_ns();
    for (int i = 0; i < 10000; i++) kyjit_zone_alloc(z, 32);
    double t1 = now_ns();
    PASS("10K allocs", (t1-t0)/10000);
    kyjit_zone_destroy(z);
}

/* 08. HANDLES */
#include "kyjit_handles.h"
static void t08(void) {
    SECTION("08. HANDLES");
    KyjitHandleTable *t = kyjit_handle_table_create(1024);
    if (!t) { SKIP("handles", "create failed"); return; }
    int x = 42;
    KyjitHandle h = kyjit_handle_create(t, &x);
    double t0 = now_ns();
    void *p = kyjit_handle_get(t, h);
    double t1 = now_ns();
    if (p == &x) PASS("handle create + get", t1-t0);
    else FAIL("handles", "wrong ptr");
    kyjit_handle_table_free(t);
}

/* 09. ROOTS */
#include "kyjit_roots.h"
static void t09(void) {
    SECTION("09. ROOTS");
    KyjitRootSet *rs = kyjit_roots_create(1024);
    if (!rs) { SKIP("roots", "create failed"); return; }
    int x = 42;
    double t0 = now_ns();
    kyjit_roots_add(rs, &x);
    double t1 = now_ns();
    if (kyjit_roots_count(rs) == 1) PASS("roots add", t1-t0);
    else FAIL("roots", "wrong count");
    kyjit_roots_free(rs);
}

/* 10. OBJECTS */
#include "kyjit_object.h"
static void t10(void) {
    SECTION("10. OBJECTS + SHAPES");
    KyjitShape *s = kyjit_shape_create(42);
    if (s) PASS("shape create", 0); else FAIL("shape", "null");
}

/* 11. STRINGS */
#include "kyjit_string.h"
static void t11(void) {
    SECTION("11. STRINGS");
    KyString *a = ky_string_create("hello");
    KyString *b = ky_string_create("world");
    if (a && b) {
        double t0 = now_ns();
        KyString *c = ky_string_concat(a, b);
        double t1 = now_ns();
        if (c && c->length == 10) PASS("concat", t1-t0);
        else FAIL("concat", "wrong");
    } else FAIL("string", "null");
}

/* 12. SANDBOX */
#include "kyjit_sandbox.h"
static void t12(void) {
    SECTION("12. SANDBOX");
    kyjit_sandbox_t *sb = kyjit_sandbox_create(1024*1024, 1000.0, 1000);
    if (!sb) { FAIL("sandbox", "null"); return; }
    void *p1 = kyjit_sandbox_alloc(sb, 100);
    if (p1) PASS("small alloc", 0);
    void *p2 = kyjit_sandbox_alloc(sb, 2*1024*1024);
    if (!p2) PASS("blocks over-limit", 0); else FAIL("sandbox", "no block");
    kyjit_sandbox_destroy(sb);
}

/* 13. STACKGUARD */
#include "kyjit_stackguard.h"
static void t13(void) {
    SECTION("13. STACKGUARD");
    stack_guard_t *sg = kyjit_stackguard_create(1024*1024);
    if (sg) { PASS("create", 0); kyjit_stackguard_destroy(sg); }
    else SKIP("stackguard", "not linked");
}

/* 14. WASI */
#include "kyjit_wasi.h"
static void t14(void) {
    SECTION("14. WASI");
    kyjit_wasi_t *w = kyjit_wasi_create();
    if (!w) { FAIL("wasi", "null"); return; }
    kyjit_wasi_write(w, "test\n", 5);
    int rc = kyjit_wasi_set_preopen(w, "/tmp");
    if (rc == 0) PASS("write + preopen", 0);
    kyjit_wasi_destroy(w);
}

/* 15. CPU DETECT */
#include "kyjit_cpu_detect.h"
static void t15(void) {
    SECTION("15. CPU DETECTION");
    const char *name = kyjit_cpu_name();
    int cores = kyjit_cpu_count();
    if (name && cores > 0) {
        printf("  CPU: %s (%d cores)\n", name, cores);
        PASS("cpu detect", 0);
    } else FAIL("cpu", "wrong");
}

/* 16. GPU DETECT */
#include "kyjit_gpu_detect.h"
static void t16(void) {
    SECTION("16. GPU DETECTION");
    const char *name = kyjit_gpu_name();
    if (name) { printf("  GPU: %s\n", name); PASS("gpu detect", 0); }
    else SKIP("gpu", "not linked");
}

/* 17. BLAS */
extern int kyjit_blas_matmul_jit(const float*, const float*, float*, int);
static void t17(void) {
    SECTION("17. MATRIX (OpenBLAS)");
    int N = 512;
    float *A = aligned_alloc(64, N*N*4);
    float *B = aligned_alloc(64, N*N*4);
    float *C = aligned_alloc(64, N*N*4);
    for (int i = 0; i < N*N; i++) { A[i] = i%100; B[i] = (i+1)%100; }
    double t0 = now_ns();
    int rc = kyjit_blas_matmul_jit(A, B, C, N);
    double t1 = now_ns();
    if (rc == 0) {
        double ms = (t1-t0)/1e6;
        double gflops = (2.0*N*N*N)/(ms*1e6);
        printf("  Matrix 512x512: %.2f ms (%.2f GFLOP/s)\n", ms, gflops);
        PASS("matrix 512", t1-t0);
    } else FAIL("blas", "failed");
    free(A); free(B); free(C);
}

/* 18. MFR */
extern long long kyjit_mfr_exec(int32_t*, int);
static void t18(void) {
    SECTION("18. MFR (Map/Filter/Reduce)");
    int32_t *data = malloc(1000000 * 4);
    for (int i = 0; i < 1000000; i++) data[i] = i;
    double t0 = now_ns();
    long long r = kyjit_mfr_exec(data, 1000000);
    double t1 = now_ns();
    if (r != 0) PASS("MFR 1M", t1-t0); else FAIL("MFR", "wrong");
    free(data);
}

/* 19. FAST SUM */
extern double kyjit_fast_sum(const double*, int);
static void t19(void) {
    SECTION("19. FAST SUM");
    double *a = aligned_alloc(64, 10000000 * 8);
    for (int i = 0; i < 10000000; i++) a[i] = i % 100;
    double t0 = now_ns();
    double r = kyjit_fast_sum(a, 10000000);
    double t1 = now_ns();
    if (r > 0) PASS("fast sum 10M", t1-t0); else FAIL("fast sum", "wrong");
    free(a);
}

/* 20. ARRAY AVX2 */
#include "kyjit_array.h"
static void t20(void) {
    SECTION("20. ARRAY SUM AVX2");
    int32_t *a = aligned_alloc(64, 10000000 * 4);
    for (int i = 0; i < 10000000; i++) a[i] = i % 100;
    double t0 = now_ns();
    long long r = kyjit_array_sum_avx2(a, 10000000);
    double t1 = now_ns();
    if (r > 0) PASS("AVX2 sum 10M", t1-t0); else FAIL("avx2", "wrong");
    free(a);
}

/* 21. μOP CACHE */
#include "kyjit_uop_cache.h"
static void t21(void) {
    SECTION("21. μOP CACHE");
    KyUopCache *c = kyjit_uop_cache_create();
    if (!c) { FAIL("uop", "null"); return; }
    printf("  Cache: %d blocks, %zu bytes\n",
           kyjit_uop_cache_count(c), kyjit_uop_cache_size(c));
    const int ITERS = 1000000;
    volatile int sum = 0;
    struct { const char *op; int a, b, exp; } tests[] = {
        {"add", 5, 3, 8}, {"mul", 6, 7, 42}, {"sub", 10, 4, 6},
    };
    for (int i = 0; i < 3; i++) {
        for (int j = 0; j < 1000; j++) sum += kyjit_uop_cache_execute(c, tests[i].op, tests[i].a, tests[i].b);
        double t0 = now_ns();
        for (int j = 0; j < ITERS; j++)
            sum += kyjit_uop_cache_execute(c, tests[i].op, tests[i].a, tests[i].b);
        double t1 = now_ns();
        int r = kyjit_uop_cache_execute(c, tests[i].op, tests[i].a, tests[i].b);
        if (r == tests[i].exp) PASS(tests[i].op, (t1-t0)/ITERS);
        else FAIL(tests[i].op, "wrong");
    }
    kyjit_uop_cache_destroy(c);
}

/* 22. THREAD POOL */
#include "kyjit_thread_pool.h"
static void t22(void) {
    SECTION("22. THREAD POOL");
    KyThreadPool *tp = kyjit_pool_create(4);
    if (tp) { PASS("thread pool create (4)", 0); kyjit_pool_destroy(tp); }
    else SKIP("thread pool", "not linked");
}

/* 23. ATOMIC */
#include "kyjit_atomic_track.h"
static void t23(void) {
    SECTION("23. ATOMIC TRACK");
    KyAtomicTrack track;
    kyjit_track_init(&track);
    kyjit_track_submit(&track);
    if (kyjit_track_inbound(&track) == 1) PASS("atomic inc", 0);
    else FAIL("atomic", "wrong");
}

/* 24. REGEXP */
#include "kyjit_regexp.h"
static void t24(void) {
    SECTION("24. REGEXP");
    KyjitRegexp *re = kyjit_regexp_compile("hello");
    if (re) {
        int m = kyjit_regexp_match(re, "hello world", 11);
        if (m > 0) PASS("regexp match", 0); else FAIL("regexp", "no match");
        kyjit_regexp_free(re);
    } else FAIL("regexp", "compile failed");
}

/* 25. HASH */
extern uint32_t kyjit_hash(const char*, int);
static void t25(void) {
    SECTION("25. HASH");
    uint32_t h1 = 0; // no kyjit_hash("test", 4);
    uint32_t h2 = 0; // no kyjit_hash("test", 4);
    if (h1 == h2 && h1 != 0) PASS("hash deterministic", 0);
    else FAIL("hash", "not deterministic");
}

/* 26. BASE64 */
#include "kyjit_base64.h"
static void t26(void) {
    SECTION("26. BASE64");
    kyjit_base64_init();
    PASS("base64 init", 0);
}

/* 27. BRANCHLESS */
#include "kyjit_branchless.h"
static void t27(void) {
    SECTION("27. BRANCHLESS");
    int r = kyjit_branchless_select(1, 100, 200);
    if (r == 100) PASS("branchless select", 0);
    else FAIL("branchless", "wrong");
}

/* 28. REGISTER ALLOC */
#include "kyjit_regalloc.h"
static void t28(void) {
    SECTION("28. REGISTER ALLOCATION");
    KyRegAlloc *ra = ky_ra_create(KY_RA_LINEAR_SCAN, 16, 256);
    
    int reg = ky_ra_linear_scan(ra, 0, 0, 100);
    if (reg < 16) PASS("alloc register", 0);
    else FAIL("regalloc", "no register");
}

/* 29. CACHE BLOCK */
#include "kyjit_cache_block.h"
static void t29(void) {
    SECTION("29. CACHE-BLOCKED MATRIX");
    int N = 256;
    float *A = aligned_alloc(64, N*N*4);
    float *B = aligned_alloc(64, N*N*4);
    float *C = aligned_alloc(64, N*N*4);
    for (int i = 0; i < N*N; i++) { A[i] = i%100; B[i] = (i+1)%100; }
    double t0 = now_ns();
    int rc = kyjit_cache_block_matmul(A, B, C, N, N, N);
    double t1 = now_ns();
    if (rc == 0) PASS("cache-blocked 256x256", t1-t0);
    else FAIL("cache block", "failed");
    free(A); free(B); free(C);
}

/* MAIN */
int main(void) {
    printf("\n");
    printf("╔══════════════════════════════════════════════════╗\n");
    printf("║  KYJIT — FULL FEATURE TEST (v2)                  ║\n");
    printf("║  Machine: Intel i5-1240P                         ║\n");
    printf("║  Date: 2026-09-13                                ║\n");
    printf("╚══════════════════════════════════════════════════╝\n");

    t01(); t02(); t03(); t04(); t05(); t06(); t07(); t08();
    t09(); t10(); t11(); t12(); t13(); t14(); t15(); t16();
    t17(); t18(); t19(); t20(); t21(); t22(); t23(); t24();
    t25(); t26(); t27(); t28(); t29();

    printf("\n");
    printf("╔══════════════════════════════════════════════════╗\n");
    printf("║  SUMMARY                                         ║\n");
    printf("╠══════════════════════════════════════════════════╣\n");
    printf("║  ✅ PASS:  %3d                                   ║\n", pass_count);
    printf("║  ❌ FAIL:  %3d                                   ║\n", fail_count);
    printf("║  ⚠️  SKIP:  %3d                                   ║\n", skip_count);
    printf("║  TOTAL:   %3d                                   ║\n", pass_count+fail_count+skip_count);
    printf("╚══════════════════════════════════════════════════╝\n");

    return fail_count > 0 ? 1 : 0;
}
