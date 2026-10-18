#ifndef KYJIT_CUDA_H
#define KYJIT_CUDA_H

#include <stdint.h>
#include <stdlib.h>
#include "kyjit_platform.h"

// CUDA backend (lazy-loaded to avoid hard dependency)
typedef struct KyCUDABackend {
    int available;
    void *cuda_lib;           // dlopen handle
    void *cuda_context;
    void *cuda_module;
    void *cuda_function;
    uint32_t device_count;
    uint32_t active_device;
} KyCUDABackend;

// Check if CUDA is available
static inline int ky_cuda_init(KyCUDABackend *cuda) {
    if (!cuda) return -1;
    cuda->available = ky_cuda_available();
    if (!cuda->available) return -1;
    // In production: dlopen libcudart.so, get device count, etc.
    // For now: just mark available
    cuda->device_count = 1; // Assume 1 GPU
    return 0;
}

// Check GPU capabilities
static inline void ky_cuda_get_info(KyCUDABackend *cuda, uint32_t *compute_capability, uint32_t *memory_gb) {
    if (!cuda || !cuda->available) {
        if (compute_capability) *compute_capability = 0;
        if (memory_gb) *memory_gb = 0;
        return;
    }
    // H100: compute capability 9.0, 80GB HBM3
    // A100: compute capability 8.0, 40/80GB
    // For now: return defaults
    if (compute_capability) *compute_capability = 90; // 9.0
    if (memory_gb) *memory_gb = 80;
}

#endif
