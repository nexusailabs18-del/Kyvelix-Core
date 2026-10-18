#include "kyjit_batch_exec.h"
#include "kyjit_thread_pool.h"
#include <stdlib.h>

static KyThreadPool *g_pool = NULL;

int kyjit_batch_init(int num_threads) {
    if (g_pool) return 0;
    g_pool = kyjit_pool_create(num_threads);
    return g_pool ? 0 : -1;
}

int kyjit_batch_exec(KyBatchFunc *funcs, void **args, int num_tasks) {
    if (!g_pool || !funcs || !args || num_tasks <= 0) return -1;
    for (int i = 0; i < num_tasks; i++) {
        if (kyjit_pool_submit(g_pool, funcs[i], args[i]) != 0)
            return -1;
    }
    return 0;
}

void kyjit_batch_wait(void) {
    if (g_pool) kyjit_pool_wait(g_pool);
}

void kyjit_batch_destroy(void) {
    if (g_pool) {
        kyjit_pool_destroy(g_pool);
        g_pool = NULL;
    }
}
/* Multi-threaded INT8 FMA with pthreads */
#include <pthread.h>

typedef struct {
    int8_t *a, *b, *c;
    int start, end;
} Int8Arg;

static void* int8_fma_worker(void *arg) {
    Int8Arg *ta = (Int8Arg*)arg;
    int i = ta->start;
    for(; i+4096 <= ta->end; i+=4096) {
        #pragma GCC unroll 128
        for(int j=0;j<4096;j+=32) {
            __m256i va = _mm256_loadu_si256((__m256i*)&ta->a[i+j]);
            __m256i vb = _mm256_loadu_si256((__m256i*)&ta->b[i+j]);
            _mm256_storeu_si256((__m256i*)&ta->c[i+j], _mm256_add_epi8(va, vb));
        }
    }
    for(; i+32 <= ta->end; i+=32) {
        __m256i va = _mm256_loadu_si256((__m256i*)&ta->a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&ta->b[i]);
        _mm256_storeu_si256((__m256i*)&ta->c[i], _mm256_add_epi8(va, vb));
    }
    return NULL;
}

void kyjit_int8_fma_mt(int8_t *a, int8_t *b, int8_t *c, int n, int num_threads) {
    pthread_t threads[32];
    Int8Arg args[32];
    int chunk = n / num_threads;
    for(int t=0;t<num_threads;t++){
        args[t].a = a; args[t].b = b; args[t].c = c;
        args[t].start = t*chunk;
        args[t].end = (t==num_threads-1) ? n : (t+1)*chunk;
        pthread_create(&threads[t], NULL, int8_fma_worker, &args[t]);
    }
    for(int t=0;t<num_threads;t++) pthread_join(threads[t], NULL);
}
