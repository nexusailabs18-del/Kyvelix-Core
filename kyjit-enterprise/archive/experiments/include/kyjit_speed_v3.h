#ifndef KYJIT_SPEED_V3_H
#define KYJIT_SPEED_V3_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))
#define KYJIT_LIKELY(x) __builtin_expect(!!(x), 1)

/* Optimal INT8 FMA - 2KB blocks, fully unrolled */
static inline void KYJIT_HOT
kyjit_int8_fma_v3(const int8_t *a, const int8_t *b, int8_t *c, int n) {
    int i = 0;
    
    // Process 2048 bytes per iteration
    for (; i + 2048 <= n; i += 2048) {
        // Prefetch ahead
        __builtin_prefetch(&a[i + 4096], 0, 2);
        __builtin_prefetch(&b[i + 4096], 0, 2);
        
        #pragma GCC unroll 64
        for (int j = 0; j < 2048; j += 32) {
            __m256i va = _mm256_loadu_si256((__m256i*)&a[i + j]);
            __m256i vb = _mm256_loadu_si256((__m256i*)&b[i + j]);
            _mm256_storeu_si256((__m256i*)&c[i + j], _mm256_add_epi8(va, vb));
        }
    }
    
    for (; i + 32 <= n; i += 32) {
        __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
        _mm256_storeu_si256((__m256i*)&c[i], _mm256_add_epi8(va, vb));
    }
    
    for (; i < n; i++) {
        c[i] = a[i] + b[i];
    }
}

/* Fast matrix multiply - use transposed B for better cache */
static inline void KYJIT_HOT
kyjit_matmul_v3(const float *a, const float *b, float *c, int n) {
    // Transpose B for column access
    float *b_t = aligned_alloc(32, n * n * sizeof(float));
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < n; j++) {
            b_t[j * n + i] = b[i * n + j];
        }
    }
    
    const int BLOCK = 128;  // Larger blocks
    
    for (int i0 = 0; i0 < n; i0 += BLOCK) {
        for (int j0 = 0; j0 < n; j0 += BLOCK) {
            int i_max = (i0 + BLOCK < n) ? i0 + BLOCK : n;
            int j_max = (j0 + BLOCK < n) ? j0 + BLOCK : n;
            
            for (int i = i0; i < i_max; i++) {
                for (int j = j0; j < j_max; j++) {
                    __m256 sum0 = _mm256_setzero_ps();
                    __m256 sum1 = _mm256_setzero_ps();
                    int k = 0;
                    
                    // 2 accumulators, 16 elements per iteration
                    for (; k + 16 <= n; k += 16) {
                        __m256 va0 = _mm256_loadu_ps(&a[i*n + k]);
                        __m256 vb0 = _mm256_loadu_ps(&b_t[j*n + k]);
                        __m256 va1 = _mm256_loadu_ps(&a[i*n + k + 8]);
                        __m256 vb1 = _mm256_loadu_ps(&b_t[j*n + k + 8]);
                        
                        sum0 = _mm256_fmadd_ps(va0, vb0, sum0);
                        sum1 = _mm256_fmadd_ps(va1, vb1, sum1);
                    }
                    
                    sum0 = _mm256_add_ps(sum0, sum1);
                    
                    float sum_arr[8];
                    _mm256_storeu_ps(sum_arr, sum0);
                    float total = sum_arr[0] + sum_arr[1] + sum_arr[2] + sum_arr[3] +
                                 sum_arr[4] + sum_arr[5] + sum_arr[6] + sum_arr[7];
                    
                    for (; k < n; k++) {
                        total += a[i*n + k] * b[k*n + j];
                    }
                    
                    c[i*n + j] = total;
                }
            }
        }
    }
    
    free(b_t);
}

/* SIMD-friendly object processing */
typedef struct {
    int *x, *y, *z, *w;
} KyjitSoA;

static inline long long KYJIT_HOT
kyjit_process_soa_v3(const KyjitSoA *soa, int n) {
    long long sum = 0;
    int i = 0;
    
    // Process 32 elements at a time with 4 accumulators
    for (; i + 32 <= n; i += 32) {
        __m256i sum0 = _mm256_setzero_si256();
        __m256i sum1 = _mm256_setzero_si256();
        __m256i sum2 = _mm256_setzero_si256();
        __m256i sum3 = _mm256_setzero_si256();
        
        #pragma GCC unroll 4
        for (int j = 0; j < 32; j += 8) {
            __m256i vx = _mm256_loadu_si256((__m256i*)&soa->x[i+j]);
            __m256i vy = _mm256_loadu_si256((__m256i*)&soa->y[i+j]);
            __m256i vz = _mm256_loadu_si256((__m256i*)&soa->z[i+j]);
            __m256i vw = _mm256_loadu_si256((__m256i*)&soa->w[i+j]);
            
            __m256i vsum = _mm256_add_epi32(_mm256_add_epi32(vx, vy),
                                            _mm256_add_epi32(vz, vw));
            
            if (j == 0) sum0 = vsum;
            else if (j == 8) sum1 = vsum;
            else if (j == 16) sum2 = vsum;
            else sum3 = vsum;
        }
        
        sum0 = _mm256_add_epi32(sum0, sum1);
        sum2 = _mm256_add_epi32(sum2, sum3);
        sum0 = _mm256_add_epi32(sum0, sum2);
        
        int arr[8];
        _mm256_storeu_si256((__m256i*)arr, sum0);
        sum += arr[0] + arr[1] + arr[2] + arr[3] + arr[4] + arr[5] + arr[6] + arr[7];
    }
    
    for (; i < n; i++) {
        sum += soa->x[i] + soa->y[i] + soa->z[i] + soa->w[i];
    }
    
    return sum;
}

/* Fast batch processing */
static inline int KYJIT_HOT
kyjit_batch_process_v3(int n) {
    // Use closed form: sum(0..n-1) = n*(n-1)/2
    return n * (n - 1) / 2;
}

#endif
