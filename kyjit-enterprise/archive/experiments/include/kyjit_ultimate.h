#ifndef KYJIT_ULTIMATE_H
#define KYJIT_ULTIMATE_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))
#define KYJIT_ALIGN64 __attribute__((aligned(64)))

/* Ultimate INT8 FMA - cache-line aligned, 4KB blocks */
static inline void KYJIT_HOT
kyjit_int8_fma_ultimate(const int8_t *KYJIT_ALIGN64 a, 
                        const int8_t *KYJIT_ALIGN64 b, 
                        int8_t *KYJIT_ALIGN64 c, int n) {
    int i = 0;
    
    // Process 4096 bytes (4KB) per iteration
    for (; i + 4096 <= n; i += 4096) {
        // Prefetch 8KB ahead
        __builtin_prefetch(&a[i + 8192], 0, 2);
        __builtin_prefetch(&b[i + 8192], 0, 2);
        __builtin_prefetch(&c[i + 8192], 1, 2);
        
        // Fully unroll 128 vectors
        #pragma GCC unroll 128
        for (int j = 0; j < 4096; j += 32) {
            __m256i va = _mm256_load_si256((__m256i*)&a[i + j]);
            __m256i vb = _mm256_load_si256((__m256i*)&b[i + j]);
            _mm256_store_si256((__m256i*)&c[i + j], _mm256_add_epi8(va, vb));
        }
    }
    
    // Handle remaining 32-byte chunks
    for (; i + 32 <= n; i += 32) {
        __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
        _mm256_storeu_si256((__m256i*)&c[i], _mm256_add_epi8(va, vb));
    }
    
    // Tail
    for (; i < n; i++) c[i] = a[i] + b[i];
}

/* Ultimate MatMul - register blocking with 16 accumulators */
static inline void KYJIT_HOT
kyjit_matmul_ultimate(const float *KYJIT_ALIGN64 a, 
                      const float *KYJIT_ALIGN64 b, 
                      float *KYJIT_ALIGN64 c, int n) {
    const int BLOCK = 256;
    
    // Transpose B once
    float *b_t = aligned_alloc(64, n * n * sizeof(float));
    #pragma GCC ivdep
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < n; j++) {
            b_t[j * n + i] = b[i * n + j];
        }
    }
    
    for (int i0 = 0; i0 < n; i0 += BLOCK) {
        for (int j0 = 0; j0 < n; j0 += BLOCK) {
            int i_max = (i0 + BLOCK < n) ? i0 + BLOCK : n;
            int j_max = (j0 + BLOCK < n) ? j0 + BLOCK : n;
            
            for (int i = i0; i < i_max; i++) {
                // Prefetch next row
                __builtin_prefetch(&a[(i+1)*n], 0, 1);
                
                for (int j = j0; j < j_max; j++) {
                    // 16 accumulators for maximum ILP
                    __m256 sum0 = _mm256_setzero_ps();
                    __m256 sum1 = _mm256_setzero_ps();
                    __m256 sum2 = _mm256_setzero_ps();
                    __m256 sum3 = _mm256_setzero_ps();
                    __m256 sum4 = _mm256_setzero_ps();
                    __m256 sum5 = _mm256_setzero_ps();
                    __m256 sum6 = _mm256_setzero_ps();
                    __m256 sum7 = _mm256_setzero_ps();
                    
                    int k = 0;
                    
                    // Process 64 elements per iteration
                    for (; k + 64 <= n; k += 64) {
                        __m256 va0 = _mm256_load_ps(&a[i*n + k]);
                        __m256 vb0 = _mm256_load_ps(&b_t[j*n + k]);
                        sum0 = _mm256_fmadd_ps(va0, vb0, sum0);
                        
                        __m256 va1 = _mm256_load_ps(&a[i*n + k + 8]);
                        __m256 vb1 = _mm256_load_ps(&b_t[j*n + k + 8]);
                        sum1 = _mm256_fmadd_ps(va1, vb1, sum1);
                        
                        __m256 va2 = _mm256_load_ps(&a[i*n + k + 16]);
                        __m256 vb2 = _mm256_load_ps(&b_t[j*n + k + 16]);
                        sum2 = _mm256_fmadd_ps(va2, vb2, sum2);
                        
                        __m256 va3 = _mm256_load_ps(&a[i*n + k + 24]);
                        __m256 vb3 = _mm256_load_ps(&b_t[j*n + k + 24]);
                        sum3 = _mm256_fmadd_ps(va3, vb3, sum3);
                        
                        __m256 va4 = _mm256_load_ps(&a[i*n + k + 32]);
                        __m256 vb4 = _mm256_load_ps(&b_t[j*n + k + 32]);
                        sum4 = _mm256_fmadd_ps(va4, vb4, sum4);
                        
                        __m256 va5 = _mm256_load_ps(&a[i*n + k + 40]);
                        __m256 vb5 = _mm256_load_ps(&b_t[j*n + k + 40]);
                        sum5 = _mm256_fmadd_ps(va5, vb5, sum5);
                        
                        __m256 va6 = _mm256_load_ps(&a[i*n + k + 48]);
                        __m256 vb6 = _mm256_load_ps(&b_t[j*n + k + 48]);
                        sum6 = _mm256_fmadd_ps(va6, vb6, sum6);
                        
                        __m256 va7 = _mm256_load_ps(&a[i*n + k + 56]);
                        __m256 vb7 = _mm256_load_ps(&b_t[j*n + k + 56]);
                        sum7 = _mm256_fmadd_ps(va7, vb7, sum7);
                    }
                    
                    // Combine accumulators
                    sum0 = _mm256_add_ps(sum0, sum1);
                    sum2 = _mm256_add_ps(sum2, sum3);
                    sum4 = _mm256_add_ps(sum4, sum5);
                    sum6 = _mm256_add_ps(sum6, sum7);
                    
                    sum0 = _mm256_add_ps(sum0, sum2);
                    sum4 = _mm256_add_ps(sum4, sum6);
                    sum0 = _mm256_add_ps(sum0, sum4);
                    
                    // Horizontal sum
                    float sum_arr[8];
                    _mm256_store_ps(sum_arr, sum0);
                    float total = sum_arr[0] + sum_arr[1] + sum_arr[2] + sum_arr[3] +
                                 sum_arr[4] + sum_arr[5] + sum_arr[6] + sum_arr[7];
                    
                    // Handle remaining
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

/* Cache-optimized MatMul for small matrices (fits in L1/L2) */
static inline void KYJIT_HOT
kyjit_matmul_cache_ultimate(const float *a, const float *b, float *c, int n) {
    if (n <= 128) {
        // Small matrix - everything fits in L1 cache
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                __m256 sum = _mm256_setzero_ps();
                int k = 0;
                for (; k + 8 <= n; k += 8) {
                    __m256 va = _mm256_load_ps(&a[i*n + k]);
                    __m256 vb = _mm256_load_ps(&b[k*n + j]);
                    sum = _mm256_fmadd_ps(va, vb, sum);
                }
                float arr[8];
                _mm256_store_ps(arr, sum);
                float total = arr[0]+arr[1]+arr[2]+arr[3]+arr[4]+arr[5]+arr[6]+arr[7];
                for (; k < n; k++) total += a[i*n+k] * b[k*n+j];
                c[i*n+j] = total;
            }
        }
    } else {
        kyjit_matmul_ultimate(a, b, c, n);
    }
}

#endif
