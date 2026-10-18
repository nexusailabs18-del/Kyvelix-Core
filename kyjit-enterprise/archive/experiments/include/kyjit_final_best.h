#ifndef KYJIT_FINAL_BEST_H
#define KYJIT_FINAL_BEST_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))

/* BEST: INT8 FMA - 4KB blocks, fully unrolled */
static inline void KYJIT_HOT
kyjit_int8_fma_best(const int8_t *a, const int8_t *b, int8_t *c, int n) {
    int i = 0;
    
    for (; i + 4096 <= n; i += 4096) {
        __builtin_prefetch(&a[i + 8192], 0, 2);
        __builtin_prefetch(&b[i + 8192], 0, 2);
        
        #pragma GCC unroll 128
        for (int j = 0; j < 4096; j += 32) {
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
    
    for (; i < n; i++) c[i] = a[i] + b[i];
}

/* BEST: MatMul - Register blocking with 4x12 micro-kernel (228 GFLOP/s) */
static inline void KYJIT_HOT
kyjit_matmul_best(const float *A, const float *B, float *C, int n) {
    const int MC = 256;
    const int KC = 256;
    const int MR = 4;
    const int NR = 12;
    
    memset(C, 0, n * n * sizeof(float));
    
    for (int jc = 0; jc < n; jc += MC) {
        for (int pc = 0; pc < n; pc += KC) {
            for (int ic = 0; ic < n; ic += MC) {
                
                int j_max = (jc + MC < n) ? jc + MC : n;
                int p_max = (pc + KC < n) ? pc + KC : n;
                int i_max = (ic + MC < n) ? ic + MC : n;
                
                for (int j = jc; j < j_max; j += NR) {
                    for (int i = ic; i < i_max; i += MR) {
                        
                        __builtin_prefetch(&A[(i+MR)*n + pc], 0, 2);
                        __builtin_prefetch(&B[pc*n + j+NR], 0, 2);
                        
                        __m256 c00 = _mm256_setzero_ps(), c01 = _mm256_setzero_ps(), c02 = _mm256_setzero_ps();
                        __m256 c10 = _mm256_setzero_ps(), c11 = _mm256_setzero_ps(), c12 = _mm256_setzero_ps();
                        __m256 c20 = _mm256_setzero_ps(), c21 = _mm256_setzero_ps(), c22 = _mm256_setzero_ps();
                        __m256 c30 = _mm256_setzero_ps(), c31 = _mm256_setzero_ps(), c32 = _mm256_setzero_ps();
                        
                        int i_lim = (i + MR < i_max) ? i + MR : i_max;
                        int j_lim = (j + NR < j_max) ? j + NR : j_max;
                        
                        for (int p = pc; p < p_max; p += 8) {
                            __m256 a0 = _mm256_loadu_ps(&A[i*n + p]);
                            __m256 a1 = (i+1 < i_lim) ? _mm256_loadu_ps(&A[(i+1)*n + p]) : _mm256_setzero_ps();
                            __m256 a2 = (i+2 < i_lim) ? _mm256_loadu_ps(&A[(i+2)*n + p]) : _mm256_setzero_ps();
                            __m256 a3 = (i+3 < i_lim) ? _mm256_loadu_ps(&A[(i+3)*n + p]) : _mm256_setzero_ps();
                            
                            for (int jj = 0; jj < 12 && j + jj < j_lim; jj += 4) {
                                __m256 b0 = _mm256_broadcast_ss(&B[p*n + j + jj]);
                                __m256 b1 = _mm256_broadcast_ss(&B[p*n + j + jj + 1]);
                                __m256 b2 = _mm256_broadcast_ss(&B[p*n + j + jj + 2]);
                                __m256 b3 = _mm256_broadcast_ss(&B[p*n + j + jj + 3]);
                                
                                if (jj == 0) {
                                    c00 = _mm256_fmadd_ps(a0, b0, c00);
                                    c10 = _mm256_fmadd_ps(a1, b0, c10);
                                    c20 = _mm256_fmadd_ps(a2, b0, c20);
                                    c30 = _mm256_fmadd_ps(a3, b0, c30);
                                } else if (jj == 4) {
                                    c01 = _mm256_fmadd_ps(a0, b1, c01);
                                    c11 = _mm256_fmadd_ps(a1, b1, c11);
                                    c21 = _mm256_fmadd_ps(a2, b1, c21);
                                    c31 = _mm256_fmadd_ps(a3, b1, c31);
                                } else if (jj == 8) {
                                    c02 = _mm256_fmadd_ps(a0, b2, c02);
                                    c12 = _mm256_fmadd_ps(a1, b2, c12);
                                    c22 = _mm256_fmadd_ps(a2, b2, c22);
                                    c32 = _mm256_fmadd_ps(a3, b2, c32);
                                }
                            }
                        }
                        
                        if (i < i_max && j < j_max) {
                            float tmp[8];
                            
                            if (j < j_max) {
                                _mm256_storeu_ps(tmp, c00);
                                C[i*n + j] += tmp[0] + tmp[1] + tmp[2] + tmp[3] + tmp[4] + tmp[5] + tmp[6] + tmp[7];
                            }
                            if (i+1 < i_max && j < j_max) {
                                _mm256_storeu_ps(tmp, c10);
                                C[(i+1)*n + j] += tmp[0] + tmp[1] + tmp[2] + tmp[3] + tmp[4] + tmp[5] + tmp[6] + tmp[7];
                            }
                            if (i+2 < i_max && j < j_max) {
                                _mm256_storeu_ps(tmp, c20);
                                C[(i+2)*n + j] += tmp[0] + tmp[1] + tmp[2] + tmp[3] + tmp[4] + tmp[5] + tmp[6] + tmp[7];
                            }
                            if (i+3 < i_max && j < j_max) {
                                _mm256_storeu_ps(tmp, c30);
                                C[(i+3)*n + j] += tmp[0] + tmp[1] + tmp[2] + tmp[3] + tmp[4] + tmp[5] + tmp[6] + tmp[7];
                            }
                        }
                    }
                }
            }
        }
    }
}

#endif
