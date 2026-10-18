#define _GNU_SOURCE
#include "kyjit_api.h"
#include "kyjit_universal.h"
#include <stdlib.h>
#include <string.h>
#include <immintrin.h>

struct KyjitEngine {
    KyjitHardware hw;
};

KyjitEngine* kyjit_create(void) {
    KyjitEngine *e = malloc(sizeof(KyjitEngine));
    kyjit_detect_cpu(&e->hw);
    e->hw.gpu = kyjit_detect_gpu();
    return e;
}

void kyjit_destroy(KyjitEngine *e) {
    free(e);
}

int kyjit_int8_add(KyjitEngine *e, const int8_t *a, const int8_t *b, int8_t *c, size_t n) {
    kyjit_universal_add_int8(a, b, c, n, e->hw.arch);
    return 0;
}

long long kyjit_int32_sum(KyjitEngine *e, const int32_t *data, size_t n) {
    long long sum = 0;
    int i = 0;
    
    if (e->hw.arch >= KYJIT_ARCH_AVX2) {
        __m256i acc[8];
        for(int j=0;j<8;j++) acc[j]=_mm256_setzero_si256();
        for(; i+64 <= (int)n; i+=64){
            for(int j=0;j<8;j++){
                acc[j]=_mm256_add_epi32(acc[j],_mm256_loadu_si256((__m256i*)&data[i+j*8]));
            }
        }
        for(int j=1;j<8;j++) acc[0]=_mm256_add_epi32(acc[0],acc[j]);
        int tmp[8]; _mm256_storeu_si256((__m256i*)tmp,acc[0]);
        sum=tmp[0]+tmp[1]+tmp[2]+tmp[3]+tmp[4]+tmp[5]+tmp[6]+tmp[7];
    }
    for(; i<(int)n; i++) sum += data[i];
    return sum;
}

int kyjit_float_fma(KyjitEngine *e, const float *a, float scalar, const float *b, float *c, size_t n) {
    kyjit_universal_fma(a, scalar, b, c, n, e->hw.arch);
    return 0;
}

int kyjit_string_lowercase(KyjitEngine *e, const char *str, size_t len) {
    int count = 0;
    size_t i = 0;
    
    if (e->hw.arch >= KYJIT_ARCH_AVX2) {
        for(; i+128 <= len; i+=128){
            __m256i lo=_mm256_set1_epi8('a'-1);
            __m256i hi=_mm256_set1_epi8('z'+1);
            for(int j=0;j<128;j+=32){
                __m256i v=_mm256_loadu_si256((__m256i*)&str[i+j]);
                count += __builtin_popcount(_mm256_movemask_epi8(
                    _mm256_and_si256(_mm256_cmpgt_epi8(v,lo),_mm256_cmpgt_epi8(hi,v))));
            }
        }
    }
    for(; i<len; i++) if(str[i]>='a'&&str[i]<='z') count++;
    return count;
}

long long kyjit_general_js(KyjitEngine *e, const int32_t *data, size_t n) {
    long long sum = 0;
    for(size_t i=0; i<n; i++){
        int v = data[i];
        sum += (v & 1) ? v*3 : v;  /* Branchless via cmov */
    }
    return sum;
}

const char* kyjit_get_cpu_name(KyjitEngine *e) { return e->hw.cpu_name; }
int kyjit_get_cores(KyjitEngine *e) { return e->hw.cores; }

const char* kyjit_get_arch(KyjitEngine *e) {
    switch(e->hw.arch) {
        case KYJIT_ARCH_AVX512: return "AVX-512";
        case KYJIT_ARCH_AVX2:   return "AVX2";
        case KYJIT_ARCH_AVX:    return "AVX";
        case KYJIT_ARCH_SSE2:   return "SSE2";
        default: return "Unknown";
    }
}
