#ifndef KYJIT_WORK_STEAL_H
#define KYJIT_WORK_STEAL_H

#include <stddef.h>
#include <stdatomic.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque work-stealing deque */
typedef struct KyWorkSteal KyWorkSteal;

/* Create a work-stealing deque with capacity `capacity`.
 * capacity must be a power of two.
 * Returns NULL on failure.
 */
KyWorkSteal *kyjit_ws_create(size_t capacity);

/* Push a task to the local deque (called by owner thread).
 * Returns 0 on success, -1 if full.
 */
int kyjit_ws_push(KyWorkSteal *ws, void *item);

/* Pop a task from the local deque (called by owner thread).
 * Returns 0 on success, -1 if empty.
 */
int kyjit_ws_pop(KyWorkSteal *ws, void **item);

/* Steal a task from the deque (called by any thief thread).
 * Returns 0 on success, -1 if empty or racing.
 */
int kyjit_ws_steal(KyWorkSteal *ws, void **item);

/* Destroy the deque. */
void kyjit_ws_destroy(KyWorkSteal *ws);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_WORK_STEAL_H */