#include <stddef.h>
#include <stdint.h>
#ifndef KYJIT_SVM_FUSION_H
#define KYJIT_SVM_FUSION_H

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------------- */
/* Hardware-limit optimized SVM fusion API                                   */
/* Shared Virtual Memory + fused operations + async pipelines                */
/* ------------------------------------------------------------------------- */

/* Initialize SVM fusion engine.
 * Tries GPU (OpenCL SVM/zero-copy) first; if unavailable, enables AVX2 CPU
 * backend. Returns 0 on success, -1 on failure.
 */
int kyjit_svm_init(void);

/* Check if GPU zero-copy fusion is active. */
int kyjit_svm_gpu_active(void);

/* Pin host memory for zero-copy access (optional optimization).
 * For large buffers, pinning reduces page faults during GPU access.
 * Returns 0 on success, -1 on failure.
 */
int kyjit_svm_pin(void *ptr, size_t size);

/* Unpin previously pinned host memory. */
void kyjit_svm_unpin(void *ptr);

/* Fused multiply-add: c[i] = a[i] * scalar + b[i].
 * Uses GPU SVM if available; otherwise AVX2 CPU fallback.
 * Returns 0 on success, -1 on error.
 */
int kyjit_svm_fma(const float *a, float scalar, const float *b, float *c, int n);

/* Fused scale-add: c[i] = a[i] * sa + b[i] * sb.
 * Same performance characteristics as kyjit_svm_fma.
 * Returns 0 on success, -1 on error.
 */
int kyjit_svm_scale_add(const float *a, float sa, const float *b, float sb, float *c, int n);

/* Batched fused multiply-add over multiple arrays.
 * This submits all operations asynchronously to keep the GPU pipeline full,
 * then waits once at the end. Amortizes kernel launch overhead.
 *
 * arrays: array of pointers to const float arrays (a)
 * scalars: array of float scalars (one per task)
 * b_arrays: array of pointers to const float arrays (b)
 * c_arrays: array of pointers to output float arrays
 * counts: array of element counts per task
 * num_tasks: number of fused operations
 *
 * Returns 0 on success, -1 on error.
 */
int kyjit_svm_fma_batch(const float **arrays, const float *scalars,
                        const float **b_arrays, float **c_arrays,
                        const int *counts, int num_tasks);

/* Wait for all previously submitted asynchronous SVM operations to complete.
 * Use after batch submissions to synchronize.
 */
void kyjit_svm_sync(void);

/* INT8 fused add: c[i] = a[i] + b[i] (8-bit, 4x less memory traffic).
 * Returns 0 on success, -1 on error.
 */
int kyjit_svm_fma_int8(const int8_t *a, const int8_t *b, int8_t *c, int n);

/* Cleanup SVM resources. */
void kyjit_svm_cleanup(void);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_SVM_FUSION_H */