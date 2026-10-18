#ifndef KYJIT_WARMUP_H
#define KYJIT_WARMUP_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint32_t function_id;
    uint32_t call_count;
    uint32_t hot_threshold;
    void *compiled_code;
} KyWarmupEntry;

typedef struct {
    KyWarmupEntry *entries;
    uint32_t count;
} KyWarmup;

static inline KyWarmup *ky_warmup_create(uint32_t max_entries) {
    KyWarmup *w = calloc(1, sizeof(*w));
    if (!w) return NULL;
    w->entries = calloc(max_entries, sizeof(KyWarmupEntry));
    return w;
}

static inline void ky_warmup_register(KyWarmup *w, uint32_t func_id, uint32_t threshold) {
    if (!w) return;
    w->entries[w->count].function_id = func_id;
    w->entries[w->count].hot_threshold = threshold;
    w->count++;
}

static inline int ky_warmup_record_call(KyWarmup *w, uint32_t func_id) {
    if (!w) return 0;
    for (uint32_t i = 0; i < w->count; i++) {
        if (w->entries[i].function_id == func_id) {
            w->entries[i].call_count++;
            return w->entries[i].call_count >= w->entries[i].hot_threshold;
        }
    }
    return 0;
}
#endif
