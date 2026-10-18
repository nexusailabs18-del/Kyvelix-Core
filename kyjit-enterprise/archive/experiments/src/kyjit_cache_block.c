#include "kyjit_cache_block.h"
#include <immintrin.h>
#include <stdlib.h>
#include <string.h>

#define BLOCK 64

/* 4x8 micro-kernel using AVX2 FMA */
static void micro_kernel_4x8(const float *A, const float *B, float *C,
                             int K, int N, int i, int j) {
    __m256 c[4];
    for (int r = 0; r < 4; r++) c[r] = _mm256_loadu_ps(&C[(i+r)*N + j]);

    for (int k = 0; k < K; k++) {
        __m256 b = _mm256_loadu_ps(&B[k*N + j]);
        for (int r = 0; r < 4; r++) {
            c[r] = _mm256_fmadd_ps(_mm256_set1_ps(A[(i+r)*K + k]), b, c[r]);
        }
    }
    for (int r = 0; r < 4; r++) {
        _mm256_storeu_ps(&C[(i+r)*N + j], c[r]);
    }
}

int kyjit_cache_block_matmul(const float *A, const float *B, float *C,
                             int M, int K, int N) {
    if (!A || !B || !C || M <= 0 || K <= 0 || N <= 0) return -1;
    memset(C, 0, M * N * sizeof(float));

    for (int i0 = 0; i0 < M; i0 += BLOCK) {
        int imax = (i0 + BLOCK < M) ? i0 + BLOCK : M;
        for (int j0 = 0; j0 < N; j0 += BLOCK) {
            int jmax = (j0 + BLOCK < N) ? j0 + BLOCK : N;
            for (int k0 = 0; k0 < K; k0 += BLOCK) {
                int kmax = (k0 + BLOCK < K) ? k0 + BLOCK : K;
                /* micro-kernel inside block */
                for (int i = i0; i < imax; i += 4) {
                    for (int j = j0; j < jmax; j += 8) {
                        /* ensure valid 4x8 tile */
                        if (i+3 < imax && j+7 < jmax) {
                            micro_kernel_4x8(A, B, C, kmax-k0, N, i, j);
                        }
                    }
                }
            }
        }
    }
    return 0;
}

float kyjit_cache_block_sum(const float *arr, int n) {
    if (!arr || n <= 0) return 0.0f;
    __m256 sum0 = _mm256_setzero_ps(), sum1 = _mm256_setzero_ps(),
           sum2 = _mm256_setzero_ps(), sum3 = _mm256_setzero_ps(),
           sum4 = _mm256_setzero_ps(), sum5 = _mm256_setzero_ps(),
           sum6 = _mm256_setzero_ps(), sum7 = _mm256_setzero_ps();
    int i = 0;
    for (; i + 64 <= n; i += 64) {
        sum0 = _mm256_add_ps(sum0, _mm256_loadu_ps(&arr[i]));
        sum1 = _mm256_add_ps(sum1, _mm256_loadu_ps(&arr[i+8]));
        sum2 = _mm256_add_ps(sum2, _mm256_loadu_ps(&arr[i+16]));
        sum3 = _mm256_add_ps(sum3, _mm256_loadu_ps(&arr[i+24]));
        sum4 = _mm256_add_ps(sum4, _mm256_loadu_ps(&arr[i+32]));
        sum5 = _mm256_add_ps(sum5, _mm256_loadu_ps(&arr[i+40]));
        sum6 = _mm256_add_ps(sum6, _mm256_loadu_ps(&arr[i+48]));
        sum7 = _mm256_add_ps(sum7, _mm256_loadu_ps(&arr[i+56]));
    }
    sum0 = _mm256_add_ps(_mm256_add_ps(sum0,sum1), _mm256_add_ps(sum2,sum3));
    sum4 = _mm256_add_ps(_mm256_add_ps(sum4,sum5), _mm256_add_ps(sum6,sum7));
    sum0 = _mm256_add_ps(sum0, sum4);
    float res[8]; _mm256_storeu_ps(res, sum0);
    float total = res[0]+res[1]+res[2]+res[3]+res[4]+res[5]+res[6]+res[7];
    for (; i < n; i++) total += arr[i];
    return total;
}
