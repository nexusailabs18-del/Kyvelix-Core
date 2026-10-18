#include <immintrin.h>
#include "kyjit_pack.h"

/* Unpack 8-bit values to 32-bit (4x memory reduction) */
void kyjit_unpack_8to32(const uint8_t *restrict packed, 
                         int32_t *restrict unpacked, int n) {
    int i = 0, j = 0;
    for(; i+8 <= n; i+=8, j+=32) {
        /* Load 8 packed bytes */
        __m128i v = _mm_loadl_epi64((__m128i*)&packed[i]);
        /* Zero-extend to 32-bit integers */
        __m256i v32 = _mm256_cvtepu8_epi32(v);
        _mm256_storeu_si256((__m256i*)&unpacked[j], v32);
    }
    for(; i<n; i++, j++) unpacked[j] = packed[i];
}

/* Unpack 16-bit to 32-bit (2x memory reduction) */
void kyjit_unpack_16to32(const uint16_t *restrict packed,
                          int32_t *restrict unpacked, int n) {
    int i = 0, j = 0;
    for(; i+8 <= n; i+=8, j+=8) {
        __m128i v = _mm_loadu_si128((__m128i*)&packed[i]);
        __m256i v32 = _mm256_cvtepu16_epi32(v);
        _mm256_storeu_si256((__m256i*)&unpacked[j], v32);
    }
    for(; i<n; i++, j++) unpacked[j] = packed[i];
}
