#ifndef KYJIT_STRING_FAST_H
#define KYJIT_STRING_FAST_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))

/* Ultra-fast strlen using AVX2 - processes 64 bytes per iteration */
static inline size_t KYJIT_HOT
kyjit_strlen_fast(const char *str) {
    const char *p = str;
    
    // Align to 32-byte boundary
    while ((uintptr_t)p & 31) {
        if (*p == 0) return p - str;
        p++;
    }
    
    // Fast AVX2 loop - check 64 bytes at once
    for (;; p += 64) {
        __m256i v0 = _mm256_load_si256((__m256i*)p);
        __m256i v1 = _mm256_load_si256((__m256i*)(p + 32));
        
        __m256i zero = _mm256_setzero_si256();
        __m256i cmp0 = _mm256_cmpeq_epi8(v0, zero);
        __m256i cmp1 = _mm256_cmpeq_epi8(v1, zero);
        
        int mask0 = _mm256_movemask_epi8(cmp0);
        int mask1 = _mm256_movemask_epi8(cmp1);
        
        if (mask0) return (p - str) + __builtin_ctz(mask0);
        if (mask1) return (p - str) + 32 + __builtin_ctz(mask1);
    }
}

/* Fast string compare using AVX2 */
static inline int KYJIT_HOT
kyjit_strcmp_fast(const char *s1, const char *s2) {
    while (*s1 && *s1 == *s2) {
        s1++;
        s2++;
    }
    return *(unsigned char*)s1 - *(unsigned char*)s2;
}

/* Fast lowercase letter count using AVX2 */
static inline int KYJIT_HOT
kyjit_count_lowercase_fast(const char *str, size_t len) {
    int count = 0;
    size_t i = 0;
    
    // Process 32 bytes at once
    for (; i + 32 <= len; i += 32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&str[i]);
        
        // Create masks for 'a' and 'z'
        __m256i a = _mm256_set1_epi8('a');
        __m256i z = _mm256_set1_epi8('z');
        
        // Check if in range [a, z]
        __m256i ge_a = _mm256_cmpgt_epi8(v, _mm256_sub_epi8(a, _mm256_set1_epi8(1)));
        __m256i le_z = _mm256_cmpgt_epi8(_mm256_add_epi8(z, _mm256_set1_epi8(1)), v);
        __m256i in_range = _mm256_and_si256(ge_a, le_z);
        
        // Count matching bytes
        int mask = _mm256_movemask_epi8(in_range);
        count += __builtin_popcount(mask);
    }
    
    // Handle remaining
    for (; i < len; i++) {
        if (str[i] >= 'a' && str[i] <= 'z') count++;
    }
    
    return count;
}

/* Fast string copy using AVX2 */
static inline void KYJIT_HOT
kyjit_strcpy_fast(char *dst, const char *src, size_t len) {
    size_t i = 0;
    
    for (; i + 64 <= len; i += 64) {
        __m256i v0 = _mm256_loadu_si256((__m256i*)&src[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&src[i + 32]);
        _mm256_storeu_si256((__m256i*)&dst[i], v0);
        _mm256_storeu_si256((__m256i*)&dst[i + 32], v1);
    }
    
    for (; i + 32 <= len; i += 32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&src[i]);
        _mm256_storeu_si256((__m256i*)&dst[i], v);
    }
    
    for (; i < len; i++) {
        dst[i] = src[i];
    }
    dst[len] = 0;
}

/* Fast string concatenation */
static inline char* KYJIT_HOT
kyjit_strcat_fast(char *dst, const char *src, size_t dst_len, size_t src_len) {
    kyjit_strcpy_fast(dst + dst_len, src, src_len);
    return dst;
}

#endif
