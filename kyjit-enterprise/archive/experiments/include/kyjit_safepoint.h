#ifndef KYJIT_SAFEPOINT_H
#define KYJIT_SAFEPOINT_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

typedef struct KySafePoint {
    uint32_t pc_offset;
    uint32_t stack_height;
    uint64_t *spill_slots;
    uint32_t spill_count;
} KySafePoint;

typedef struct KySafePointTable {
    KySafePoint *points;
    uint32_t count;
    uint32_t capacity;
} KySafePointTable;

static inline KySafePointTable *ky_safepoint_create(uint32_t capacity) {
    KySafePointTable *t = calloc(1, sizeof(KySafePointTable));
    if (!t) return NULL;
    t->points = calloc(capacity, sizeof(KySafePoint));
    if (!t->points) { free(t); return NULL; }
    t->capacity = capacity;
    return t;
}

static inline void ky_safepoint_add(KySafePointTable *t, uint32_t pc, uint32_t stack_height) {
    if (!t || t->count >= t->capacity) return;
    t->points[t->count].pc_offset = pc;
    t->points[t->count].stack_height = stack_height;
    t->count++;
}

static inline KySafePoint *ky_safepoint_find(KySafePointTable *t, uint32_t pc) {
    if (!t) return NULL;
    for (uint32_t i = 0; i < t->count; i++) {
        if (t->points[i].pc_offset == pc) return &t->points[i];
    }
    return NULL;
}
#endif
