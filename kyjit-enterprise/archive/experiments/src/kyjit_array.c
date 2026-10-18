#include "kyjit_array.h"

/* AVX2 sum with 16 accumulators */
long long kyjit_array_sum_avx2(const int32_t *data, int n) {
    __m256i acc[16];
    for(int j=0;j<16;j++) acc[j] = _mm256_setzero_si256();
    int i = 0;
    for(; i+128 <= n; i+=128) {
        for(int j=0;j<16;j++) {
            acc[j] = _mm256_add_epi32(acc[j],
                _mm256_loadu_si256((__m256i*)&data[i+j*8]));
        }
    }
    for(int j=8;j>0;j/=2)
        for(int k=0;k<j;k++)
            acc[k] = _mm256_add_epi32(acc[k], acc[k+j]);
    int tmp[8];
    _mm256_storeu_si256((__m256i*)tmp, acc[0]);
    long long sum = tmp[0]+tmp[1]+tmp[2]+tmp[3]+tmp[4]+tmp[5]+tmp[6]+tmp[7];
    for(; i<n; i++) sum += data[i];
    return sum;
}

/* Non-temporal streaming add */
void kyjit_array_stream_add(const int32_t *restrict a,
                            const int32_t *restrict b,
                            int32_t *restrict c, int n) {
    int i = 0;
    for(; i+8 <= n; i+=8) {
        __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
        __m256i vc = _mm256_add_epi32(va, vb);
        _mm256_stream_si256((__m256i*)&c[i], vc);
    }
    _mm_sfence();
    for(; i<n; i++) c[i] = a[i] + b[i];
}

/* Restrict-optimized sum */
long long kyjit_array_sum_restrict(const int32_t *restrict data, int n) {
    long long sum = 0;
    int i = 0;
    __m256i acc[16];
    for(int j=0;j<16;j++) acc[j] = _mm256_setzero_si256();
    
    for(; i+128 <= n; i+=128) {
        __builtin_prefetch(&data[i+1024], 0, 0);
        #pragma GCC unroll 16
        for(int j=0;j<16;j++) {
            acc[j] = _mm256_add_epi32(acc[j],
                _mm256_loadu_si256((__m256i*)&data[i+j*8]));
        }
    }
    for(int j=8;j>0;j/=2)
        for(int k=0;k<j;k++)
            acc[k] = _mm256_add_epi32(acc[k], acc[k+j]);
    int tmp[8];
    _mm256_storeu_si256((__m256i*)tmp, acc[0]);
    sum = tmp[0]+tmp[1]+tmp[2]+tmp[3]+tmp[4]+tmp[5]+tmp[6]+tmp[7];
    for(; i<n; i++) sum += data[i];
    return sum;
}
