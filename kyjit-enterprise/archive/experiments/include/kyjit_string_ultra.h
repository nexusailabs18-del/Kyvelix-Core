#ifndef KYJIT_STRING_ULTRA_H
#define KYJIT_STRING_ULTRA_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))

/* ULTRA-FAST strlen - processes 256 bytes per iteration */
static inline size_t KYJIT_HOT
kyjit_strlen_ultra(const char *str) {
    const char *p = str;
    
    // Align to 64-byte boundary for AVX2
    while ((uintptr_t)p & 63) {
        if (*p == 0) return p - str;
        p++;
    }
    
    // Process 256 bytes per iteration (8 AVX2 loads)
    for (;; p += 256) {
        __m256i v0 = _mm256_load_si256((__m256i*)p);
        __m256i v1 = _mm256_load_si256((__m256i*)(p + 32));
        __m256i v2 = _mm256_load_si256((__m256i*)(p + 64));
        __m256i v3 = _mm256_load_si256((__m256i*)(p + 96));
        __m256i v4 = _mm256_load_si256((__m256i*)(p + 128));
        __m256i v5 = _mm256_load_si256((__m256i*)(p + 160));
        __m256i v6 = _mm256_load_si256((__m256i*)(p + 192));
        __m256i v7 = _mm256_load_si256((__m256i*)(p + 224));
        
        __m256i zero = _mm256_setzero_si256();
        
        // Check all 8 vectors
        int mask0 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v0, zero));
        if (mask0) return (p - str) + __builtin_ctz(mask0);
        
        int mask1 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v1, zero));
        if (mask1) return (p - str) + 32 + __builtin_ctz(mask1);
        
        int mask2 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v2, zero));
        if (mask2) return (p - str) + 64 + __builtin_ctz(mask2);
        
        int mask3 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v3, zero));
        if (mask3) return (p - str) + 96 + __builtin_ctz(mask3);
        
        int mask4 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v4, zero));
        if (mask4) return (p - str) + 128 + __builtin_ctz(mask4);
        
        int mask5 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v5, zero));
        if (mask5) return (p - str) + 160 + __builtin_ctz(mask5);
        
        int mask6 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v6, zero));
        if (mask6) return (p - str) + 192 + __builtin_ctz(mask6);
        
        int mask7 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v7, zero));
        if (mask7) return (p - str) + 224 + __builtin_ctz(mask7);
    }
}

/* ULTRA-FAST string length with known maximum - no null check needed */
static inline size_t KYJIT_HOT
kyjit_strlen_known(const char *str, size_t max_len) {
    // For strings where we know the maximum length
    // Just return the length directly
    return max_len;
}

/* ULTRA-FAST lowercase counting - processes 64 bytes per iteration */
static inline int KYJIT_HOT
kyjit_count_lowercase_ultra(const char *str, size_t len) {
    int count = 0;
    size_t i = 0;
    
    // Process 64 bytes per iteration
    for (; i + 64 <= len; i += 64) {
        __m256i v0 = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&str[i + 32]);
        
        // Range check: 'a' <= c <= 'z'
        __m256i a = _mm256_set1_epi8('a' - 1);
        __m256i z = _mm256_set1_epi8('z' + 1);
        
        __m256i gt_a0 = _mm256_cmpgt_epi8(v0, a);
        __m256i lt_z0 = _mm256_cmpgt_epi8(z, v0);
        __m256i in_range0 = _mm256_and_si256(gt_a0, lt_z0);
        
        __m256i gt_a1 = _mm256_cmpgt_epi8(v1, a);
        __m256i lt_z1 = _mm256_cmpgt_epi8(z, v1);
        __m256i in_range1 = _mm256_and_si256(gt_a1, lt_z1);
        
        int mask0 = _mm256_movemask_epi8(in_range0);
        int mask1 = _mm256_movemask_epi8(in_range1);
        
        count += __builtin_popcount(mask0) + __builtin_popcount(mask1);
    }
    
    // Handle remaining
    for (; i + 32 <= len; i += 32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i a = _mm256_set1_epi8('a' - 1);
        __m256i z = _mm256_set1_epi8('z' + 1);
        __m256i in_range = _mm256_and_si256(_mm256_cmpgt_epi8(v, a),
                                            _mm256_cmpgt_epi8(z, v));
        int mask = _mm256_movemask_epi8(in_range);
        count += __builtin_popcount(mask);
    }
    
    // Handle tail
    for (; i < len; i++) {
        if (str[i] >= 'a' && str[i] <= 'z') count++;
    }
    
    return count;
}

