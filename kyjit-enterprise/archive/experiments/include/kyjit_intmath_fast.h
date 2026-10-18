#ifndef KYJIT_INTMATH_FAST_H
#define KYJIT_INTMATH_FAST_H
#include <stdint.h>

// Fast unchecked arithmetic (like HotSpot/RyuJIT int overflow)
static inline int32_t ky_fast_add32(int32_t a, int32_t b) {
    return a + b;  // wraps on overflow, same as Java/C#
}

static inline int32_t ky_fast_mul32(int32_t a, int32_t b) {
    return a * b;  // wraps
}

// Checked arithmetic (correct but slower)
static inline int64_t ky_checked_add64(int64_t a, int64_t b, int *overflow) {
    int64_t result;
    if (__builtin_add_overflow(a, b, &result)) { *overflow = 1; return 0; }
    *overflow = 0;
    return result;
}
#endif
