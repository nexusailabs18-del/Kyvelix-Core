#include <stdlib.h>
#include "kyjit_simple_loop.h"

/* Core pinning done ONCE by caller — not in hot path */
long long kyjit_simple_loop_sum(const int32_t *data, int n) {
    long long sum = 0;
    int i = 0;
    
    __m256i acc[16];
    for(int j=0;j<16;j++) acc[j] = _mm256_setzero_si256();
    
    for(; i+128 <= n; i+=128) {
        __builtin_prefetch(&data[i+256], 0, 3);
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

long long kyjit_simple_loop_batch(const int32_t *data, int batch_count, int n) {
    long long total = 0;
    for(int b=0;b<batch_count;b++) {
        total += kyjit_simple_loop_sum(data, n);
    }
    return total;
}
