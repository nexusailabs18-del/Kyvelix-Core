#include <stddef.h>
#include <omp.h>

void kernel_strided_copy(const double *restrict  a, double *restrict  b, long n, long stride)
{
    #pragma omp parallel for num_threads(4) schedule(static)
    for (long i = 0; i < n; i++) {
        b[i] = a[i * stride];
    }
}
