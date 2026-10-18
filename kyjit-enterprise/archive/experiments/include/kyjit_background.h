#ifndef KYJIT_BACKGROUND_H
#define KYJIT_BACKGROUND_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include <pthread.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// Background compile job
typedef struct KyCompileJob {
    void *input;
    void *output;
    int (*compile_fn)(void *input, void *output);
    int done;
    pthread_t thread;
} KyCompileJob;

// Background compiler
typedef struct KyBackgroundCompiler {
    KyCompileJob *jobs;
    uint32_t job_count;
    uint32_t max_jobs;
    pthread_mutex_t mutex;
} KyBackgroundCompiler;

static inline KyBackgroundCompiler *ky_bg_create(uint32_t max_jobs) {
    KyBackgroundCompiler *bg = calloc(1, sizeof(KyBackgroundCompiler));
    if (!bg) return NULL;
    bg->jobs = calloc(max_jobs, sizeof(KyCompileJob));
    if (!bg->jobs) { free(bg); return NULL; }
    bg->max_jobs = max_jobs;
    pthread_mutex_init(&bg->mutex, NULL);
    return bg;
}

// Submit compilation job (runs synchronously for now)
static inline int ky_bg_submit(KyBackgroundCompiler *bg, void *input, void *output,
                                int (*compile_fn)(void *, void *)) {
    if (!bg || !compile_fn) return -1;
    pthread_mutex_lock(&bg->mutex);
    if (bg->job_count >= bg->max_jobs) {
        pthread_mutex_unlock(&bg->mutex);
        return -1;
    }
    uint32_t idx = bg->job_count++;
    bg->jobs[idx].input = input;
    bg->jobs[idx].output = output;
    bg->jobs[idx].compile_fn = compile_fn;
    pthread_mutex_unlock(&bg->mutex);
    
    // Run synchronously for now (threading can be added later)
    return compile_fn(input, output);
}
#endif
