#ifndef KYJIT_BATCH_OPTIMIZED_H
#define KYJIT_BATCH_OPTIMIZED_H

#ifdef __cplusplus
extern "C" {
#endif

/* Batch fused multiply-add over multiple arrays using AVX2 16 accumulators.
 * arrays: array of pointers to float (a)
 * scalars: array of scalars (one per task)
 * b_arrays: array of pointers to float (b)
 * c_arrays: array of pointers to float (output)
 * counts: array of element counts per task
 * num_tasks: number of tasks
 * Returns 0 on success, -1 on error.
 */
int kyjit_batch_fma_16acc(const float **arrays, const float *scalars,
                          const float **b_arrays, float **c_arrays,
                          const int *counts, int num_tasks);

#ifdef __cplusplus
}
#endif
#endif
