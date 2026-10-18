#ifndef KYJIT_GPU_DETECT_H
#define KYJIT_GPU_DETECT_H

int kyjit_gpu_has_cuda(void);
int kyjit_gpu_has_rocm(void);
int kyjit_gpu_has_intel(void);
const char* kyjit_gpu_name(void);
const char* kyjit_gpu_best_path(void);
void kyjit_gpu_info(void);
void kyjit_hardware_info(void);

#endif
