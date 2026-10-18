#ifndef KYJIT_FRAME_FULL_H
#define KYJIT_FRAME_FULL_H
#include <stdint.h>
#include <stdlib.h>

typedef enum { KY_FRAME_JIT=0, KY_FRAME_INTERPRETED=1 } KyFrameTypeFull;

typedef struct KyFrameFull {
    KyFrameTypeFull type;
    uint32_t bytecode_offset;
    uint32_t slot_count;
    struct KyFrameFull *caller;
    void *return_address;
    uint64_t callee_saved[6];   // rbx, r12-r15, rbp
    uint64_t locals[];          // flexible stack slots
} KyFrameFull;

static inline KyFrameFull *ky_frame_full_create(KyFrameTypeFull type,
                                                uint32_t offset,
                                                uint32_t slot_count,
                                                KyFrameFull *caller) {
    KyFrameFull *f = calloc(1, sizeof(KyFrameFull) + slot_count * sizeof(uint64_t));
    if (!f) return NULL;
    f->type = type;
    f->bytecode_offset = offset;
    f->slot_count = slot_count;
    f->caller = caller;
    f->return_address = NULL;
    return f;
}

static inline KyFrameFull *ky_frame_full_get_caller(KyFrameFull *f) {
    return f ? f->caller : NULL;
}

static inline uint32_t ky_frame_full_get_offset(KyFrameFull *f) {
    return f ? f->bytecode_offset : 0;
}

static inline void *ky_frame_full_get_return_address(KyFrameFull *f) {
    return f ? f->return_address : NULL;
}

static inline uint64_t ky_frame_full_get_local(KyFrameFull *f, uint32_t idx) {
    if (!f || idx >= f->slot_count) return 0;
    return f->locals[idx];
}

static inline void ky_frame_full_set_local(KyFrameFull *f, uint32_t idx, uint64_t value) {
    if (!f || idx >= f->slot_count) return;
    f->locals[idx] = value;
}
#endif
