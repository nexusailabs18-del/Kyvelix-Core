#ifndef KYJIT_METRICS_H
#define KYJIT_METRICS_H
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

typedef struct {
    uint64_t compile_time_ns;
    uint64_t jit_time_ns;
    uint64_t deopt_count;
    uint64_t ic_hits;
    uint64_t ic_misses;
    uint64_t code_bytes;
    uint32_t compiled_functions;
} KyJITMetrics;

static inline KyJITMetrics *ky_metrics_create(void) {
    return calloc(1, sizeof(KyJITMetrics));
}

static inline void ky_metrics_record_compile(KyJITMetrics *m, uint64_t time_ns, uint64_t code_bytes) {
    if (!m) return;
    m->compile_time_ns += time_ns;
    m->code_bytes += code_bytes;
    m->compiled_functions++;
}

static inline void ky_metrics_record_ic_hit(KyJITMetrics *m) {
    if (m) m->ic_hits++;
}

static inline void ky_metrics_record_ic_miss(KyJITMetrics *m) {
    if (m) m->ic_misses++;
}

static inline void ky_metrics_print(KyJITMetrics *m) {
    if (!m) return;
    printf("=== JIT Metrics ===\n");
    printf("Compiled functions: %u\n", m->compiled_functions);
    printf("Code bytes: %lu\n", (unsigned long)m->code_bytes);
    printf("Compile time: %lu ns\n", (unsigned long)m->compile_time_ns);
    printf("Deopts: %lu\n", (unsigned long)m->deopt_count);
    printf("IC hits: %lu\n", (unsigned long)m->ic_hits);
    printf("IC misses: %lu\n", (unsigned long)m->ic_misses);
    printf("===================\n");
}
#endif