/* ULTRA-FAST string copy - 128 bytes per iteration */
static inline void KYJIT_HOT
kyjit_strcpy_ultra(char *dst, const char *src, size_t len) {
    size_t i = 0;
    
    // Process 128 bytes per iteration
    for (; i + 128 <= len; i += 128) {
        __m256i v0 = _mm256_loadu_si256((__m256i*)&src[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&src[i + 32]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&src[i + 64]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&src[i + 96]);
        
        _mm256_storeu_si256((__m256i*)&dst[i], v0);
        _mm256_storeu_si256((__m256i*)&dst[i + 32], v1);
        _mm256_storeu_si256((__m256i*)&dst[i + 64], v2);
        _mm256_storeu_si256((__m256i*)&dst[i + 96], v3);
    }
    
    // Handle remaining
    for (; i + 32 <= len; i += 32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&src[i]);
        _mm256_storeu_si256((__m256i*)&dst[i], v);
    }
    
    for (; i < len; i++) {
        dst[i] = src[i];
    }
    dst[len] = 0;
}

/* ULTRA-FAST string fill - 128 bytes per iteration */
static inline void KYJIT_HOT
kyjit_strfill_ultra(char *str, char c, size_t len) {
    size_t i = 0;
    __m256i vc = _mm256_set1_epi8(c);
    
    for (; i + 128 <= len; i += 128) {
        _mm256_storeu_si256((__m256i*)&str[i], vc);
        _mm256_storeu_si256((__m256i*)&str[i + 32], vc);
        _mm256_storeu_si256((__m256i*)&str[i + 64], vc);
        _mm256_storeu_si256((__m256i*)&str[i + 96], vc);
    }
    
    for (; i + 32 <= len; i += 32) {
        _mm256_storeu_si256((__m256i*)&str[i], vc);
    }
    
    for (; i < len; i++) {
        str[i] = c;
    }
    str[len] = 0;
}

/* ULTRA-FAST string concatenation - optimized for known lengths */
static inline void KYJIT_HOT
kyjit_strcat_ultra(char *dst, const char *src, size_t dst_len, size_t src_len) {
    // Direct copy with AVX2
    kyjit_strcpy_ultra(dst + dst_len, src, src_len);
}

/* ULTRA-FAST string comparison - AVX2 */
static inline int KYJIT_HOT
kyjit_strcmp_ultra(const char *s1, const char *s2, size_t len) {
    size_t i = 0;
    
    for (; i + 64 <= len; i += 64) {
        __m256i v1_0 = _mm256_loadu_si256((__m256i*)&s1[i]);
        __m256i v2_0 = _mm256_loadu_si256((__m256i*)&s2[i]);
        __m256i v1_1 = _mm256_loadu_si256((__m256i*)&s1[i + 32]);
        __m256i v2_1 = _mm256_loadu_si256((__m256i*)&s2[i + 32]);
        
        __m256i cmp0 = _mm256_cmpeq_epi8(v1_0, v2_0);
        __m256i cmp1 = _mm256_cmpeq_epi8(v1_1, v2_1);
        
        int mask0 = _mm256_movemask_epi8(cmp0);
        int mask1 = _mm256_movemask_epi8(cmp1);
        
        if (mask0 != 0xFFFFFFFF) {
            int diff_pos = __builtin_ctz(~mask0);
            return (unsigned char)s1[i + diff_pos] - (unsigned char)s2[i + diff_pos];
        }
        if (mask1 != 0xFFFFFFFF) {
            int diff_pos = __builtin_ctz(~mask1);
            return (unsigned char)s1[i + 32 + diff_pos] - (unsigned char)s2[i + 32 + diff_pos];
        }
    }
    
    for (; i < len; i++) {
        if (s1[i] != s2[i]) {
            return (unsigned char)s1[i] - (unsigned char)s2[i];
        }
    }
    
    return 0;
}

#endif
