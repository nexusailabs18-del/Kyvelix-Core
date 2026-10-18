#ifndef KYJIT_VALUE_H
#define KYJIT_VALUE_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_object.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_string.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Tagged value representation - stolen from LuaJIT TValue
typedef enum {
    KY_TAG_INT32 = 0,
    KY_TAG_DOUBLE = 1,
    KY_TAG_OBJECT = 2,
    KY_TAG_STRING = 3,
    KY_TAG_NULL = 4,
    KY_TAG_UNDEFINED = 5,
    KY_TAG_BOOL = 6
} KyTag;

typedef struct KyValue {
    union {
        int32_t i32;
        double f64;
        KyjitObject *obj;
        KyString *str;
        uint8_t boolean;
    } as;
    KyTag tag;
} KyValue;

// Inline constructors
static inline KyValue ky_value_int32(int32_t v) {
    KyValue val = { .tag = KY_TAG_INT32 };
    val.as.i32 = v;
    return val;
}

static inline KyValue ky_value_double(double v) {
    KyValue val = { .tag = KY_TAG_DOUBLE };
    val.as.f64 = v;
    return val;
}

static inline KyValue ky_value_object(KyjitObject *v) {
    KyValue val = { .tag = KY_TAG_OBJECT };
    val.as.obj = v;
    return val;
}

static inline KyValue ky_value_string(KyString *v) {
    KyValue val = { .tag = KY_TAG_STRING };
    val.as.str = v;
    return val;
}

static inline KyValue ky_value_null(void) {
    KyValue val = { .tag = KY_TAG_NULL };
    return val;
}

static inline KyValue ky_value_undefined(void) {
    KyValue val = { .tag = KY_TAG_UNDEFINED };
    return val;
}

static inline KyValue ky_value_bool(uint8_t v) {
    KyValue val = { .tag = KY_TAG_BOOL };
    val.as.boolean = v;
    return val;
}

#endif
