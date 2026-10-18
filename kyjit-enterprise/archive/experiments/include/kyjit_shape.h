#ifndef KYJIT_SHAPE_H
#define KYJIT_SHAPE_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include <immintrin.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

#ifdef __cplusplus
#include <atomic>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#define _Atomic(T) std::atomic<T>
#else
#include <stdatomic.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#endif

typedef struct KyShape KyShape;

struct KyShape {
    uint32_t shape_id;
    uint32_t property_count;
    KyShape *parent;
    
    struct {
        uint32_t key_hash;
        uint32_t offset;
    } properties[32];
    
    struct {
        uint32_t key_hash;
        _Atomic(uintptr_t) target;
    } transitions[16];
    
    uint32_t transition_count;
};

#define KY_SHAPE_NOT_FOUND 0xFFFFFFFFu

// Ultra-fast branchless SIMD lookup
static inline uint32_t ky_shape_get_offset_inline(KyShape *shape, uint32_t key_hash) {
    if (!shape || shape->property_count == 0) return KY_SHAPE_NOT_FOUND;
    
    uint32_t count = shape->property_count;
    
    // Load first 4 key_hashes (they are at even 32-bit offsets: 0, 2, 4, 6)
    // Properties struct layout: [key_hash][offset][key_hash][offset]...
    __m256i keys = _mm256_loadu_si256((const __m256i *)shape->properties);
    
    // Extract just key_hashes (even 32-bit lanes)
    // We need to compare only lanes 0, 2, 4, 6
    __m256i target = _mm256_set1_epi32((int)key_hash);
    __m256i cmp = _mm256_cmpeq_epi32(keys, target);
    
    // Mask of matching lanes (only even lanes can match since offsets are in odd lanes)
    int mask = _mm256_movemask_ps((__m256)cmp);
    
    // Check even lanes only (bits 0, 2, 4, 6)
    int even_mask = mask & 0x55; // 01010101 in binary
    
    if (even_mask && count > 0) {
        // Convert bit position to property index
        // Bit 0 -> prop 0, bit 2 -> prop 1, bit 4 -> prop 2, bit 6 -> prop 3
        int bit_idx = __builtin_ctz(even_mask);
        int prop_idx = bit_idx / 2;
        
        // Now extract offset from that property
        if (prop_idx < 4) {
            return shape->properties[prop_idx].offset;
        }
    }
    
    // Fallback for >4 properties or no match
    for (uint32_t i = 4; i < count; i++) {
        if (shape->properties[i].key_hash == key_hash) {
            return shape->properties[i].offset;
        }
    }
    return KY_SHAPE_NOT_FOUND;
}

KyShape *ky_shape_create(KyShape *parent);
KyShape *ky_shape_add_property(KyShape *shape, uint32_t key_hash, uint32_t *offset);
uint32_t ky_shape_get_offset(KyShape *shape, uint32_t key_hash);

#ifdef __cplusplus
#undef _Atomic
#endif

#endif
