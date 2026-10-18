#ifndef KYJIT_DESCRIPTOR_H
#define KYJIT_DESCRIPTOR_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Property attributes - stolen from V8 PropertyAttributes
#define KY_PROP_NONE         0x0
#define KY_PROP_READ_ONLY    0x1
#define KY_PROP_DONT_ENUM    0x2
#define KY_PROP_DONT_DELETE  0x4

// Accessor flags
#define KY_PROP_HAS_GETTER   0x8
#define KY_PROP_HAS_SETTER   0x10

typedef struct KyPropertyDescriptor {
    uint32_t flags;
    KyValue value;
    KyValue getter;  // function object if HAS_GETTER
    KyValue setter;  // function object if HAS_SETTER
} KyPropertyDescriptor;

static inline KyPropertyDescriptor ky_desc_data(KyValue value, uint32_t flags) {
    KyPropertyDescriptor desc = {
        .flags = flags,
        .value = value,
        .getter = ky_value_undefined(),
        .setter = ky_value_undefined()
    };
    return desc;
}

static inline KyPropertyDescriptor ky_desc_accessor(KyValue getter, KyValue setter, uint32_t flags) {
    KyPropertyDescriptor desc = {
        .flags = flags | KY_PROP_HAS_GETTER | KY_PROP_HAS_SETTER,
        .value = ky_value_undefined(),
        .getter = getter,
        .setter = setter
    };
    return desc;
}

#endif
