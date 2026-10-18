#ifndef KYJIT_CACHE_BLOCK_H
#define KYJIT_CACHE_BLOCK_H

#ifdef __cplusplus
extern "C" {
#endif

/* Cache-blocked matrix multiply: C = A * B (row-major).
 * A is M x K, B is K x N, C is M x N.
 * Uses block sizes tuned for L2/L3 cache.
 * Returns 0 on success, -1 on error.
 */
int kyjit_cache_block_matmul(const float *A, const float *B, float *C,
                             int M, int K, int N);

/* Cache-blocked vector sum reduction over an array.
 * Returns the sum of all elements.
 */
float kyjit_cache_block_sum(const float *arr, int n);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_CACHE_BLOCK_H */