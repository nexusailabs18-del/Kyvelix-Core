#ifndef KYJIT_FAST_H
#define KYJIT_FAST_H

#include <immintrin.h>
#include <stdint.h>

/* Force inline for hot paths */
#define KYJIT_HOT __attribute__((always_inline, hot, flatten))
#define KYJIT_COLD __attribute__((cold))
#define KYJIT_LIKELY(x) __builtin_expect(!!(x), 1)
#define KYJIT_UNLIKELY(x) __builtin_expect(!!(x), 0)

/* Prefetch macros */
#define KYJIT_PREFETCH_R(x) __builtin_prefetch((x), 0, 3)
#define KYJIT_PREFETCH_W(x) __builtin_prefetch((x), 1, 3)
#define KYJIT_PREFETCH_L1(x) __builtin_prefetch((x), 0, 1)

/* Branch hints */
#define KYJIT_LIKELY_BRANCH(x) if (KYJIT_LIKELY(x))
#define KYJIT_UNLIKELY_BRANCH(x) if (KYJIT_UNLIKELY(x))

/* Vectorized operations - 16 accumulators for better ILP */
static inline __m256i kyjit_sum_i32_avx2_16acc(const int32_t *data, int n) {
    __m256i s0 = _mm256_setzero_si256();
    __m256i s1 = _mm256_setzero_si256();
    __m256i s2 = _mm256_setzero_si256();
    __m256i s3 = _mm256_setzero_si256();
    __m256i s4 = _mm256_setzero_si256();
    __m256i s5 = _mm256_setzero_si256();
    __m256i s6 = _mm256_setzero_si256();
    __m256i s7 = _mm256_setzero_si256();
    __m256i s8 = _mm256_setzero_si256();
    __m256i s9 = _mm256_setzero_si256();
    __m256i s10 = _mm256_setzero_si256();
    __m256i s11 = _mm256_setzero_si256();
    __m256i s12 = _mm256_setzero_si256();
    __m256i s13 = _mm256_setzero_si256();
    __m256i s14 = _mm256_setzero_si256();
    __m256i s15 = _mm256_setzero_si256();
    
    int i = 0;
    for (; i + 128 <= n; i += 128) {
        s0 = _mm256_add_epi32(s0, _mm256_loadu_si256((__m256i*)&data[i]));
        s1 = _mm256_add_epi32(s1, _mm256_loadu_si256((__m256i*)&data[i+8]));
        s2 = _mm256_add_epi32(s2, _mm256_loadu_si256((__m256i*)&data[i+16]));
        s3 = _mm256_add_epi32(s3, _mm256_loadu_si256((__m256i*)&data[i+24]));
        s4 = _mm256_add_epi32(s4, _mm256_loadu_si256((__m256i*)&data[i+32]));
        s5 = _mm256_add_epi32(s5, _mm256_loadu_si256((__m256i*)&data[i+40]));
        s6 = _mm256_add_epi32(s6, _mm256_loadu_si256((__m256i*)&data[i+48]));
        s7 = _mm256_add_epi32(s7, _mm256_loadu_si256((__m256i*)&data[i+56]));
        s8 = _mm256_add_epi32(s8, _mm256_loadu_si256((__m256i*)&data[i+64]));
        s9 = _mm256_add_epi32(s9, _mm256_loadu_si256((__m256i*)&data[i+72]));
        s10 = _mm256_add_epi32(s10, _mm256_loadu_si256((__m256i*)&data[i+80]));
        s11 = _mm256_add_epi32(s11, _mm256_loadu_si256((__m256i*)&data[i+88]));
        s12 = _mm256_add_epi32(s12, _mm256_loadu_si256((__m256i*)&data[i+96]));
        s13 = _mm256_add_epi32(s13, _mm256_loadu_si256((__m256i*)&data[i+104]));
        s14 = _mm256_add_epi32(s14, _mm256_loadu_si256((__m256i*)&data[i+112]));
        s15 = _mm256_add_epi32(s15, _mm256_loadu_si256((__m256i*)&data[i+120]));
    }
    
    // Combine all 16 accumulators
    s0 = _mm256_add_epi32(s0, s1);
    s2 = _mm256_add_epi32(s2, s3);
    s4 = _mm256_add_epi32(s4, s5);
    s6 = _mm256_add_epi32(s6, s7);
    s8 = _mm256_add_epi32(s8, s9);
    s10 = _mm256_add_epi32(s10, s11);
    s12 = _mm256_add_epi32(s12, s13);
    s14 = _mm256_add_epi32(s14, s15);
    
    s0 = _mm256_add_epi32(s0, s2);
    s4 = _mm256_add_epi32(s4, s6);
    s8 = _mm256_add_epi32(s8, s10);
    s12 = _mm256_add_epi32(s12, s14);
    
    s0 = _mm256_add_epi32(s0, s4);
    s8 = _mm256_add_epi32(s8, s12);
    
    return _mm256_add_epi32(s0, s8);
}

