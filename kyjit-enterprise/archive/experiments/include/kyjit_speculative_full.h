#ifndef KYJIT_SPECULATIVE_FULL_H
#define KYJIT_SPECULATIVE_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include "kyjit_type.h"
#include "kyjit_value.h"

typedef struct {
    KyType expected_type;
    uint32_t shape_id;
    uint32_t guard_flags;
} KySpecAssumption;

typedef struct {
    KySpecAssumption *assumptions;
    uint32_t count;
    void *compiled_code;
    uint32_t version;
    uint32_t deopt_count;
} KySpeculativeCode;

static inline KySpeculativeCode *ky_spec_full_create(uint32_t max_assumptions) {
    KySpeculativeCode *sc = calloc(1, sizeof(*sc));
    if (!sc) return NULL;
    sc->assumptions = calloc(max_assumptions, sizeof(KySpecAssumption));
    return sc;
}

static inline void ky_spec_full_add_assumption(KySpeculativeCode *sc, KyType type, uint32_t shape_id, uint32_t flags) {
    if (!sc) return;
    sc->assumptions[sc->count].expected_type = type;
    sc->assumptions[sc->count].shape_id = shape_id;
    sc->assumptions[sc->count].guard_flags = flags;
    sc->count++;
}

static inline int ky_spec_full_check(KySpeculativeCode *sc, uint32_t idx, KyValue v) {
    if (!sc || idx >= sc->count) return 0;
    KySpecAssumption *a = &sc->assumptions[idx];
    if (ky_type_of(v) != a->expected_type) return 0;
    // Only access as.obj when expected type is OBJECT (avoids invalid union read)
    if (a->expected_type == KY_TYPE_OBJECT && a->shape_id) {
        if (v.as.obj == NULL) return 0;
        if (v.as.obj->shape_id != a->shape_id) return 0;
    }
    return 1;
}
#endif
