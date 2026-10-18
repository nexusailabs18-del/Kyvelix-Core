#include "kyjit_thread_pool.h"
#include "kyjit_work_steal.h"
#include <pthread.h>
#include <stdlib.h>
#include <stdatomic.h>
#include <sched.h>

#define MAX_TASKS 65536

typedef struct {
    void (*func)(void*);
    void *arg;
} KyTask;

struct KyThreadPool {
    pthread_t *threads;
    int thread_count;
    atomic_int task_count;
    atomic_int task_head;
    atomic_int task_tail;
    KyTask tasks[MAX_TASKS];
    _Atomic int stop;
    KyWorkSteal **work_steal;   /* one deque per thread */
};

static void *kyjit_worker(void *arg) {
    KyThreadPool *pool = (KyThreadPool*)arg;
    int my_id = 0;
    for (int i = 0; i < pool->thread_count; i++) {
        if (pthread_equal(pthread_self(), pool->threads[i])) {
            my_id = i;
            break;
        }
    }

    while (1) {
        int head = atomic_load_explicit(&pool->task_head, memory_order_relaxed);
        int tail = atomic_load_explicit(&pool->task_tail, memory_order_acquire);
        if (head < tail) {
            if (atomic_compare_exchange_weak_explicit(
                    &pool->task_head, &head, head + 1,
                    memory_order_release, memory_order_relaxed)) {
                KyTask task = pool->tasks[head % MAX_TASKS];
                task.func(task.arg);
                atomic_fetch_sub_explicit(&pool->task_count, 1, memory_order_release);
                continue;
            }
        }

        /* Try to steal from other threads */
        int stolen = 0;
        for (int i = 1; i < pool->thread_count; i++) {
            int victim = (my_id + i) % pool->thread_count;
            void *item = NULL;
            if (kyjit_ws_steal(pool->work_steal[victim], &item) == 0) {
                /* item is a KyTask* */
                KyTask *task = (KyTask*)item;
                task->func(task->arg);
                atomic_fetch_sub_explicit(&pool->task_count, 1, memory_order_release);
                stolen = 1;
                break;
            }
        }

        if (!stolen) {
            if (atomic_load_explicit(&pool->stop, memory_order_relaxed))
                return NULL;
            sched_yield();
        }
    }
    return NULL;
}

KyThreadPool *kyjit_pool_create(int num_threads) {
    if (num_threads <= 0) return NULL;
    KyThreadPool *pool = calloc(1, sizeof(KyThreadPool));
    if (!pool) return NULL;
    pool->thread_count = num_threads;
    pool->threads = malloc(num_threads * sizeof(pthread_t));
    pool->work_steal = malloc(num_threads * sizeof(KyWorkSteal*));
    if (!pool->threads || !pool->work_steal) {
        free(pool->threads);
        free(pool->work_steal);
        free(pool);
        return NULL;
    }
    atomic_init(&pool->task_count, 0);
    atomic_init(&pool->task_head, 0);
    atomic_init(&pool->task_tail, 0);
    pool->stop = 0;
    for (int i = 0; i < num_threads; i++) {
        pool->work_steal[i] = kyjit_ws_create(1024);
    }
    for (int i = 0; i < num_threads; i++) {
        pthread_create(&pool->threads[i], NULL, kyjit_worker, pool);
    }
    return pool;
}

int kyjit_pool_submit(KyThreadPool *pool, void (*func)(void*), void *arg) {
    if (!pool || !func) return -1;
    int tail = atomic_load_explicit(&pool->task_tail, memory_order_relaxed);
    int head = atomic_load_explicit(&pool->task_head, memory_order_acquire);
    if (tail - head >= MAX_TASKS) return -1;
    pool->tasks[tail % MAX_TASKS].func = func;
    pool->tasks[tail % MAX_TASKS].arg = arg;
    atomic_fetch_add_explicit(&pool->task_count, 1, memory_order_release);
    atomic_store_explicit(&pool->task_tail, tail + 1, memory_order_release);
    return 0;
}

void kyjit_pool_wait(KyThreadPool *pool) {
    if (!pool) return;
    while (atomic_load_explicit(&pool->task_count, memory_order_acquire) > 0) {
        sched_yield();
    }
}

void kyjit_pool_destroy(KyThreadPool *pool) {
    if (!pool) return;
    atomic_store(&pool->stop, 1);
    for (int i = 0; i < pool->thread_count; i++) {
        pthread_join(pool->threads[i], NULL);
        kyjit_ws_destroy(pool->work_steal[i]);
    }
    free(pool->threads);
    free(pool->work_steal);
    free(pool);
}
