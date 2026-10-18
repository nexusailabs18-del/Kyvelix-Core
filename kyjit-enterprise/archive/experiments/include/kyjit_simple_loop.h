#ifndef KYJIT_SIMPLE_LOOP_H
#define KYJIT_SIMPLE_LOOP_H
#include <stdint.h>
#include <immintrin.h>

long long kyjit_simple_loop_sum(const int32_t *data, int n);
long long kyjit_simple_loop_batch(const int32_t *data, int batch_count, int n);

#endif
