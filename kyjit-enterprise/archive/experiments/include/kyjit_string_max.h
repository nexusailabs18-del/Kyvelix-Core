#ifndef KYJIT_STRING_MAX_H
#define KYJIT_STRING_MAX_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>
#include <stdlib.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))

/* String structure - mutable data pointer */
typedef struct {
    char *data;        // Mutable data pointer
    size_t length;     // Cached length
} KyjitString;

/* O(1) strlen - just a memory read */
#define KYJIT_STRLEN(s) ((s).length)

/* O(1) strcpy - just copy pointer + length */
#define KYJIT_STRCPY(dst, src) do { \
    (dst).data = (src).data; \
    (dst).length = (src).length; \
} while(0)

/* Create string */
#define KYJIT_STRING_CREATE(str, len) \
    (KyjitString){.data = (char*)(str), .length = (len)}

/* Ultra-fast lowercase count - 256 bytes per iteration */
static inline int KYJIT_HOT
kyjit_count_lowercase_max(const KyjitString *s) {
    int count = 0;
    size_t i = 0;
    const char *str = s->data;
    size_t len = s->length;
    
    for (; i + 256 <= len; i += 256) {
        __m256i v0 = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&str[i + 32]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&str[i + 64]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&str[i + 96]);
        __m256i v4 = _mm256_loadu_si256((__m256i*)&str[i + 128]);
        __m256i v5 = _mm256_loadu_si256((__m256i*)&str[i + 160]);
        __m256i v6 = _mm256_loadu_si256((__m256i*)&str[i + 192]);
        __m256i v7 = _mm256_loadu_si256((__m256i*)&str[i + 224]);
        
        __m256i a_minus = _mm256_set1_epi8('a' - 1);
        __m256i z_plus = _mm256_set1_epi8('z' + 1);
        
        __m256i r0 = _mm256_and_si256(_mm256_cmpgt_epi8(v0, a_minus), _mm256_cmpgt_epi8(z_plus, v0));
        __m256i r1 = _mm256_and_si256(_mm256_cmpgt_epi8(v1, a_minus), _mm256_cmpgt_epi8(z_plus, v1));
        __m256i r2 = _mm256_and_si256(_mm256_cmpgt_epi8(v2, a_minus), _mm256_cmpgt_epi8(z_plus, v2));
        __m256i r3 = _mm256_and_si256(_mm256_cmpgt_epi8(v3, a_minus), _mm256_cmpgt_epi8(z_plus, v3));
        __m256i r4 = _mm256_and_si256(_mm256_cmpgt_epi8(v4, a_minus), _mm256_cmpgt_epi8(z_plus, v4));
        __m256i r5 = _mm256_and_si256(_mm256_cmpgt_epi8(v5, a_minus), _mm256_cmpgt_epi8(z_plus, v5));
        __m256i r6 = _mm256_and_si256(_mm256_cmpgt_epi8(v6, a_minus), _mm256_cmpgt_epi8(z_plus, v6));
        __m256i r7 = _mm256_and_si256(_mm256_cmpgt_epi8(v7, a_minus), _mm256_cmpgt_epi8(z_plus, v7));
        
        count += __builtin_popcount(_mm256_movemask_epi8(r0)) +
                 __builtin_popcount(_mm256_movemask_epi8(r1)) +
                 __builtin_popcount(_mm256_movemask_epi8(r2)) +
                 __builtin_popcount(_mm256_movemask_epi8(r3)) +
                 __builtin_popcount(_mm256_movemask_epi8(r4)) +
                 __builtin_popcount(_mm256_movemask_epi8(r5)) +
                 __builtin_popcount(_mm256_movemask_epi8(r6)) +
                 __builtin_popcount(_mm256_movemask_epi8(r7));
    }
    
    for (; i + 32 <= len; i += 32) {
        __m256i v = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i r = _mm256_and_si256(_mm256_cmpgt_epi8(v, _mm256_set1_epi8('a' - 1)),
                                     _mm256_cmpgt_epi8(_mm256_set1_epi8('z' + 1), v));
        count += __builtin_popcount(_mm256_movemask_epi8(r));
    }
    
    for (; i < len; i++) {
        if (str[i] >= 'a' && str[i] <= 'z') count++;
    }
    
    return count;
}

#endif
