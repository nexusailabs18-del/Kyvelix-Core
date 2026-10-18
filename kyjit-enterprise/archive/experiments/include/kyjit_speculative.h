#ifndef KYJIT_SPECULATIVE_H
#define KYJIT_SPECULATIVE_H

#include <stdint.h>
#include <stdlib.h>
#include "kyjit_type.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

/* Type assumption - from LuaJIT IR guard */
typedef struct KyTypeAssumption {
    KyType expected_type;
    uint32_t expected_shape_id;
    uint32_t guard_flags;
} KyTypeAssumption;

/* Speculative compilation state */
typedef struct KySpeculativeState {
    KyTypeAssumption *assumptions;
    uint32_t assumption_count;
    uint32_t bailout_count;
    uint32_t recompile_count;
} KySpeculativeState;

/* Create speculative state */
KySpeculativeState *ky_speculative_create(uint32_t max_assumptions);

/* Add type assumption */
void ky_speculative_assume(KySpeculativeState *ss, KyType type, uint32_t shape_id, uint32_t guard_flags);

/* Check assumption against runtime value */
int ky_speculative_check(KySpeculativeState *ss, uint32_t idx, KyValue v);

/* Bailout - assumption failed, need deopt */
void ky_speculative_bailout(KySpeculativeState *ss);

/* Branchless speculative selection */
int kyjit_spec_select(int condition, int (*true_fn)(void*), int (*false_fn)(void*), void *arg);

/* Cleanup */
void ky_speculative_destroy(KySpeculativeState *ss);

#endif
