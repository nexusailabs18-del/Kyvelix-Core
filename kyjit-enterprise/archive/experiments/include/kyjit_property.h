#ifndef KYJIT_PROPERTY_H
#define KYJIT_PROPERTY_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include <stdlib.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include "kyjit_value.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Hash table entry for named properties
typedef struct KyHashEntry {
    uint32_t key_hash;
    KyValue value;
    struct KyHashEntry *next;  // chain for collisions
} KyHashEntry;

// Property storage - split inline/array/hash like LuaJIT GCtab
typedef struct KyPropertyStorage {
    uint32_t inline_count;      // inline properties (fast)
    uint32_t array_count;       // array part for indexed
    uint32_t hash_count;        // hash part for named
    uint32_t hash_size;         // power of 2
    uint32_t hash_mask;         // hash_size - 1
    KyValue inline_props[8];    // inline storage
    KyValue *array_props;       // array part
    KyHashEntry **hash_buckets; // hash table
} KyPropertyStorage;

// Create property storage
static inline KyPropertyStorage *ky_property_create(void) {
    KyPropertyStorage *ps = calloc(1, sizeof(KyPropertyStorage));
    if (!ps) return NULL;
    ps->hash_size = 16;
    ps->hash_mask = 15;
    ps->hash_buckets = calloc(16, sizeof(KyHashEntry *));
    if (!ps->hash_buckets) {
        free(ps);
        return NULL;
    }
    return ps;
}

// Get inline property (direct array access)
static inline KyValue ky_property_get_inline(KyPropertyStorage *ps, uint32_t idx) {
    return ps->inline_props[idx];
}

// Set inline property
static inline void ky_property_set_inline(KyPropertyStorage *ps, uint32_t idx, KyValue val) {
    ps->inline_props[idx] = val;
}

// Get array property (bounds checked)
static inline KyValue ky_property_get_array(KyPropertyStorage *ps, uint32_t idx) {
    if (idx >= ps->array_count) return ky_value_undefined();
    return ps->array_props[idx];
}

// Set array property
static inline void ky_property_set_array(KyPropertyStorage *ps, uint32_t idx, KyValue val) {
    if (idx >= ps->array_count) return;
    ps->array_props[idx] = val;
}

// Hash property lookup
static inline KyValue ky_property_get_hash(KyPropertyStorage *ps, uint32_t key_hash) {
    uint32_t idx = key_hash & ps->hash_mask;
    KyHashEntry *e = ps->hash_buckets[idx];
    while (e) {
        if (e->key_hash == key_hash) return e->value;
        e = e->next;
    }
    return ky_value_undefined();
}

// Hash property set
static inline void ky_property_set_hash(KyPropertyStorage *ps, uint32_t key_hash, KyValue val) {
    uint32_t idx = key_hash & ps->hash_mask;
    KyHashEntry *e = ps->hash_buckets[idx];
    while (e) {
        if (e->key_hash == key_hash) {
            e->value = val;
            return;
        }
        e = e->next;
    }
    // Insert new entry at head
    e = malloc(sizeof(KyHashEntry));
    if (!e) return;
    e->key_hash = key_hash;
    e->value = val;
    e->next = ps->hash_buckets[idx];
    ps->hash_buckets[idx] = e;
    ps->hash_count++;
}

#endif
