#include "kyjit_tasks.h"
#include <stdlib.h>
#include <pthread.h>
#include <stdatomic.h>
#include <unistd.h>

#define MAX_TASKS  4096
#define MAX_WORKERS 64

typedef struct {
    KyTaskFn fn;
    void    *arg;
} Task;

static Task task_queue[MAX_TASKS];
static atomic_int task_head = 0;   /* next to execute */
static atomic_int task_tail = 0;   /* next to insert */
static int task_count = 0;

static pthread_t workers[MAX_WORKERS];
static int num_workers = 0;
static atomic_int shutdown_flag = 0;
static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t  cond  = PTHREAD_COND_INITIALIZER;
static pthread_cond_t  done_cond = PTHREAD_COND_INITIALIZER;

static void *worker_loop(void *arg) {
    (void)arg;

    for (;;) {
        pthread_mutex_lock(&mutex);

        /* Wait for work */
        while (atomic_load(&task_head) >= atomic_load(&task_tail) &&
               !atomic_load(&shutdown_flag)) {
            pthread_cond_wait(&cond, &mutex);
        }

        if (atomic_load(&shutdown_flag) &&
            atomic_load(&task_head) >= atomic_load(&task_tail)) {
            pthread_mutex_unlock(&mutex);
            break;
        }

        /* Take one task */
        int idx = atomic_fetch_add(&task_head, 1);
        if (idx >= atomic_load(&task_tail)) {
            /* Race — no actual task */
            pthread_mutex_unlock(&mutex);
            continue;
        }

        Task t = task_queue[idx % MAX_TASKS];
        pthread_mutex_unlock(&mutex);

        /* Execute */
        if (t.fn) t.fn(t.arg);

        /* Signal completion */
        pthread_mutex_lock(&mutex);
        task_count--;
        if (task_count == 0) pthread_cond_broadcast(&done_cond);
        pthread_mutex_unlock(&mutex);
    }
    return NULL;
}

int kyjit_tasks_init(int n) {
    if (num_workers > 0) return 0;  /* already initialized */

    if (n <= 0) {
        n = (int)sysconf(_SC_NPROCESSORS_ONLN);
        if (n < 1) n = 1;
    }
    if (n > MAX_WORKERS) n = MAX_WORKERS;

    atomic_init(&task_head, 0);
    atomic_init(&task_tail, 0);
    atomic_init(&shutdown_flag, 0);

    for (int i = 0; i < n; i++) {
        if (pthread_create(&workers[i], NULL, worker_loop, NULL) != 0) {
            return -1;
        }
    }
    num_workers = n;
    return 0;
}

int kyjit_task_submit(KyTaskFn fn, void *arg) {
    pthread_mutex_lock(&mutex);
    if (task_count >= MAX_TASKS) {
        pthread_mutex_unlock(&mutex);
        return -1;
    }

    int idx = atomic_fetch_add(&task_tail, 1);
    task_queue[idx % MAX_TASKS].fn = fn;
    task_queue[idx % MAX_TASKS].arg = arg;
    task_count++;

    pthread_cond_signal(&cond);
    pthread_mutex_unlock(&mutex);
    return 0;
}

void kyjit_tasks_wait(void) {
    pthread_mutex_lock(&mutex);
    while (task_count > 0) {
        pthread_cond_wait(&done_cond, &mutex);
    }
    pthread_mutex_unlock(&mutex);
}

void kyjit_tasks_shutdown(void) {
    atomic_store(&shutdown_flag, 1);
    pthread_mutex_lock(&mutex);
    pthread_cond_broadcast(&cond);
    pthread_mutex_unlock(&mutex);

    for (int i = 0; i < num_workers; i++) {
        pthread_join(workers[i], NULL);
    }
    num_workers = 0;
}

int kyjit_tasks_worker_count(void) {
    return num_workers;
}

int kyjit_tasks_pending(void) {
    pthread_mutex_lock(&mutex);
    int p = task_count;
    pthread_mutex_unlock(&mutex);
    return p;
}
