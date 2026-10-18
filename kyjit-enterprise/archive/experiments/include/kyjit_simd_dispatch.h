#ifndef KYJIT_SIMD_DISPATCH_H
#define KYJIT_SIMD_DISPATCH_H

#include <immintrin.h>
#include <stdint.h>

/* Auto-detect best SIMD at compile time */
#ifdef __AVX512F__
    #define KYJIT_HAVE_AVX512 1
    #define KYJIT_SIMD_WIDTH 64
    #define KYJIT_VEC_TYPE __m512i
    #define KYJIT_LOAD(ptr) _mm512_loadu_si512((__m512i*)(ptr))
    #define KYJIT_STORE(ptr, val) _mm512_storeu_si512((__m512i*)(ptr), (val))
    #define KYJIT_ADD_EPI32(a,b) _mm512_add_epi32((a),(b))
    #define KYJIT_ADD_EPI8(a,b) _mm512_add_epi8((a),(b))
    #define KYJIT_ZERO() _mm512_setzero_si512()
#elif defined(__AVX2__)
    #define KYJIT_HAVE_AVX512 0
    #define KYJIT_SIMD_WIDTH 32
    #define KYJIT_VEC_TYPE __m256i
    #define KYJIT_LOAD(ptr) _mm256_loadu_si256((__m256i*)(ptr))
    #define KYJIT_STORE(ptr, val) _mm256_storeu_si256((__m256i*)(ptr), (val))
    #define KYJIT_ADD_EPI32(a,b) _mm256_add_epi32((a),(b))
    #define KYJIT_ADD_EPI8(a,b) _mm256_add_epi8((a),(b))
    #define KYJIT_ZERO() _mm256_setzero_si256()
#else
    #define KYJIT_HAVE_AVX512 0
    #define KYJIT_SIMD_WIDTH 16
    #define KYJIT_VEC_TYPE __m128i
    #define KYJIT_LOAD(ptr) _mm_loadu_si128((__m128i*)(ptr))
    #define KYJIT_STORE(ptr, val) _mm_storeu_si128((__m128i*)(ptr), (val))
    #define KYJIT_ADD_EPI32(a,b) _mm_add_epi32((a),(b))
    #define KYJIT_ADD_EPI8(a,b) _mm_add_epi8((a),(b))
    #define KYJIT_ZERO() _mm_setzero_si128()
#endif

#endif
