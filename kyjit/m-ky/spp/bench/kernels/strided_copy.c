#include "kernels.h"

void kernel_strided_copy(const double* restrict input, double* restrict output, long n, long stride) {
    for (long i = 0; i < n; i++) output[i] = input[i * stride];
}