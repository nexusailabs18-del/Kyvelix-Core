#ifndef KYJIT_CLOSURE_H
#define KYJIT_CLOSURE_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Variable reference - from QuickJS JSVarRef
typedef struct KyVarRef {
    uint8_t is_detached;
    union {
        KyValue value;       // detached: own copy
        struct {
            uint16_t var_idx;   // index in closure's captured vars
            void *stack_frame;  // pointer to stack frame
        };                      // attached: reference to stack
    };
} KyVarRef;

// Closure - function + captured variables
typedef struct KyClosure {
    uint32_t func_id;        // function identifier
    uint32_t var_count;      // number of captured variables
    KyVarRef *captured;      // captured variable references
} KyClosure;

// Create closure
static inline KyClosure *ky_closure_create(uint32_t func_id, uint32_t var_count) {
    KyClosure *c = calloc(1, sizeof(KyClosure));
    if (!c) return NULL;
    c->func_id = func_id;
    c->var_count = var_count;
    c->captured = calloc(var_count, sizeof(KyVarRef));
    if (!c->captured) {
        free(c);
        return NULL;
    }
    return c;
}

// Capture variable by reference (attached)
static inline void ky_closure_capture_ref(KyClosure *c, uint32_t idx, uint16_t var_idx, void *stack_frame) {
    if (idx >= c->var_count) return;
    c->captured[idx].is_detached = 0;
    c->captured[idx].var_idx = var_idx;
    c->captured[idx].stack_frame = stack_frame;
}

// Capture variable by value (detached)
static inline void ky_closure_capture_value(KyClosure *c, uint32_t idx, KyValue value) {
    if (idx >= c->var_count) return;
    c->captured[idx].is_detached = 1;
    c->captured[idx].value = value;
}

// Get captured variable
static inline KyValue ky_closure_get_captured(KyClosure *c, uint32_t idx) {
    if (!c || idx >= c->var_count) return ky_value_undefined();
    KyVarRef *ref = &c->captured[idx];
    if (ref->is_detached) {
        return ref->value;
    }
    // Attached: read from stack frame
    // For now, just return undefined (stack frame access needs frame layout)
    return ky_value_undefined();
}

#endif
