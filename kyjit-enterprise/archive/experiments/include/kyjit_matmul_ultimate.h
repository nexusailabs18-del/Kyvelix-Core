#ifndef KYJIT_MATMUL_ULTIMATE_H
#define KYJIT_MATMUL_ULTIMATE_H

#include <immintrin.h>
#include <stdint.h>

#define KYJIT_HOT __attribute__((always_inline, hot, flatten))

/* Ultimate MatMul - register blocking + cache blocking + SIMD */
static inline void KYJIT_HOT
kyjit_matmul_ultimate(const float *A, const float *B, float *C, int n) {
    const int MC = 256;  // Cache block size
    const int KC = 256;
    const int MR = 4;    // Register block rows
    const int NR = 12;   // Register block cols (3 AVX2 registers)
    
    // Initialize C to zero
    memset(C, 0, n * n * sizeof(float));
    
    // Main loop with cache blocking
    for (int jc = 0; jc < n; jc += MC) {
        for (int pc = 0; pc < n; pc += KC) {
            for (int ic = 0; ic < n; ic += MC) {
                
                int j_max = (jc + MC < n) ? jc + MC : n;
                int p_max = (pc + KC < n) ? pc + KC : n;
                int i_max = (ic + MC < n) ? ic + MC : n;
                
                // Micro-kernel with register blocking
                for (int j = jc; j < j_max; j += NR) {
                    for (int i = ic; i < i_max; i += MR) {
                        
                        // Prefetch next tiles
                        __builtin_prefetch(&A[(i+MR)*n + pc], 0, 2);
                        __builtin_prefetch(&B[pc*n + j+NR], 0, 2);
                        
                        // Register accumulators: 4 rows x 12 cols
                        __m256 c00 = _mm256_setzero_ps(), c01 = _mm256_setzero_ps(), c02 = _mm256_setzero_ps();
                        __m256 c10 = _mm256_setzero_ps(), c11 = _mm256_setzero_ps(), c12 = _mm256_setzero_ps();
                        __m256 c20 = _mm256_setzero_ps(), c21 = _mm256_setzero_ps(), c22 = _mm256_setzero_ps();
                        __m256 c30 = _mm256_setzero_ps(), c31 = _mm256_setzero_ps(), c32 = _mm256_setzero_ps();
                        
                        int i_lim = (i + MR < i_max) ? i + MR : i_max;
                        int j_lim = (j + NR < j_max) ? j + NR : j_max;
                        
                        for (int p = pc; p < p_max; p += 8) {
                            // Load A block (4 rows x 8 cols)
                            __m256 a0 = _mm256_loadu_ps(&A[i*n + p]);
                            __m256 a1 = (i+1 < i_lim) ? _mm256_loadu_ps(&A[(i+1)*n + p]) : _mm256_setzero_ps();
                            __m256 a2 = (i+2 < i_lim) ? _mm256_loadu_ps(&A[(i+2)*n + p]) : _mm256_setzero_ps();
                            __m256 a3 = (i+3 < i_lim) ? _mm256_loadu_ps(&A[(i+3)*n + p]) : _mm256_setzero_ps();
                            
                            // Load and broadcast B elements
                            for (int jj = 0; jj < 12 && j + jj < j_lim; jj += 4) {
                                __m256 b0 = _mm256_broadcast_ss(&B[p*n + j + jj]);
                                __m256 b1 = _mm256_broadcast_ss(&B[p*n + j + jj + 1]);
                                __m256 b2 = _mm256_broadcast_ss(&B[p*n + j + jj + 2]);
                                __m256 b3 = _mm256_broadcast_ss(&B[p*n + j + jj + 3]);
                                
                                // FMA operations for all 4 rows
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
                        
                        // Store results
                        if (i < i_max && j < j_max) {
                            // Horizontal sums and store
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

/* Alternative: Use transposed B for better SIMD */
static inline void KYJIT_HOT
kyjit_matmul_transposed(const float *A, const float *B, float *C, int n) {
    // Transpose B
    float *Bt = aligned_alloc(32, n * n * sizeof(float));
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < n; j++) {
            Bt[j * n + i] = B[i * n + j];
        }
    }
    
    memset(C, 0, n * n * sizeof(float));
    
    const int BLOCK = 64;
    
    for (int i = 0; i < n; i += BLOCK) {
        for (int j = 0; j < n; j += BLOCK) {
            int i_max = (i + BLOCK < n) ? i + BLOCK : n;
            int j_max = (j + BLOCK < n) ? j + BLOCK : n;
            
            for (int ii = i; ii < i_max; ii++) {
                for (int jj = j; jj < j_max; jj += 8) {
                    __m256 sum = _mm256_setzero_ps();
                    
                    for (int k = 0; k < n; k += 8) {
                        __m256 va = _mm256_loadu_ps(&A[ii*n + k]);
                        __m256 vb = _mm256_loadu_ps(&Bt[jj*n + k]);
                        sum = _mm256_fmadd_ps(va, vb, sum);
                    }
                    
                    float tmp[8];
                    _mm256_storeu_ps(tmp, sum);
                    for (int t = 0; t < 8 && jj + t < j_max; t++) {
                        C[ii*n + jj + t] += tmp[t];
                    }
                }
            }
        }
    }
    
    free(Bt);
}

#endif
