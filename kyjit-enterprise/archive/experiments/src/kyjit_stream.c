#include <immintrin.h>
#include <string.h>
#include "kyjit_stream.h"

/* OPTIMIZED non-temporal streaming
 * - 4 simultaneous stream stores (ILP)
 * - Cache-line aligned (64 bytes)
 * - No cache pollution
 */
void kyjit_stream_copy_optimized(const int32_t *restrict src, 
                                  int32_t *restrict dst, int n) {
    int i = 0;
    
    /* Process 64 elements = 256 bytes = 4 cache lines */
    for(; i+64 <= n; i+=64) {
        /* Prefetch NEXT iteration's source (not cache-polluting) */
        __builtin_prefetch(&src[i+128], 0, 0);
        
        /* Load 4 vectors */
        __m256i v0 = _mm256_loadu_si256((__m256i*)&src[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&src[i+8]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&src[i+16]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&src[i+24]);
        __m256i v4 = _mm256_loadu_si256((__m256i*)&src[i+32]);
        __m256i v5 = _mm256_loadu_si256((__m256i*)&src[i+40]);
        __m256i v6 = _mm256_loadu_si256((__m256i*)&src[i+48]);
        __m256i v7 = _mm256_loadu_si256((__m256i*)&src[i+56]);
        
        /* Stream store — bypass cache entirely */
        _mm256_stream_si256((__m256i*)&dst[i], v0);
        _mm256_stream_si256((__m256i*)&dst[i+8], v1);
        _mm256_stream_si256((__m256i*)&dst[i+16], v2);
        _mm256_stream_si256((__m256i*)&dst[i+24], v3);
        _mm256_stream_si256((__m256i*)&dst[i+32], v4);
        _mm256_stream_si256((__m256i*)&dst[i+40], v5);
        _mm256_stream_si256((__m256i*)&dst[i+48], v6);
        _mm256_stream_si256((__m256i*)&dst[i+56], v7);
    }
    
    /* Tail */
    for(; i+8 <= n; i+=8) {
        __m256i v = _mm256_loadu_si256((__m256i*)&src[i]);
        _mm256_stream_si256((__m256i*)&dst[i], v);
    }
    _mm_sfence();
    for(; i<n; i++) dst[i] = src[i];
}

/* INT8 version — 128 elements at once */
void kyjit_stream_int8_optimized(const int8_t *restrict src,
                                  int8_t *restrict dst, int n) {
    int i = 0;
    for(; i+128 <= n; i+=128) {
        __builtin_prefetch(&src[i+256], 0, 0);
        
        __m256i v0 = _mm256_loadu_si256((__m256i*)&src[i]);
        __m256i v1 = _mm256_loadu_si256((__m256i*)&src[i+32]);
        __m256i v2 = _mm256_loadu_si256((__m256i*)&src[i+64]);
        __m256i v3 = _mm256_loadu_si256((__m256i*)&src[i+96]);
        
        _mm256_stream_si256((__m256i*)&dst[i], v0);
        _mm256_stream_si256((__m256i*)&dst[i+32], v1);
        _mm256_stream_si256((__m256i*)&dst[i+64], v2);
        _mm256_stream_si256((__m256i*)&dst[i+96], v3);
    }
    _mm_sfence();
    for(; i<n; i++) dst[i] = src[i];
}
