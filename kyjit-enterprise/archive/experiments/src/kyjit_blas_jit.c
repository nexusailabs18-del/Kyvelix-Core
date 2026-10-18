#include "kyjit_blas_jit.h"
#include <cblas.h>

int kyjit_blas_matmul_jit(const float *A, const float *B, float *C, int N) {
    cblas_sgemm(CblasRowMajor, CblasNoTrans, CblasNoTrans,
                N, N, N, 1.0f, A, N, B, N, 0.0f, C, N);
    return 0;
}

float kyjit_blas_dot_jit(const float *a, const float *b, int n) {
    return cblas_sdot(n, a, 1, b, 1);
}

int kyjit_blas_gemv_jit(const float *A, const float *x, float *y, int M, int N) {
    cblas_sgemv(CblasRowMajor, CblasNoTrans,
                M, N, 1.0f, A, N, x, 1, 0.0f, y, 1);
    return 0;
}

/* General matrix multiply: C(M×N) = A(M×K) × B(K×N) */
int kyjit_blas_matmul_general(const float *A, const float *B, float *C,
                               int M, int N, int K) {
    cblas_sgemm(CblasRowMajor, CblasNoTrans, CblasNoTrans,
                M, N, K, 1.0f, A, K, B, N, 0.0f, C, N);
    return 0;
}
