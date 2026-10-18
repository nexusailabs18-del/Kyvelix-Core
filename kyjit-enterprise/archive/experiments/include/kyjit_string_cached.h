#ifndef KYJIT_STRING_CACHED_H
#define KYJIT_STRING_CACHED_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>
#include <stdlib.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))

/* String structure with cached length - like V8 */
typedef struct {
    char *data;
    size_t length;  // Cached length!
    size_t capacity;
} KyjitString;

/* Create string with cached length - O(1) */
static inline KyjitString KYJIT_HOT
kyjit_string_create(const char *str) {
    KyjitString s;
    s.length = strlen(str);  // Only called once at creation
    s.capacity = s.length + 1;
    s.data = malloc(s.capacity);
    memcpy(s.data, str, s.length + 1);
    return s;
}

/* Get length - O(1) like V8! */
static inline size_t KYJIT_HOT
kyjit_string_length(const KyjitString *s) {
    return s->length;  // Just return cached value!
}

/* Ultra-fast lowercase count using cached length */
static inline int KYJIT_HOT
kyjit_string_count_lowercase(const KyjitString *s) {
    int count = 0;
    size_t i = 0;
    size_t len = s->length;
    const char *str = s->data;
    
    // Process 128 bytes per iteration
    for (; i + 128 <= len; i += 128) {
        __m256i v0 = _mm256_loadu_si256((__m256i*)&str[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&str[i + 32]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&str[i + 64]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&str[i + 96]);
        
        __m256i a_minus = _mm256_set1_epi8('a' - 1);
        __m256i z_plus = _mm256_set1_epi8('z' + 1);
        
        __m256i r0 = _mm256_and_si256(_mm256_cmpgt_epi8(v0, a_minus), _mm256_cmpgt_epi8(z_plus, v0));
        __m256i r1 = _mm256_and_si256(_mm256_cmpgt_epi8(v1, a_minus), _mm256_cmpgt_epi8(z_plus, v1));
        __m256i r2 = _mm256_and_si256(_mm256_cmpgt_epi8(v2, a_minus), _mm256_cmpgt_epi8(z_plus, v2));
        __m256i r3 = _mm256_and_si256(_mm256_cmpgt_epi8(v3, a_minus), _mm256_cmpgt_epi8(z_plus, v3));
        
        count += __builtin_popcount(_mm256_movemask_epi8(r0)) +
                 __builtin_popcount(_mm256_movemask_epi8(r1)) +
                 __builtin_popcount(_mm256_movemask_epi8(r2)) +
                 __builtin_popcount(_mm256_movemask_epi8(r3));
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

/* Ultra-fast string copy with cached length */
static inline void KYJIT_HOT
kyjit_string_copy(KyjitString *dst, const KyjitString *src) {
    dst->length = src->length;
    dst->capacity = src->capacity;
    dst->data = malloc(src->capacity);
    
    size_t i = 0;
    for (; i + 128 <= src->length; i += 128) {
        __m256i v0 = _mm256_loadu_si256((__m256i*)&src->data[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&src->data[i + 32]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&src->data[i + 64]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&src->data[i + 96]);
        
        _mm256_storeu_si256((__m256i*)&dst->data[i], v0);
        _mm256_storeu_si256((__m256i*)&dst->data[i + 32], v1);
        _mm256_storeu_si256((__m256i*)&dst->data[i + 64], v2);
        _mm256_storeu_si256((__m256i*)&dst->data[i + 96], v3);
    }
    
    for (; i < src->length; i++) {
        dst->data[i] = src->data[i];
    }
    dst->data[src->length] = 0;
}

/* Free string */
static inline void KYJIT_HOT
kyjit_string_free(KyjitString *s) {
    free(s->data);
    s->data = NULL;
    s->length = 0;
    s->capacity = 0;
}

#endif
