#ifndef KYJIT_ACCESS_H
#define KYJIT_ACCESS_H

#include <stdint.h>
#include "kyjit_shape.h"
#include "kyjit_ic.h"
#include "kyjit_value.h"
#include "kyjit_property.h"
#include "kyjit_object2.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)

typedef enum {
    KY_LOOKUP_NOT_FOUND = 0,
    KY_LOOKUP_PROPERTY = 1,
    KY_LOOKUP_TRANSITION = 2,
    KY_LOOKUP_ACCESSOR = 3
} KyLookupState;

// Ultra-fast get: IC hit = single load after shape check
static inline KyValue ky_access_get_fast(KyObject2 *obj,
                                          KyShape *shape,
                                          KyIC *ic,
                                          uint32_t key_hash) {
    // Shape check (1 comparison)
    if (unlikely(obj->shape_id != shape->shape_id)) {
        return ky_value_undefined();
    }
    
    // IC hit: check entry 0 first (monomorphic case = 90% of accesses)
    if (likely(ic->entries[0].shape_id == shape->shape_id)) {
        return ky_object2_get_field(obj, ic->entries[0].offset);
    }
    
    // Polymorphic: check entries 1-3
    if (likely(ic->entry_count > 1 && ic->entries[1].shape_id == shape->shape_id)) {
        return ky_object2_get_field(obj, ic->entries[1].offset);
    }
    if (likely(ic->entry_count > 2 && ic->entries[2].shape_id == shape->shape_id)) {
        return ky_object2_get_field(obj, ic->entries[2].offset);
    }
    if (likely(ic->entry_count > 3 && ic->entries[3].shape_id == shape->shape_id)) {
        return ky_object2_get_field(obj, ic->entries[3].offset);
    }
    
    // IC miss: shape lookup + update
    uint32_t offset = ky_shape_get_offset_inline(shape, key_hash);
    if (likely(offset != KY_SHAPE_NOT_FOUND)) {
        ky_ic_update(ic, shape->shape_id, offset);
        return ky_object2_get_field(obj, offset);
    }
    return ky_value_undefined();
}

// Ultra-fast set
static inline int ky_access_set_fast(KyObject2 *obj,
                                      KyShape *shape,
                                      KyIC *ic,
                                      uint32_t key_hash,
                                      KyValue value) {
    if (unlikely(obj->shape_id != shape->shape_id)) return -1;
    
    if (likely(ic->entries[0].shape_id == shape->shape_id)) {
        ky_object2_set_field(obj, ic->entries[0].offset, value);
        return 0;
    }
    if (likely(ic->entry_count > 1 && ic->entries[1].shape_id == shape->shape_id)) {
        ky_object2_set_field(obj, ic->entries[1].offset, value);
        return 0;
    }
    if (likely(ic->entry_count > 2 && ic->entries[2].shape_id == shape->shape_id)) {
        ky_object2_set_field(obj, ic->entries[2].offset, value);
        return 0;
    }
    if (likely(ic->entry_count > 3 && ic->entries[3].shape_id == shape->shape_id)) {
        ky_object2_set_field(obj, ic->entries[3].offset, value);
        return 0;
    }
    
    uint32_t offset = ky_shape_get_offset_inline(shape, key_hash);
    if (likely(offset != KY_SHAPE_NOT_FOUND)) {
        ky_ic_update(ic, shape->shape_id, offset);
        ky_object2_set_field(obj, offset, value);
        return 0;
    }
    return -1;
}

// Typed dispatch
static inline KyValue ky_access_get_typed(KyPropertyStorage *ps, KyValue key) {
    switch (key.tag) {
        case KY_TAG_INT32:
            return ky_property_get_array(ps, (uint32_t)key.as.i32);
        case KY_TAG_STRING:
            if (key.as.str) return ky_property_get_hash(ps, key.as.str->hash);
            return ky_value_undefined();
        default:
            return ky_value_undefined();
    }
}

static inline int ky_access_set_typed(KyPropertyStorage *ps, KyValue key, KyValue val) {
    switch (key.tag) {
        case KY_TAG_INT32:
            ky_property_set_array(ps, (uint32_t)key.as.i32, val);
            return 0;
        case KY_TAG_STRING:
            if (key.as.str) {
                ky_property_set_hash(ps, key.as.str->hash, val);
                return 0;
            }
            return -1;
        default:
            return -1;
    }
}

#endif
