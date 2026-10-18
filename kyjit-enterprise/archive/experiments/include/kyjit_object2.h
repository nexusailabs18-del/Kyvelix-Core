#ifndef KYJIT_OBJECT2_H
#define KYJIT_OBJECT2_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_shape.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Object with tagged values - Phase 2 upgrade from int64_t properties
typedef struct KyObject2 {
    uint32_t shape_id;
    uint32_t flags;
    KyObject *prototype;  // prototype chain link
    KyValue properties[];  // tagged values instead of int64_t
} KyObject2;

// Create object with prototype
static inline KyObject2 *ky_object2_create(KyShape *shape, KyObject *proto) {
    size_t size = sizeof(KyObject2) + shape->property_count * sizeof(KyValue);
    KyObject2 *obj = (KyObject2 *)calloc(1, size);
    if (!obj) return NULL;
    obj->shape_id = shape->shape_id;
    obj->flags = 0;
    obj->prototype = proto;
    return obj;
}

// Inline property access - direct offset load/store
static inline KyValue ky_object2_get_field(KyObject2 *obj, uint32_t offset) {
    return *(KyValue *)((char *)obj + offset);
}

static inline void ky_object2_set_field(KyObject2 *obj, uint32_t offset, KyValue value) {
    *(KyValue *)((char *)obj + offset) = value;
}

// Property access with prototype chain traversal
static inline KyValue ky_object2_get_property(KyObject2 *obj, KyShape *shape, uint32_t key_hash) {
    uint32_t offset = ky_shape_get_offset_inline(shape, key_hash);
    if (offset != KY_SHAPE_NOT_FOUND) {
        return ky_object2_get_field(obj, offset);
    }
    // Walk prototype chain
    if (obj->prototype) {
        // TODO: Need prototype's shape too
        // For now, return undefined
    }
    return ky_value_undefined();
}

#endif
