
#include "kernels.h"
#include <stdlib.h>

int main(int argc, char** argv) {
    long N = 8192;
    long M = 64, K = 64;
    long stride = 4;
    if (argc > 1) N = atol(argv[1]);
    if (argc > 2) M = atol(argv[2]);
    if (argc > 3) stride = atol(argv[3]);

    double* input = (double*)aligned_alloc(64, N * 16 * sizeof(double));
    double* output = (double*)aligned_alloc(64, N * sizeof(double));
    double* A = (double*)aligned_alloc(64, 64 * 64 * sizeof(double));
    double* B = (double*)aligned_alloc(64, 64 * 64 * sizeof(double));
    double* C = (double*)aligned_alloc(64, 64 * 64 * sizeof(double));

    for (long i = 0; i < N * 16; i++) input[i] = (double)rand() / RAND_MAX * 2.0 - 1.0;
    for (long i = 0; i < N; i++) output[i] = 0.0;
    for (long i = 0; i < 64 * 64; i++) A[i] = (double)rand() / RAND_MAX * 2.0 - 1.0;
    for (long i = 0; i < 64 * 64; i++) B[i] = (double)rand() / RAND_MAX * 2.0 - 1.0;
    for (long i = 0; i < 64 * 64; i++) C[i] = 0.0;

    kernel_strided_copy(input, output, N, stride);

    free(input);
    free(output);
    free(A);
    free(B);
    free(C);
    return 0;
}
