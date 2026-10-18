#ifndef KYJIT_GPU_H
#define KYJIT_GPU_H
#include <stdint.h>

int kyjit_gpu_init(void);
int kyjit_gpu_available(void);
int kyjit_gpu_mfr(const int32_t *data, int n, long long *result);
void kyjit_gpu_cleanup(void);

#endif
