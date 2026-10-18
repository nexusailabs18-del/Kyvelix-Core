#include "kernels.h"

void kernel_gemm(const double* restrict A, const double* restrict B, double* restrict C,
                 long M, long N, long K) {
    for (long i = 0; i < M; i++) {
        for (long j = 0; j < N; j++) {
            double sum = 0.0;
            for (long k = 0; k < K; k++) {
                sum += A[i * K + k] * B[k * N + j];
            }
            C[i * N + j] = sum;
        }
    }
}