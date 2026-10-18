#ifndef KYJIT_PROMISE_H
#define KYJIT_PROMISE_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Promise states - from QuickJS promise
typedef enum {
    KY_PROMISE_PENDING = 0,
    KY_PROMISE_FULFILLED = 1,
    KY_PROMISE_REJECTED = 2
} KyPromiseState;

// Promise structure
typedef struct KyPromise {
    KyPromiseState state;
    KyValue result;
    KyValue reason;
    void (*on_fulfilled)(KyValue value, void *ctx);
    void (*on_rejected)(KyValue reason, void *ctx);
    void *callback_ctx;
} KyPromise;

// Create promise
static inline KyPromise *ky_promise_create(void) {
    KyPromise *p = calloc(1, sizeof(KyPromise));
    if (!p) return NULL;
    p->state = KY_PROMISE_PENDING;
    p->result = ky_value_undefined();
    p->reason = ky_value_undefined();
    return p;
}

// Resolve promise
static inline void ky_promise_resolve(KyPromise *p, KyValue value) {
    if (!p || p->state != KY_PROMISE_PENDING) return;
    p->state = KY_PROMISE_FULFILLED;
    p->result = value;
    if (p->on_fulfilled) p->on_fulfilled(value, p->callback_ctx);
}

// Reject promise
static inline void ky_promise_reject(KyPromise *p, KyValue reason) {
    if (!p || p->state != KY_PROMISE_PENDING) return;
    p->state = KY_PROMISE_REJECTED;
    p->reason = reason;
    if (p->on_rejected) p->on_rejected(reason, p->callback_ctx);
}

// Then chain
static inline void ky_promise_then(KyPromise *p, 
                                    void (*on_fulfilled)(KyValue, void *),
                                    void (*on_rejected)(KyValue, void *),
                                    void *ctx) {
    if (!p) return;
    p->on_fulfilled = on_fulfilled;
    p->on_rejected = on_rejected;
    p->callback_ctx = ctx;
    
    // If already settled, call immediately
    if (p->state == KY_PROMISE_FULFILLED && on_fulfilled) {
        on_fulfilled(p->result, ctx);
    } else if (p->state == KY_PROMISE_REJECTED && on_rejected) {
        on_rejected(p->reason, ctx);
    }
}

#endif
