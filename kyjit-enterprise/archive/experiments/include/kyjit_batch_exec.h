#ifndef KYJIT_BATCH_EXEC_H
#define KYJIT_BATCH_EXEC_H

#include <stdint.h>
#include <immintrin.h>

typedef void (*KyBatchFunc)(void*);
int kyjit_batch_init(int num_threads);
int kyjit_batch_exec(KyBatchFunc *funcs, void **args, int num_tasks);
void kyjit_batch_wait(void);
void kyjit_batch_destroy(void);
void kyjit_int8_fma_mt(int8_t *a, int8_t *b, int8_t *c, int n, int num_threads);

#endif
