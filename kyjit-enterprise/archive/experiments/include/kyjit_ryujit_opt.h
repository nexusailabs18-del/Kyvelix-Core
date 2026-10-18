#ifndef KYJIT_RYUJIT_OPT_H
#define KYJIT_RYUJIT_OPT_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// RyuJIT optimization levels (tiered compilation)
typedef enum {
    KY_TIER0_QUICK = 0,      // quick JIT (no optimization)
    KY_TIER1_OPTIMIZED = 1,  // optimized (aggressive)
    KY_TIER2_SPECIALIZED = 2 // specialized (PGO)
} KyTierLevel;

// Optimization flags (from RyuJIT)
typedef struct {
    uint32_t inline_threshold;      // inline functions smaller than this
    uint32_t unroll_factor;         // loop unrolling
    int aggressive_inline;          // inline even larger
    int vectorize;                  // auto-vectorize
    int branch_optimize;            // reorder branches
    int escape_analyze;             // stack allocation
    int devirtualize;               // remove virtual calls
} KyRyuJITOpt;

static inline KyRyuJITOpt *ky_ryujit_opt_create(KyTierLevel tier) {
    KyRyuJITOpt *opt = calloc(1, sizeof(*opt));
    if (!opt) return NULL;
    switch (tier) {
        case KY_TIER0_QUICK:
            opt->inline_threshold = 16;
            opt->unroll_factor = 2;
            opt->aggressive_inline = 0;
            opt->vectorize = 0;
            opt->branch_optimize = 0;
            opt->escape_analyze = 0;
            opt->devirtualize = 0;
            break;
        case KY_TIER1_OPTIMIZED:
            opt->inline_threshold = 64;
            opt->unroll_factor = 8;
            opt->aggressive_inline = 1;
            opt->vectorize = 1;
            opt->branch_optimize = 1;
            opt->escape_analyze = 1;
            opt->devirtualize = 1;
            break;
        case KY_TIER2_SPECIALIZED:
            opt->inline_threshold = 256;
            opt->unroll_factor = 16;
            opt->aggressive_inline = 1;
            opt->vectorize = 1;
            opt->branch_optimize = 1;
            opt->escape_analyze = 1;
            opt->devirtualize = 1;
            break;
    }
    return opt;
}

// Branch optimization: likely/unlikely hints
#define KY_LIKELY(x)   __builtin_expect(!!(x), 1)
#define KY_UNLIKELY(x) __builtin_expect(!!(x), 0)

// Fast inline for small functions (RyuJIT inlining)
static inline int64_t ky_inline_square(int64_t x) {
    return x * x;
}

// Branch-optimized fib (RyuJIT-style)
static inline int64_t ky_fib_optimized(int64_t n) {
    if (KY_UNLIKELY(n < 2)) return n;
    int64_t a = 0, b = 1;
    // Unrolled Fibonacci (avoids recursion overhead)
    for (int64_t i = 2; i <= n; i++) {
        int64_t tmp = a + b;
        a = b;
        b = tmp;
    }
    return b;
}

// Vectorized sum (RyuJIT auto-vectorization)
static inline int64_t ky_sum_vectorized(const int64_t *data, uint32_t n) {
    int64_t sum = 0;
    uint32_t i = 0;
    // 8-way unrolled (what RyuJIT auto-vectorizes to)
    for (; i + 8 <= n; i += 8) {
        sum += data[i] + data[i+1] + data[i+2] + data[i+3]
             + data[i+4] + data[i+5] + data[i+6] + data[i+7];
    }
    for (; i < n; i++) sum += data[i];
    return sum;
}
#endif
