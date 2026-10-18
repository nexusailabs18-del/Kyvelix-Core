#ifndef KYJIT_FRAME_H
#define KYJIT_FRAME_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Unified frame - works for both interpreter and JIT
typedef enum {
    KY_FRAME_INTERPRETED = 0,
    KY_FRAME_JIT = 1
} KyFrameType;

typedef struct KyFrame {
    KyFrameType type;
    uint32_t bytecode_offset;
    uint32_t stack_height;
    struct KyFrame *caller;
    void *return_address;
    uint64_t saved_registers[8];  // callee-saved
} KyFrame;

// Stack walker
typedef struct KyStackWalker {
    KyFrame *current;
    uint32_t depth;
} KyStackWalker;

static inline void ky_frame_init(KyFrame *f, KyFrameType type, uint32_t offset, KyFrame *caller) {
    f->type = type;
    f->bytecode_offset = offset;
    f->stack_height = 0;
    f->caller = caller;
    f->return_address = NULL;
}

static inline KyFrame *ky_frame_next(KyFrame *f) {
    return f ? f->caller : NULL;
}

static inline uint32_t ky_frame_offset(KyFrame *f) {
    return f ? f->bytecode_offset : 0;
}
#endif
