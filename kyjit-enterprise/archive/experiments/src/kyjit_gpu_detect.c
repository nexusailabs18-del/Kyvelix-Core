#include "kyjit_cpu_detect.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>

/* GPU detection via system files */

int kyjit_gpu_has_cuda(void) {
    FILE *f = fopen("/proc/driver/nvidia/version", "r");
    if (f) { fclose(f); return 1; }
    return 0;
}

int kyjit_gpu_has_rocm(void) {
    FILE *f = fopen("/dev/kfd", "r");
    if (f) { fclose(f); return 1; }
    return 0;
}

int kyjit_gpu_has_intel(void) {
    /* Check /dev/dri for render nodes */
    DIR *d = opendir("/dev/dri");
    if (!d) return 0;
    
    struct dirent *entry;
    while ((entry = readdir(d))) {
        if (strncmp(entry->d_name, "renderD", 7) == 0) {
            closedir(d);
            return 1;
        }
    }
    closedir(d);
    return 0;
}

const char* kyjit_gpu_name(void) {
    if (kyjit_gpu_has_cuda()) {
        static char name[256] = "NVIDIA GPU";
        FILE *f = fopen("/proc/driver/nvidia/version", "r");
        if (f) {
            char line[256];
            if (fgets(line, sizeof(line), f)) {
                char *p = strstr(line, "Kernel Module");
                if (p) {
                    p = strstr(p, "for");
                    if (p) {
                        p += 4;
                        while (*p == ' ') p++;
                        char *end = strchr(p, '\n');
                        if (end) *end = 0;
                        snprintf(name, sizeof(name), "NVIDIA %s", p);
                    }
                }
            }
            fclose(f);
        }
        return name;
    }
    
    if (kyjit_gpu_has_rocm()) return "AMD GPU (ROCm)";
    if (kyjit_gpu_has_intel()) return "Intel GPU";
    return "None (CPU only)";
}

const char* kyjit_gpu_best_path(void) {
    if (kyjit_gpu_has_cuda())  return "cuBLAS";
    if (kyjit_gpu_has_rocm())  return "rocBLAS";
    if (kyjit_gpu_has_intel()) return "oneDNN GPU";
    return "OpenBLAS CPU";
}

void kyjit_gpu_info(void) {
    printf("GPU: %s\n", kyjit_gpu_name());
    printf("Best path: %s\n", kyjit_gpu_best_path());
    printf("  CUDA:  %s\n", kyjit_gpu_has_cuda() ? "YES" : "no");
    printf("  ROCm:  %s\n", kyjit_gpu_has_rocm() ? "YES" : "no");
    printf("  Intel: %s\n", kyjit_gpu_has_intel() ? "YES" : "no");
}

/* Unified hardware info */
void kyjit_hardware_info(void) {
    printf("╔══════════════════════════════════════════════╗\n");
    printf("║   KYJIT — HARDWARE DETECTION                 ║\n");
    printf("╠══════════════════════════════════════════════╣\n");
    printf("║ CPU: %-40s ║\n", kyjit_cpu_name());
    printf("║ Cores: %-38d ║\n", kyjit_cpu_count());
    printf("║ CPU Path: %-35s ║\n", kyjit_cpu_best_path());
    printf("║ GPU: %-40s ║\n", kyjit_gpu_name());
    printf("║ GPU Path: %-35s ║\n", kyjit_gpu_best_path());
    printf("╚══════════════════════════════════════════════╝\n");
}
