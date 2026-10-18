#include <stdlib.h>
#include "kyjit_lockfree.h"

KyLockFreeQueue* kyjit_lfq_create(void) {
    KyLockFreeQueue *q = calloc(1, sizeof(KyLockFreeQueue));
    atomic_init(&q->head, 0);
    atomic_init(&q->tail, 0);
    return q;
}

int kyjit_lfq_push(KyLockFreeQueue *q, void *item) {
    int tail = atomic_load_explicit(&q->tail, memory_order_relaxed);
    int next = (tail + 1) % KYJIT_QUEUE_SIZE;
    if (next == atomic_load_explicit(&q->head, memory_order_acquire)) {
        return -1;
    }
    q->items[tail] = item;
    atomic_store_explicit(&q->tail, next, memory_order_release);
    return 0;
}

int kyjit_lfq_pop(KyLockFreeQueue *q, void **item) {
    int head = atomic_load_explicit(&q->head, memory_order_relaxed);
    if (head == atomic_load_explicit(&q->tail, memory_order_acquire)) {
        return -1;
    }
    *item = q->items[head];
    atomic_store_explicit(&q->head, (head + 1) % KYJIT_QUEUE_SIZE, memory_order_release);
    return 0;
}

void kyjit_lfq_destroy(KyLockFreeQueue *q) {
    free(q);
}
