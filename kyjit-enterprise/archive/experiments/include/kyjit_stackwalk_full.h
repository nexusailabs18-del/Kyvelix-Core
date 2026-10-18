#ifndef KYJIT_STACKWALK_FULL_H
#define KYJIT_STACKWALK_FULL_H
#include <stdint.h>
#include "kyjit_frame_full.h"

typedef struct {
    KyFrameFull *current;
    uint32_t depth;
} KyStackWalkerFull;

static inline void ky_stackwalk_full_init(KyStackWalkerFull *w, KyFrameFull *top) {
    if (!w) return;
    w->current = top;
    w->depth = 0;
}

static inline int ky_stackwalk_full_done(KyStackWalkerFull *w) {
    return !w || w->current == NULL;
}

static inline KyFrameFull *ky_stackwalk_full_next(KyStackWalkerFull *w) {
    if (!w || w->current == NULL) return NULL;
    w->current = w->current->caller;
    w->depth++;
    return w->current;
}

static inline uint32_t ky_stackwalk_full_get_offset(KyStackWalkerFull *w) {
    return w && w->current ? w->current->bytecode_offset : 0;
}

static inline uint32_t ky_stackwalk_full_get_depth(KyStackWalkerFull *w) {
    return w ? w->depth : 0;
}
#endif
