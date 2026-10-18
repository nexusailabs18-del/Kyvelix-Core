#ifndef KYJIT_LOCKFREE_H
#define KYJIT_LOCKFREE_H

#include <stdatomic.h>
#include <stddef.h>

#define KYJIT_QUEUE_SIZE 1024

typedef struct KyLockFreeQueue {
    _Atomic int head;
    _Atomic int tail;
    void *items[KYJIT_QUEUE_SIZE];
} KyLockFreeQueue;

KyLockFreeQueue* kyjit_lfq_create(void);
int kyjit_lfq_push(KyLockFreeQueue *q, void *item);
int kyjit_lfq_pop(KyLockFreeQueue *q, void **item);
void kyjit_lfq_destroy(KyLockFreeQueue *q);

#endif
