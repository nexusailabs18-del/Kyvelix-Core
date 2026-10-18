#ifndef KYJIT_BLAS_JIT_H
#define KYJIT_BLAS_JIT_H

#include <stdint.h>
#include <stddef.h>

/* Matrix multiply using OpenBLAS inside JIT context */
int kyjit_blas_matmul_jit(const float *A, const float *B, float *C, int N);

/* Vector dot product using OpenBLAS */
float kyjit_blas_dot_jit(const float *a, const float *b, int n);

/* Matrix-vector multiply */
int kyjit_blas_gemv_jit(const float *A, const float *x, float *y, int M, int N);

#endif

/* General matrix multiply: C(M×N) = A(M×K) × B(K×N) */
int kyjit_blas_matmul_general(const float *A, const float *B, float *C,
                               int M, int N, int K);

/* cuBLAS GPU functions (fallback to OpenBLAS if no GPU) */
int kyjit_cublas_init(void);
int kyjit_cublas_matmul(const float *A, const float *B, float *C, int N);
int kyjit_cublas_matmul_general(const float *A, const float *B, float *C,
                                 int M, int N, int K);
int kyjit_cuda_available(void);
void kyjit_cublas_cleanup(void);
