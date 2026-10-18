#include "kyjit_ring_buffer.h"
#include <stdlib.h>
#include <stdatomic.h>
#include <string.h>

struct KyRingBuffer {
    size_t capacity;
    size_t mask;
    atomic_size_t head;   /* producer index */
    atomic_size_t tail;   /* consumer index */
    void **items;
};

KyRingBuffer *kyjit_ring_create(size_t capacity) {
    /* Ensure capacity is power of two */
    if (capacity == 0 || (capacity & (capacity - 1)) != 0)
        return NULL;

    KyRingBuffer *rb = (KyRingBuffer*)malloc(sizeof(KyRingBuffer));
    if (!rb) return NULL;

    rb->capacity = capacity;
    rb->mask = capacity - 1;
    rb->items = (void**)malloc(capacity * sizeof(void*));
    if (!rb->items) {
        free(rb);
        return NULL;
    }

    atomic_init(&rb->head, 0);
    atomic_init(&rb->tail, 0);
    return rb;
}

int kyjit_ring_push(KyRingBuffer *rb, void *item) {
    if (!rb) return -1;
    size_t head = atomic_load_explicit(&rb->head, memory_order_relaxed);
    size_t tail = atomic_load_explicit(&rb->tail, memory_order_acquire);
    if (head - tail >= rb->capacity)
        return -1; /* full */

    rb->items[head & rb->mask] = item;
    atomic_store_explicit(&rb->head, head + 1, memory_order_release);
    return 0;
}

int kyjit_ring_pop(KyRingBuffer *rb, void **item) {
    if (!rb || !item) return -1;
    size_t tail = atomic_load_explicit(&rb->tail, memory_order_relaxed);
    size_t head = atomic_load_explicit(&rb->head, memory_order_acquire);
    if (tail == head)
        return -1; /* empty */

    *item = rb->items[tail & rb->mask];
    atomic_store_explicit(&rb->tail, tail + 1, memory_order_release);
    return 0;
}

void kyjit_ring_destroy(KyRingBuffer *rb) {
    if (!rb) return;
    free(rb->items);
    free(rb);
}