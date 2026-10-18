#ifndef KYJIT_TYPE_H
#define KYJIT_TYPE_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// IR types - from LuaJIT IRType
typedef enum {
    KY_TYPE_NIL = 0,
    KY_TYPE_FALSE = 1,
    KY_TYPE_TRUE = 2,
    KY_TYPE_INT32 = 3,
    KY_TYPE_DOUBLE = 4,
    KY_TYPE_OBJECT = 5,
    KY_TYPE_STRING = 6,
    KY_TYPE_UNDEFINED = 7
} KyType;

// Type guards - from LuaJIT IRT_GUARD
#define KY_GUARD_NONE     0x0
#define KY_GUARD_TYPE     0x1
#define KY_GUARD_SHAPE    0x2
#define KY_GUARD_BOUNDS   0x4
#define KY_GUARD_NONZERO  0x8

// Get type of value
static inline KyType ky_type_of(KyValue v) {
    switch (v.tag) {
        case KY_TAG_INT32:  return KY_TYPE_INT32;
        case KY_TAG_DOUBLE: return KY_TYPE_DOUBLE;
        case KY_TAG_OBJECT: return KY_TYPE_OBJECT;
        case KY_TAG_STRING: return KY_TYPE_STRING;
        case KY_TAG_NULL:   return KY_TYPE_NIL;
        case KY_TAG_UNDEFINED: return KY_TYPE_UNDEFINED;
        case KY_TAG_BOOL:   return v.as.boolean ? KY_TYPE_TRUE : KY_TYPE_FALSE;
        default: return KY_TYPE_UNDEFINED;
    }
}

// Type check with guard
static inline int ky_type_check(KyValue v, KyType expected, uint32_t *guard_flags) {
    KyType actual = ky_type_of(v);
    if (actual != expected) {
        if (guard_flags) *guard_flags |= KY_GUARD_TYPE;
        return 0;
    }
    return 1;
}

#endif
