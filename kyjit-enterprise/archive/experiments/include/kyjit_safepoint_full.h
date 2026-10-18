#ifndef KYJIT_SAFEPOINT_FULL_H
#define KYJIT_SAFEPOINT_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint32_t pc_offset;
    uint32_t stack_height;
    uint64_t *spill_slots;
    uint32_t spill_count;
} KySafePointFull;

typedef struct {
    KySafePointFull *points;
    uint32_t count;
    uint32_t capacity;
} KySafePointTableFull;

static inline KySafePointTableFull *ky_safepoint_full_create(uint32_t capacity) {
    KySafePointTableFull *t = calloc(1, sizeof(*t));
    if (!t) return NULL;
    t->points = calloc(capacity, sizeof(KySafePointFull));
    if (!t->points) { free(t); return NULL; }
    t->capacity = capacity;
    return t;
}

static inline void ky_safepoint_full_add(KySafePointTableFull *t, uint32_t pc,
                                         uint32_t stack_height,
                                         const uint64_t *spills, uint32_t spill_count) {
    if (!t || t->count >= t->capacity) return;
    KySafePointFull *p = &t->points[t->count];
    p->pc_offset = pc;
    p->stack_height = stack_height;
    p->spill_count = spill_count;
    if (spill_count > 0 && spills) {
        p->spill_slots = malloc(spill_count * sizeof(uint64_t));
        if (p->spill_slots) memcpy(p->spill_slots, spills, spill_count * sizeof(uint64_t));
    }
    t->count++;
}

static inline KySafePointFull *ky_safepoint_full_find(KySafePointTableFull *t, uint32_t pc) {
    if (!t) return NULL;
    for (uint32_t i = 0; i < t->count; i++) {
        if (t->points[i].pc_offset == pc) return &t->points[i];
    }
    return NULL;
}

static inline uint64_t ky_safepoint_full_get_spill(KySafePointTableFull *t, uint32_t pc, uint32_t idx) {
    KySafePointFull *p = ky_safepoint_full_find(t, pc);
    if (!p || idx >= p->spill_count) return 0;
    return p->spill_slots[idx];
}

static inline void ky_safepoint_full_destroy(KySafePointTableFull *t) {
    if (!t) return;
    for (uint32_t i = 0; i < t->count; i++) free(t->points[i].spill_slots);
    free(t->points);
    free(t);
}
#endif
