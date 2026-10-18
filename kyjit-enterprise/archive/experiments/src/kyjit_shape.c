#include "kyjit_shape.h"
#include <stdlib.h>
#include <string.h>

static _Atomic uint32_t next_shape_id = 1;

KyShape *ky_shape_create(KyShape *parent) {
    KyShape *shape = (KyShape *)calloc(1, sizeof(KyShape));
    if (!shape) return NULL;
    shape->shape_id = atomic_fetch_add_explicit(&next_shape_id, 1,
                                                memory_order_relaxed);
    shape->parent = parent;
    shape->property_count = 0;
    shape->transition_count = 0;
    return shape;
}

KyShape *ky_shape_add_property(KyShape *shape, uint32_t key_hash, uint32_t *offset) {
    if (!shape || shape->property_count >= 32) return NULL;

    // Check transition cache first
    for (uint32_t i = 0; i < shape->transition_count; i++) {
        if (shape->transitions[i].key_hash == key_hash) {
            uintptr_t target = atomic_load_explicit(&shape->transitions[i].target,
                                                    memory_order_acquire);
            if (target) {
                KyShape *child = (KyShape *)target;
                if (offset) {
                    *offset = ky_shape_get_offset_inline(child, key_hash);
                }
                return child;
            }
        }
    }

    // Create child shape
    KyShape *child = ky_shape_create(shape);
    if (!child) return NULL;

    // Copy parent's properties
    if (shape->property_count > 0) {
        memcpy(child->properties, shape->properties,
               shape->property_count * sizeof(shape->properties[0]));
    }
    child->property_count = shape->property_count;

    // Add new property (offset starts at 8 to skip header)
    uint32_t prop_offset = 8 + child->property_count * sizeof(int64_t);
    child->properties[child->property_count].key_hash = key_hash;
    child->properties[child->property_count].offset = prop_offset;
    child->property_count++;

    // Record transition in parent (if room)
    if (shape->transition_count < 16) {
        uint32_t idx = shape->transition_count++;
        shape->transitions[idx].key_hash = key_hash;
        atomic_store_explicit(&shape->transitions[idx].target,
                              (uintptr_t)child, memory_order_release);
    }

    if (offset) *offset = prop_offset;
    return child;
}

// Fallback function (calls inline version)
uint32_t ky_shape_get_offset(KyShape *shape, uint32_t key_hash) {
    return ky_shape_get_offset_inline(shape, key_hash);
}
