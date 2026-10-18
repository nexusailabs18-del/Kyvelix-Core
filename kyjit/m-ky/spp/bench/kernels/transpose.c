#include "kernels.h"

void kernel_transpose(const double* restrict input, double* restrict output, long m, long n) {
    for (long i = 0; i < m; i++) {
        for (long j = 0; j < n; j++) {
            output[j * m + i] = input[i * n + j];
        }
    }
}