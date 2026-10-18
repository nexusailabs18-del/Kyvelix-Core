#ifndef KYJIT_PLATFORM_H
#define KYJIT_PLATFORM_H

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>

typedef enum {
    KY_BACKEND_CPU_X86_AVX2 = 0,
    KY_BACKEND_CPU_X86_AVX512 = 1,
    KY_BACKEND_CPU_ARM_NEON = 2,
    KY_BACKEND_CPU_RISCV = 3,
    KY_BACKEND_CUDA = 10,
    KY_BACKEND_OPENCL = 11,
    KY_BACKEND_NONE = 99
} KyBackendType;

typedef struct KyPlatform {
    KyBackendType backend;
    uint32_t num_cores;
    uint32_t cache_line_size;
    uint32_t simd_width;
    const char *vendor;
    const char *arch;
} KyPlatform;

static inline KyBackendType ky_detect_backend(void) {
#if defined(__x86_64__) || defined(_M_X64)
    #ifdef __AVX512F__
    return KY_BACKEND_CPU_X86_AVX512;
    #else
    return KY_BACKEND_CPU_X86_AVX2;
    #endif
#elif defined(__aarch64__) || defined(_M_ARM64)
    return KY_BACKEND_CPU_ARM_NEON;
#elif defined(__riscv)
    return KY_BACKEND_CPU_RISCV;
#else
    return KY_BACKEND_NONE;
#endif
}

static inline int ky_cuda_available(void) {
    const char *cuda_paths[] = {
        "/usr/local/cuda/lib64/libcudart.so",
        "/usr/lib/x86_64-linux-gnu/libcudart.so",
        "/opt/cuda/lib64/libcudart.so",
        NULL
    };
    for (int i = 0; cuda_paths[i]; i++) {
        FILE *f = fopen(cuda_paths[i], "r");
        if (f) { fclose(f); return 1; }
    }
    const char *cuda_home = getenv("CUDA_HOME");
    return cuda_home != NULL;
}

static inline KyPlatform ky_platform_init(void) {
    KyPlatform p;
    p.backend = ky_detect_backend();
    p.num_cores = (uint32_t)sysconf(_SC_NPROCESSORS_ONLN);
    if (p.num_cores == 0) p.num_cores = 1;
    p.cache_line_size = 64;
    #ifdef __AVX512F__
    p.simd_width = 64;
    #elif defined(__AVX2__)
    p.simd_width = 32;
    #elif defined(__ARM_NEON)
    p.simd_width = 16;
    #else
    p.simd_width = 8;
    #endif
    #if defined(__x86_64__)
    p.vendor = "Intel/AMD x86-64";
    p.arch = "x86_64";
    #elif defined(__aarch64__)
    p.vendor = "ARM";
    p.arch = "aarch64";
    #elif defined(__riscv)
    p.vendor = "RISC-V";
    p.arch = "riscv";
    #else
    p.vendor = "Unknown";
    p.arch = "unknown";
    #endif
    return p;
}

#endif
