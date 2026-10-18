#ifndef KYJIT_EXCEPTION_H
#define KYJIT_EXCEPTION_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Exception types - from QuickJS error classes
typedef enum {
    KY_EXC_NONE = 0,
    KY_EXC_ERROR = 1,
    KY_EXC_RANGE_ERROR = 2,
    KY_EXC_REFERENCE_ERROR = 3,
    KY_EXC_SYNTAX_ERROR = 4,
    KY_EXC_TYPE_ERROR = 5
} KyExceptionType;

// Exception context - from QuickJS current_exception
typedef struct KyExceptionContext {
    KyExceptionType type;
    KyValue exception;
    uint8_t is_uncatchable;
} KyExceptionContext;

// Global exception context (thread-local later)
static KyExceptionContext g_exception_ctx = { .type = KY_EXC_NONE, .is_uncatchable = 0 };

// Throw exception
static inline void ky_throw(KyExceptionType type, KyValue value) {
    g_exception_ctx.type = type;
    g_exception_ctx.exception = value;
    g_exception_ctx.is_uncatchable = 0;
}

// Throw uncatchable exception (OOM, etc.)
static inline void ky_throw_uncatchable(KyExceptionType type, KyValue value) {
    g_exception_ctx.type = type;
    g_exception_ctx.exception = value;
    g_exception_ctx.is_uncatchable = 1;
}

// Check if exception pending
static inline int ky_has_exception(void) {
    return g_exception_ctx.type != KY_EXC_NONE;
}

// Clear exception
static inline void ky_clear_exception(void) {
    g_exception_ctx.type = KY_EXC_NONE;
    g_exception_ctx.is_uncatchable = 0;
    g_exception_ctx.exception = ky_value_undefined();
}

// Try-catch helper: returns 0 if no exception, 1 if exception caught
static inline int ky_try(int (*func)(void *), void *arg) {
    ky_clear_exception();
    func(arg);
    return ky_has_exception() ? 1 : 0;
}

#endif
