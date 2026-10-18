#ifndef KYJIT_FEEDBACK_FULL_H
#define KYJIT_FEEDBACK_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include "kyjit_type.h"

typedef struct {
    uint32_t type_counts[8];
    uint32_t total_samples;
    uint32_t shape_ids[8];
    uint32_t shape_counts[8];
} KyFeedbackSlotFull;

typedef struct {
    uint32_t slot_count;
    KyFeedbackSlotFull *slots;
} KyFeedbackVectorFull;

static inline KyFeedbackVectorFull *ky_feedback_full_create(uint32_t n) {
    KyFeedbackVectorFull *fv = calloc(1, sizeof(*fv));
    if (!fv) return NULL;
    fv->slot_count = n;
    fv->slots = calloc(n, sizeof(KyFeedbackSlotFull));
    if (!fv->slots) { free(fv); return NULL; }
    return fv;
}

static inline void ky_feedback_full_record(KyFeedbackVectorFull *fv, uint32_t slot, KyType type, uint32_t shape_id) {
    if (!fv || slot >= fv->slot_count) return;
    KyFeedbackSlotFull *s = &fv->slots[slot];
    if (type < 8) s->type_counts[type]++;
    s->total_samples++;
    if (shape_id) {
        for (int i=0;i<8;i++) {
            if (s->shape_ids[i] == shape_id) { s->shape_counts[i]++; return; }
            if (s->shape_ids[i] == 0) { s->shape_ids[i] = shape_id; s->shape_counts[i]=1; return; }
        }
    }
}

static inline KyType ky_feedback_full_dominant(KyFeedbackVectorFull *fv, uint32_t slot) {
    if (!fv || slot >= fv->slot_count) return KY_TYPE_UNDEFINED;
    KyFeedbackSlotFull *s = &fv->slots[slot];
    uint32_t max_count = 0;
    KyType best = KY_TYPE_UNDEFINED;
    for (int i=0;i<8;i++) {
        if (s->type_counts[i] > max_count) {
            max_count = s->type_counts[i];
            best = (KyType)i;
        }
    }
    return best;
}

static inline uint32_t ky_feedback_full_top_shape(KyFeedbackVectorFull *fv, uint32_t slot) {
    if (!fv || slot >= fv->slot_count) return 0;
    return fv->slots[slot].shape_ids[0];
}
#endif
