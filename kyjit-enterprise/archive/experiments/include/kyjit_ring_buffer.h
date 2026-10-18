#ifndef KYJIT_RING_BUFFER_H
#define KYJIT_RING_BUFFER_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque ring buffer handle */
typedef struct KyRingBuffer KyRingBuffer;

/* Create a lock-free SPSC ring buffer with `capacity` items.
 * capacity must be a power of two.
 * Returns NULL on failure.
 */
KyRingBuffer *kyjit_ring_create(size_t capacity);

/* Try to push an item. Returns 0 on success, -1 if full. */
int kyjit_ring_push(KyRingBuffer *rb, void *item);

/* Try to pop an item. Returns 0 on success, -1 if empty. */
int kyjit_ring_pop(KyRingBuffer *rb, void **item);

/* Destroy the ring buffer. */
void kyjit_ring_destroy(KyRingBuffer *rb);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_RING_BUFFER_H */