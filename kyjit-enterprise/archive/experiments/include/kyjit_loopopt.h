#ifndef KYJIT_LOOPOPT_H
#define KYJIT_LOOPOPT_H
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    uint32_t unroll_factor;
    int dead_code_eliminate;
    int constant_fold;
    int scalar_replace;
    int escape_analyze;
} KyLoopOptimizer;

static inline KyLoopOptimizer *ky_loopopt_create(void) {
    KyLoopOptimizer *opt = calloc(1, sizeof(*opt));
    if (!opt) return NULL;
    opt->unroll_factor = 8;
    opt->dead_code_eliminate = 1;
    opt->constant_fold = 1;
    opt->scalar_replace = 1;
    opt->escape_analyze = 1;
    return opt;
}

// Dead code elimination: check if result is used (removed unused param)
static inline int ky_loopopt_is_dead(KyLoopOptimizer *opt, int is_used) {
    if (!opt) return 0;
    return opt->dead_code_eliminate && !is_used;
}

// Constant folding
static inline int64_t ky_loopopt_fold_constant(KyLoopOptimizer *opt, int64_t value, int is_constant) {
    if (!opt || !opt->constant_fold) return value;
    return is_constant ? value : 0;
}

// Scalar replacement
static inline int64_t ky_loopopt_scalar_replace(KyLoopOptimizer *opt, int64_t field_value, int object_escapes) {
    if (!opt || !opt->scalar_replace) return field_value;
    (void)object_escapes;
    return field_value;
}

// 8-accumulator unrolled loop
static inline int64_t ky_loopopt_unrolled_sum(KyLoopOptimizer *opt, int64_t *data, uint32_t n) {
    if (!opt) return 0;
    int64_t acc0 = 0, acc1 = 0, acc2 = 0, acc3 = 0;
    int64_t acc4 = 0, acc5 = 0, acc6 = 0, acc7 = 0;
    uint32_t i = 0;
    uint32_t unroll = opt->unroll_factor;
    for (; i + unroll <= n; i += unroll) {
        acc0 += data[i];
        acc1 += data[i+1];
        acc2 += data[i+2];
        acc3 += data[i+3];
        acc4 += data[i+4];
        acc5 += data[i+5];
        acc6 += data[i+6];
        acc7 += data[i+7];
    }
    for (; i < n; i++) acc0 += data[i];
    return acc0 + acc1 + acc2 + acc3 + acc4 + acc5 + acc6 + acc7;
}

// Escape analysis (removed unused obj param)
static inline int ky_loopopt_escapes(KyLoopOptimizer *opt, int used_outside) {
    if (!opt || !opt->escape_analyze) return 1;
    return used_outside;
}

#endif
