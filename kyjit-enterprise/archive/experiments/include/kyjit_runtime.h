#ifndef KYJIT_RUNTIME_H
#define KYJIT_RUNTIME_H

#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include "kyjit_platform.h"
#include "kyjit_gpu.h"

typedef struct KyRuntime {
    KyPlatform platform;
    KyGPURuntime gpu;
    uint32_t flags;
} KyRuntime;

static inline KyRuntime *ky_runtime_create(void) {
    KyRuntime *rt = calloc(1, sizeof(KyRuntime));
    if (!rt) return NULL;
    rt->platform = ky_platform_init();
    
    // Use simplified GPU detection
    rt->gpu.active_backend = ky_gpu_detect();
    if (rt->gpu.active_backend != KY_GPU_NONE) {
        rt->gpu.device_count = 1;
        rt->gpu.devices = calloc(1, sizeof(KyGPUDevice));
        if (rt->gpu.devices) {
            ky_gpu_read_sysfs(&rt->gpu.devices[0]);
            rt->gpu.devices[0].type = rt->gpu.active_backend;
        }
        rt->flags |= 0x1;
    }
    return rt;
}

static inline void ky_runtime_print_info(KyRuntime *rt) {
    if (!rt) return;
    printf("=== KYJIT Runtime ===\n");
    printf("CPU: %s (%s), %u cores, SIMD %uB\n",
           rt->platform.vendor, rt->platform.arch,
           rt->platform.num_cores, rt->platform.simd_width);
    
    if (ky_gpu_available(&rt->gpu)) {
        ky_gpu_print_info(&rt->gpu);
    } else {
        printf("GPU: None detected (CPU only)\n");
    }
    printf("=====================\n");
}

#endif
