#include "kernels.h"

void kernel_copy(const double* restrict input, double* restrict output, long n) {
    for (long i = 0; i < n; i++) output[i] = input[i];
}