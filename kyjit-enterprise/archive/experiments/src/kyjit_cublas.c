#include "kyjit_blas_jit.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ============================================================
   KYJIT cuBLAS GPU Integration
   - Uses NVIDIA cuBLAS if GPU available
   - Falls back to OpenBLAS if no GPU
   - Compiles on ANY system (no CUDA required to compile)
   ============================================================ */

#ifdef KYJIT_HAVE_CUDA
#include <cuda_runtime.h>
#include <cublas_v2.h>

static cublasHandle_t cublas_handle = NULL;
static int cuda_initialized = 0;

/* Initialize cuBLAS */
int kyjit_cublas_init(void) {
    if (cuda_initialized) return 0;
    
    cublasStatus_t status = cublasCreate(&cublas_handle);
    if (status != CUBLAS_STATUS_SUCCESS) {
        fprintf(stderr, "cuBLAS init failed: %d\n", status);
        return -1;
    }
    
    cuda_initialized = 1;
    return 0;
}

/* GPU Matrix Multiply: C = A × B using cuBLAS
 * Falls back to OpenBLAS if GPU unavailable
 */
int kyjit_cublas_matmul(const float *A, const float *B, float *C, int N) {
    if (!cuda_initialized) {
        if (kyjit_cublas_init() != 0) {
            /* Fallback to OpenBLAS */
            return kyjit_blas_matmul_jit(A, B, C, N);
        }
    }
    
    float *d_A = NULL, *d_B = NULL, *d_C = NULL;
    size_t size = (size_t)N * N * sizeof(float);
    
    /* Allocate GPU memory */
    cudaMalloc(&d_A, size);
    cudaMalloc(&d_B, size);
    cudaMalloc(&d_C, size);
    
    /* Copy to GPU */
    cudaMemcpy(d_A, A, size, cudaMemcpyHostToDevice);
    cudaMemcpy(d_B, B, size, cudaMemcpyHostToDevice);
    
    /* cuBLAS SGEMM: C = alpha*A*B + beta*C */
    const float alpha = 1.0f, beta = 0.0f;
    cublasSgemm(cublas_handle,
                CUBLAS_OP_N, CUBLAS_OP_N,
                N, N, N,
                &alpha,
                d_A, N,
                d_B, N,
                &beta,
                d_C, N);
    
    /* Copy result back */
    cudaMemcpy(C, d_C, size, cudaMemcpyDeviceToHost);
    
    /* Cleanup */
    cudaFree(d_A);
    cudaFree(d_B);
    cudaFree(d_C);
    
    return 0;
}

/* General GPU Matrix Multiply: C(M×N) = A(M×K) × B(K×N) */
int kyjit_cublas_matmul_general(const float *A, const float *B, float *C,
                                 int M, int N, int K) {
    if (!cuda_initialized) {
        if (kyjit_cublas_init() != 0) {
            return kyjit_blas_matmul_general(A, B, C, M, N, K);
        }
    }
    
    float *d_A, *d_B, *d_C;
    cudaMalloc(&d_A, M*K*sizeof(float));
    cudaMalloc(&d_B, K*N*sizeof(float));
    cudaMalloc(&d_C, M*N*sizeof(float));
    
    cudaMemcpy(d_A, A, M*K*sizeof(float), cudaMemcpyHostToDevice);
    cudaMemcpy(d_B, B, K*N*sizeof(float), cudaMemcpyHostToDevice);
    
    const float alpha = 1.0f, beta = 0.0f;
    cublasSgemm(cublas_handle,
                CUBLAS_OP_N, CUBLAS_OP_N,
                N, M, K,
                &alpha,
                d_B, N,
                d_A, K,
                &beta,
                d_C, N);
    
    cudaMemcpy(C, d_C, M*N*sizeof(float), cudaMemcpyDeviceToHost);
    
    cudaFree(d_A); cudaFree(d_B); cudaFree(d_C);
    return 0;
}

/* Check if CUDA GPU is available */
int kyjit_cuda_available(void) {
    int device_count = 0;
    cudaError_t err = cudaGetDeviceCount(&device_count);
    return (err == cudaSuccess && device_count > 0);
}

/* Cleanup */
void kyjit_cublas_cleanup(void) {
    if (cuda_initialized) {
        cublasDestroy(cublas_handle);
        cuda_initialized = 0;
    }
}

#else
/* NO CUDA — Stub versions that fallback to OpenBLAS */

int kyjit_cublas_init(void) { return -1; }

int kyjit_cublas_matmul(const float *A, const float *B, float *C, int N) {
    return kyjit_blas_matmul_jit(A, B, C, N);
}

int kyjit_cublas_matmul_general(const float *A, const float *B, float *C,
                                 int M, int N, int K) {
    return kyjit_blas_matmul_general(A, B, C, M, N, K);
}

int kyjit_cuda_available(void) { return 0; }

void kyjit_cublas_cleanup(void) {}

#endif /* KYJIT_HAVE_CUDA */