/* Fast INT8 FMA with 16 accumulators */
static inline void kyjit_int8_fma_16acc(const int8_t *a, const int8_t *b, int8_t *c, int n) {
    int i = 0;
    for (; i + 256 <= n; i += 256) {
        __m256i va0 = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb0 = _mm256_loadu_si256((__m256i*)&b[i]);
        __m256i vc0 = _mm256_add_epi8(va0, vb0);
        _mm256_storeu_si256((__m256i*)&c[i], vc0);
        
        __m256i va1 = _mm256_loadu_si256((__m256i*)&a[i+32]);
        __m256i vb1 = _mm256_loadu_si256((__m256i*)&b[i+32]);
        __m256i vc1 = _mm256_add_epi8(va1, vb1);
        _mm256_storeu_si256((__m256i*)&c[i+32], vc1);
        
        __m256i va2 = _mm256_loadu_si256((__m256i*)&a[i+64]);
        __m256i vb2 = _mm256_loadu_si256((__m256i*)&b[i+64]);
        __m256i vc2 = _mm256_add_epi8(va2, vb2);
        _mm256_storeu_si256((__m256i*)&c[i+64], vc2);
        
        __m256i va3 = _mm256_loadu_si256((__m256i*)&a[i+96]);
        __m256i vb3 = _mm256_loadu_si256((__m256i*)&b[i+96]);
        __m256i vc3 = _mm256_add_epi8(va3, vb3);
        _mm256_storeu_si256((__m256i*)&c[i+96], vc3);
        
        __m256i va4 = _mm256_loadu_si256((__m256i*)&a[i+128]);
        __m256i vb4 = _mm256_loadu_si256((__m256i*)&b[i+128]);
        __m256i vc4 = _mm256_add_epi8(va4, vb4);
        _mm256_storeu_si256((__m256i*)&c[i+128], vc4);
        
        __m256i va5 = _mm256_loadu_si256((__m256i*)&a[i+160]);
        __m256i vb5 = _mm256_loadu_si256((__m256i*)&b[i+160]);
        __m256i vc5 = _mm256_add_epi8(va5, vb5);
        _mm256_storeu_si256((__m256i*)&c[i+160], vc5);
        
        __m256i va6 = _mm256_loadu_si256((__m256i*)&a[i+192]);
        __m256i vb6 = _mm256_loadu_si256((__m256i*)&b[i+192]);
        __m256i vc6 = _mm256_add_epi8(va6, vb6);
        _mm256_storeu_si256((__m256i*)&c[i+192], vc6);
        
        __m256i va7 = _mm256_loadu_si256((__m256i*)&a[i+224]);
        __m256i vb7 = _mm256_loadu_si256((__m256i*)&b[i+224]);
        __m256i vc7 = _mm256_add_epi8(va7, vb7);
        _mm256_storeu_si256((__m256i*)&c[i+224], vc7);
    }
    
    // Handle remaining
    for (; i + 32 <= n; i += 32) {
        __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
        _mm256_storeu_si256((__m256i*)&c[i], _mm256_add_epi8(va, vb));
    }
}

/* Cache-friendly matrix multiply with blocking */
static inline void kyjit_matmul_blocked_avx2(const float *a, const float *b, float *c, int n) {
    const int BLOCK = 32;  // Fits in L1 cache
    
    for (int i0 = 0; i0 < n; i0 += BLOCK) {
        for (int j0 = 0; j0 < n; j0 += BLOCK) {
            for (int k0 = 0; k0 < n; k0 += BLOCK) {
                // Blocked matrix multiply
                for (int i = i0; i < i0 + BLOCK && i < n; i++) {
                    for (int j = j0; j < j0 + BLOCK && j < n; j++) {
                        __m256 sum = _mm256_setzero_ps();
                        int k = k0;
                        for (; k + 8 <= k0 + BLOCK && k + 8 <= n; k += 8) {
                            __m256 va = _mm256_loadu_ps(&a[i*n + k]);
                            __m256 vb = _mm256_loadu_ps(&b[k*n + j]);
                            sum = _mm256_fmadd_ps(va, vb, sum);
                        }
                        // Horizontal sum
                        float sum_arr[8];
                        _mm256_storeu_ps(sum_arr, sum);
                        float total = sum_arr[0] + sum_arr[1] + sum_arr[2] + sum_arr[3] +
                                     sum_arr[4] + sum_arr[5] + sum_arr[6] + sum_arr[7];
                        // Handle remaining elements
                        for (; k < k0 + BLOCK && k < n; k++) {
                            total += a[i*n + k] * b[k*n + j];
                        }
                        c[i*n + j] = total;
                    }
                }
            }
        }
    }
}

#endif // KYJIT_FAST_H
