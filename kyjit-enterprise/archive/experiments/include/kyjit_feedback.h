#ifndef KYJIT_FEEDBACK_H
#define KYJIT_FEEDBACK_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_type.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Feedback slot kinds - from V8 FeedbackSlotKind
typedef enum {
    KY_FEEDBACK_INVALID = 0,
    KY_FEEDBACK_LOAD_PROPERTY = 1,
    KY_FEEDBACK_STORE_PROPERTY = 2,
    KY_FEEDBACK_LOAD_KEYED = 3,
    KY_FEEDBACK_STORE_KEYED = 4,
    KY_FEEDBACK_CALL = 5,
    KY_FEEDBACK_BINARY_OP = 6,
    KY_FEEDBACK_COMPARE_OP = 7
} KyFeedbackKind;

// Feedback slot - tracks observed types
typedef struct KyFeedbackSlot {
    KyFeedbackKind kind;
    uint32_t observed_types;   // bitmask of KyType
    uint32_t hit_count;
    uint32_t miss_count;
    uint32_t shape_id;         // most common shape
    uint32_t secondary_shape;  // second most common
} KyFeedbackSlot;

// Feedback vector - collection of slots (from V8 FeedbackVector)
typedef struct KyFeedbackVector {
    uint32_t slot_count;
    KyFeedbackSlot *slots;
} KyFeedbackVector;

// Create feedback vector
static inline KyFeedbackVector *ky_feedback_create(uint32_t num_slots) {
    KyFeedbackVector *fv = calloc(1, sizeof(KyFeedbackVector));
    if (!fv) return NULL;
    fv->slot_count = num_slots;
    fv->slots = calloc(num_slots, sizeof(KyFeedbackSlot));
    if (!fv->slots) {
        free(fv);
        return NULL;
    }
    return fv;
}

// Record type observation
static inline void ky_feedback_record(KyFeedbackVector *fv, uint32_t slot_idx, KyType type, uint32_t shape_id) {
    if (!fv || slot_idx >= fv->slot_count) return;
    KyFeedbackSlot *slot = &fv->slots[slot_idx];
    slot->observed_types |= (1u << type);
    if (shape_id) {
        if (slot->shape_id == 0) {
            slot->shape_id = shape_id;
        } else if (slot->shape_id != shape_id && slot->secondary_shape == 0) {
            slot->secondary_shape = shape_id;
        }
    }
    slot->hit_count++;
}

// Get dominant type from feedback
static inline KyType ky_feedback_dominant_type(KyFeedbackVector *fv, uint32_t slot_idx) {
    if (!fv || slot_idx >= fv->slot_count) return KY_TYPE_UNDEFINED;
    KyFeedbackSlot *slot = &fv->slots[slot_idx];
    // Find most significant bit (most common type)
    if (slot->observed_types == 0) return KY_TYPE_UNDEFINED;
    return (KyType)(31 - __builtin_clz(slot->observed_types));
}

// Check if monomorphic (single type observed)
static inline int ky_feedback_is_monomorphic(KyFeedbackVector *fv, uint32_t slot_idx) {
    if (!fv || slot_idx >= fv->slot_count) return 0;
    uint32_t types = fv->slots[slot_idx].observed_types;
    return (types & (types - 1)) == 0 && types != 0;
}

#endif
