#include <stdio.h>
#include <string.h>
#include <unistd.h>

static void cpuid(int leaf, int subleaf, unsigned int *eax, unsigned int *ebx,
                  unsigned int *ecx, unsigned int *edx) {
    __asm__ __volatile__("cpuid"
        : "=a"(*eax), "=b"(*ebx), "=c"(*ecx), "=d"(*edx)
        : "a"(leaf), "c"(subleaf));
}

int kyjit_cpu_has_avx512(void) {
    unsigned int eax, ebx, ecx, edx;
    cpuid(7, 0, &eax, &ebx, &ecx, &edx);
    return (ebx & (1 << 16)) != 0;
}

int kyjit_cpu_has_avx2(void) {
    unsigned int eax, ebx, ecx, edx;
    cpuid(7, 0, &eax, &ebx, &ecx, &edx);
    return (ebx & (1 << 5)) != 0;
}

int kyjit_cpu_has_fma(void) {
    unsigned int eax, ebx, ecx, edx;
    cpuid(1, 0, &eax, &ebx, &ecx, &edx);
    return (ecx & (1 << 12)) != 0;
}

int kyjit_cpu_has_sse4(void) {
    unsigned int eax, ebx, ecx, edx;
    cpuid(1, 0, &eax, &ebx, &ecx, &edx);
    return (ecx & (1 << 19)) != 0;
}

int kyjit_cpu_count(void) {
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return n > 0 ? (int)n : 1;
}

const char* kyjit_cpu_name(void) {
    static char name[49] = {0};
    unsigned int eax, ebx, ecx, edx;
    
    cpuid(0x80000000, 0, &eax, &ebx, &ecx, &edx);
    if (eax < 0x80000004) return "Unknown";
    
    unsigned int *p = (unsigned int*)name;
    cpuid(0x80000002, 0, &p[0], &p[1], &p[2], &p[3]);
    cpuid(0x80000003, 0, &p[4], &p[5], &p[6], &p[7]);
    cpuid(0x80000004, 0, &p[8], &p[9], &p[10], &p[11]);
    name[48] = 0;
    
    char *s = name;
    while (*s == ' ') s++;
    return s;
}

const char* kyjit_cpu_best_path(void) {
    if (kyjit_cpu_has_avx512()) return "AVX512";
    if (kyjit_cpu_has_avx2())   return "AVX2+FMA";
    if (kyjit_cpu_has_sse4())   return "SSE4";
    return "Baseline x86-64";
}

void kyjit_cpu_info(void) {
    printf("CPU: %s\n", kyjit_cpu_name());
    printf("Cores: %d\n", kyjit_cpu_count());
    printf("Best path: %s\n", kyjit_cpu_best_path());
}
