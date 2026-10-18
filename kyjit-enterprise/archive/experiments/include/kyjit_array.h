#ifndef KYJIT_ARRAY_H
#define KYJIT_ARRAY_H

#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <immintrin.h>

typedef struct {
    int32_t *data;
    size_t length;
    size_t capacity;
} KyArray;

void ky_array_free(KyArray *arr);
long long kyjit_array_sum_avx2(const int32_t *data, int n);
long long kyjit_array_sum_restrict(const int32_t *restrict data, int n);
void kyjit_array_stream_add(const int32_t *restrict a,
                            const int32_t *restrict b,
                            int32_t *restrict c, int n);

#endif
