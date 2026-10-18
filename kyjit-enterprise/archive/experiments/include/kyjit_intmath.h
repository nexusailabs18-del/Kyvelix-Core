#ifndef KYJIT_INTMATH_H
#define KYJIT_INTMATH_H
#include <stdint.h>

static inline int64_t ky_checked_add(int64_t a, int64_t b, int *overflow) {
    int64_t result;
    if (__builtin_add_overflow(a, b, &result)) { *overflow = 1; return 0; }
    *overflow = 0;
    return result;
}

static inline int64_t ky_checked_sub(int64_t a, int64_t b, int *overflow) {
    int64_t result;
    if (__builtin_sub_overflow(a, b, &result)) { *overflow = 1; return 0; }
    *overflow = 0;
    return result;
}

static inline int64_t ky_checked_mul(int64_t a, int64_t b, int *overflow) {
    int64_t result;
    if (__builtin_mul_overflow(a, b, &result)) { *overflow = 1; return 0; }
    *overflow = 0;
    return result;
}
#endif
