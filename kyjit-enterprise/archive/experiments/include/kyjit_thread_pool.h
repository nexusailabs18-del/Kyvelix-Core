#ifndef KYJIT_THREAD_POOL_H
#define KYJIT_THREAD_POOL_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque thread pool handle */
typedef struct KyThreadPool KyThreadPool;

/* Create a thread pool with `num_threads` persistent workers.
 * Returns NULL on failure.
 */
KyThreadPool *kyjit_pool_create(int num_threads);

/* Submit a task to the pool.
 * The task will be executed by one of the workers.
 * Returns 0 on success, -1 if the pool is full or invalid.
 */
int kyjit_pool_submit(KyThreadPool *pool, void (*func)(void*), void *arg);

/* Wait until all submitted tasks have completed. */
void kyjit_pool_wait(KyThreadPool *pool);

/* Destroy the pool and free resources. */
void kyjit_pool_destroy(KyThreadPool *pool);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_THREAD_POOL_H */