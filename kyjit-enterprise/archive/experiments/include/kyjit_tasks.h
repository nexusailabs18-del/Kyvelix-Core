#ifndef KYJIT_TASKS_H
#define KYJIT_TASKS_H

#include <stddef.h>

typedef void (*KyTaskFn)(void *arg);

/* Initialize task system with N workers (0 = auto). */
int  kyjit_tasks_init(int num_workers);

/* Submit a task. Returns 0 on success, -1 if queue full. */
int  kyjit_task_submit(KyTaskFn fn, void *arg);

/* Wait for all submitted tasks to complete. */
void kyjit_tasks_wait(void);

/* Shutdown the task system. */
void kyjit_tasks_shutdown(void);

/* Stats */
int   kyjit_tasks_worker_count(void);
int   kyjit_tasks_pending(void);

#endif
