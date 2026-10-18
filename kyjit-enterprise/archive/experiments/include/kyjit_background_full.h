#ifndef KYJIT_BACKGROUND_FULL_H
#define KYJIT_BACKGROUND_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include <pthread.h>

typedef struct KyCompileJobFull {
    void *input;
    void *output;
    int (*compile_fn)(void*, void*);
    int done;
} KyCompileJobFull;

typedef struct {
    KyCompileJobFull *jobs;
    uint32_t count;
    uint32_t capacity;
    pthread_t worker;
    pthread_mutex_t mutex;
    pthread_cond_t cond;
    int running;
} KyBackgroundCompileFull;

static inline KyBackgroundCompileFull *ky_bg_full_create(uint32_t cap) {
    KyBackgroundCompileFull *bg = calloc(1, sizeof(*bg));
    if (!bg) return NULL;
    bg->jobs = calloc(cap, sizeof(KyCompileJobFull));
    bg->capacity = cap;
    pthread_mutex_init(&bg->mutex, NULL);
    pthread_cond_init(&bg->cond, NULL);
    bg->running = 1;
    return bg;
}

static inline void *ky_bg_worker(void *arg) {
    KyBackgroundCompileFull *bg = arg;
    while (bg->running) {
        pthread_mutex_lock(&bg->mutex);
        while (bg->count == 0 && bg->running) pthread_cond_wait(&bg->cond, &bg->mutex);
        if (!bg->running) { pthread_mutex_unlock(&bg->mutex); break; }
        KyCompileJobFull job = bg->jobs[--bg->count];
        pthread_mutex_unlock(&bg->mutex);
        if (job.compile_fn) job.compile_fn(job.input, job.output);
        pthread_mutex_lock(&bg->mutex);
        job.done = 1;
        pthread_mutex_unlock(&bg->mutex);
    }
    return NULL;
}

static inline void ky_bg_full_start(KyBackgroundCompileFull *bg) {
    pthread_create(&bg->worker, NULL, ky_bg_worker, bg);
}

static inline void ky_bg_full_submit(KyBackgroundCompileFull *bg, void *in, void *out,
                                     int (*fn)(void*, void*)) {
    pthread_mutex_lock(&bg->mutex);
    if (bg->count < bg->capacity) {
        bg->jobs[bg->count].input = in;
        bg->jobs[bg->count].output = out;
        bg->jobs[bg->count].compile_fn = fn;
        bg->jobs[bg->count].done = 0;
        bg->count++;
        pthread_cond_signal(&bg->cond);
    }
    pthread_mutex_unlock(&bg->mutex);
}

static inline void ky_bg_full_stop(KyBackgroundCompileFull *bg) {
    pthread_mutex_lock(&bg->mutex);
    bg->running = 0;
    pthread_cond_broadcast(&bg->cond);
    pthread_mutex_unlock(&bg->mutex);
    pthread_join(bg->worker, NULL);
}
#endif
