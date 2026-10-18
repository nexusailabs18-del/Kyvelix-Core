#ifndef KYJIT_GENERATOR_H
#define KYJIT_GENERATOR_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Generator states - from QuickJS generator_data
typedef enum {
    KY_GEN_RUNNING = 0,
    KY_GEN_SUSPENDED = 1,
    KY_GEN_COMPLETED = 2
} KyGeneratorState;

// Generator structure
typedef struct KyGenerator {
    KyGeneratorState state;
    KyValue yielded_value;
    KyValue return_value;
    uint32_t resume_pc;    // where to resume execution
    void *stack_frame;     // saved stack frame
} KyGenerator;

// Create generator
static inline KyGenerator *ky_generator_create(void) {
    KyGenerator *g = calloc(1, sizeof(KyGenerator));
    if (!g) return NULL;
    g->state = KY_GEN_RUNNING;
    g->yielded_value = ky_value_undefined();
    g->return_value = ky_value_undefined();
    g->resume_pc = 0;
    return g;
}

// Yield value from generator
static inline KyValue ky_generator_yield(KyGenerator *g, KyValue value, uint32_t next_pc) {
    if (!g) return ky_value_undefined();
    g->state = KY_GEN_SUSPENDED;
    g->yielded_value = value;
    g->resume_pc = next_pc;
    return value;
}

// Resume generator
static inline KyValue ky_generator_resume(KyGenerator *g, KyValue send_value) {
    (void)send_value; // reserved for yield expression result
    if (!g || g->state == KY_GEN_COMPLETED) return ky_value_undefined();
    g->state = KY_GEN_RUNNING;
    // send_value would be passed as yield expression result
    // For now, just return yielded value
    return g->yielded_value;
}

// Complete generator
static inline void ky_generator_complete(KyGenerator *g, KyValue return_value) {
    if (!g) return;
    g->state = KY_GEN_COMPLETED;
    g->return_value = return_value;
}

#endif
