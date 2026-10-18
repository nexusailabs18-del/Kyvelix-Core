#ifndef KYJIT_UNIVERSAL_H
#define KYJIT_UNIVERSAL_H

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <immintrin.h>
#include <cpuid.h>
#include <pthread.h>
#include <unistd.h>
#include <sys/sysinfo.h>

typedef enum {
    KYJIT_ARCH_UNKNOWN = 0,
    KYJIT_ARCH_SSE2,
    KYJIT_ARCH_AVX,
    KYJIT_ARCH_AVX2,
    KYJIT_ARCH_AVX512,
    KYJIT_ARCH_ARM_NEON,
} KyjitArch;

typedef enum {
    KYJIT_GPU_NONE = 0,
    KYJIT_GPU_INTEL,
    KYJIT_GPU_AMD,
    KYJIT_GPU_NVIDIA,
    KYJIT_GPU_APPLE,
} KyjitGPU;

typedef struct {
    KyjitArch arch;
    KyjitGPU gpu;
    int cores;
    int has_fma;
    int has_avx2;
    int has_avx512;
    char cpu_name[64];
} KyjitHardware;

static inline void kyjit_detect_cpu(KyjitHardware *hw) {
    unsigned int eax, ebx, ecx, edx;
    memset(hw, 0, sizeof(*hw));
    
    /* Get CPU name */
    __cpuid(0x80000000, eax, ebx, ecx, edx);
    if (eax >= 0x80000004) {
        char *p = hw->cpu_name;
        for (int i = 2; i <= 4; i++) {
            __cpuid(0x80000000 + i, eax, ebx, ecx, edx);
            memcpy(p, &eax, 4); p += 4;
            memcpy(p, &ebx, 4); p += 4;
            memcpy(p, &ecx, 4); p += 4;
            memcpy(p, &edx, 4); p += 4;
        }
    }
    
    /* Check AVX + FMA */
    __cpuid(1, eax, ebx, ecx, edx);
    int has_avx = (ecx & bit_AVX) ? 1 : 0;
    hw->has_fma = (ecx & bit_FMA) ? 1 : 0;
    
    /* Check AVX2 + AVX512 */
    __cpuid_count(7, 0, eax, ebx, ecx, edx);
    hw->has_avx2 = (ebx & bit_AVX2) ? 1 : 0;
    hw->has_avx512 = (ebx & bit_AVX512F) ? 1 : 0;
    
    /* Determine architecture */
    if (hw->has_avx512) {
        hw->arch = KYJIT_ARCH_AVX512;
    } else if (hw->has_avx2) {
        hw->arch = KYJIT_ARCH_AVX2;
    } else if (has_avx) {
        hw->arch = KYJIT_ARCH_AVX;
    } else {
        hw->arch = KYJIT_ARCH_SSE2;
    }
    
    hw->cores = sysconf(_SC_NPROCESSORS_ONLN);
}

static inline KyjitGPU kyjit_detect_gpu(void) {
    FILE *f = popen("lspci 2>/dev/null | grep -i vga", "r");
    if (f) {
        char buf[256];
        while (fgets(buf, sizeof(buf), f)) {
            if (strstr(buf, "NVIDIA")) { pclose(f); return KYJIT_GPU_NVIDIA; }
            if (strstr(buf, "AMD")) { pclose(f); return KYJIT_GPU_AMD; }
            if (strstr(buf, "Intel")) { pclose(f); return KYJIT_GPU_INTEL; }
        }
        pclose(f);
    }
    return KYJIT_GPU_NONE;
}

/* Universal INT8 add */
static inline void kyjit_universal_add_int8(const int8_t *a, const int8_t *b, 
                                             int8_t *c, int n, KyjitArch arch) {
    int i = 0;
    switch(arch) {
        case KYJIT_ARCH_AVX512:
            #ifdef __AVX512F__
            for(; i+64 <= n; i+=64) {
                __m512i va = _mm512_loadu_si512((__m512i*)&a[i]);
                __m512i vb = _mm512_loadu_si512((__m512i*)&b[i]);
                _mm512_storeu_si512((__m512i*)&c[i], _mm512_add_epi8(va, vb));
            }
            #endif
            break;
        case KYJIT_ARCH_AVX2:
        case KYJIT_ARCH_AVX:
            for(; i+32 <= n; i+=32) {
                __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
                __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
                _mm256_storeu_si256((__m256i*)&c[i], _mm256_add_epi8(va, vb));
            }
            break;
        default:
            for(; i+16 <= n; i+=16) {
                __m128i va = _mm_loadu_si128((__m128i*)&a[i]);
                __m128i vb = _mm_loadu_si128((__m128i*)&b[i]);
                _mm_storeu_si128((__m128i*)&c[i], _mm_add_epi8(va, vb));
            }
            break;
    }
    for(; i < n; i++) c[i] = a[i] + b[i];
}

/* Universal FMA */
static inline void kyjit_universal_fma(const float *a, float scalar, 
                                        const float *b, float *c, int n, KyjitArch arch) {
    int i = 0;
    switch(arch) {
        case KYJIT_ARCH_AVX512:
            #ifdef __AVX512F__
            {
                __m512 sc = _mm512_set1_ps(scalar);
                for(; i+16 <= n; i+=16) {
                    __m512 va = _mm512_loadu_ps(&a[i]);
                    __m512 vb = _mm512_loadu_ps(&b[i]);
                    _mm512_storeu_ps(&c[i], _mm512_fmadd_ps(va, sc, vb));
                }
            }
            #endif
            break;
        case KYJIT_ARCH_AVX2:
        case KYJIT_ARCH_AVX:
            {
                __m256 sc = _mm256_set1_ps(scalar);
                for(; i+8 <= n; i+=8) {
                    __m256 va = _mm256_loadu_ps(&a[i]);
                    __m256 vb = _mm256_loadu_ps(&b[i]);
                    _mm256_storeu_ps(&c[i], _mm256_fmadd_ps(va, sc, vb));
                }
            }
            break;
        default:
            for(; i < n; i++) c[i] = a[i] * scalar + b[i];
            break;
    }
}

#endif
