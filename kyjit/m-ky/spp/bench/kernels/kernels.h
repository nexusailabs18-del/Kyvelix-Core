#ifndef KERNELS_H
#define KERNELS_H

#include <stddef.h>

void kernel_copy(const double* restrict a, double* restrict b, long n);
void kernel_strided_copy(const double* restrict a, double* restrict b, long n, long stride);
void kernel_transpose(const double* restrict a, double* restrict b, long rows, long cols);
void kernel_gemm(const double* restrict A, const double* restrict B, double* restrict C,
                 long M, long N, long K);

#endif