#ifndef KYJIT_STREAM_H
#define KYJIT_STREAM_H
#include <immintrin.h>
void kyjit_stream_copy_optimized(const int32_t *restrict src, int32_t *restrict dst, int n);
void kyjit_stream_int8_optimized(const int8_t *restrict src, int8_t *restrict dst, int n);
#endif
