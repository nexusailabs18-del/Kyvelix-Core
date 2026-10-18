#include <pthread.h>
#include <immintrin.h>
#include <unistd.h>
#include "kyjit_mfr_mt.h"

typedef struct {
    int32_t *data;
    int start, end;
    long long partial;
} MFRArg;

static void* mfr_worker(void *arg) {
    MFRArg *a = (MFRArg*)arg;
    long long sum = 0;
    int i = a->start;
    for(; i+8 <= a->end; i+=8) {
        __m256i v = _mm256_loadu_si256((__m256i*)&a->data[i]);
        __m256i d = _mm256_slli_epi32(v, 1);  /* *2 */
        
        /* Manual modulo 3: mod = d - ((d * 0x55555556) >> 33) * 3 */
        __m256i magic = _mm256_set1_epi32(0x55555556);
        __m256i q = _mm256_srli_epi32(_mm256_mullo_epi32(d, magic), 30);
        __m256i mod = _mm256_sub_epi32(d, _mm256_mullo_epi32(q, _mm256_set1_epi32(3)));
        
        __m256i mask = _mm256_cmpeq_epi32(mod, _mm256_setzero_si256());
        __m256i f = _mm256_and_si256(d, mask);
        int tmp[8];
        _mm256_storeu_si256((__m256i*)tmp, f);
        for(int j=0;j<8;j++) sum += tmp[j];
    }
    for(; i<a->end; i++) {
        int v = a->data[i] * 2;
        if(v % 3 == 0) sum += v;
    }
    a->partial = sum;
    return NULL;
}

long long kyjit_mfr_mt_exec(int32_t *data, int n) {
    int num_threads = sysconf(_SC_NPROCESSORS_ONLN);
    if (num_threads > 64) num_threads = 64;
    if (num_threads < 1) num_threads = 1;
    
    pthread_t threads[64];
    MFRArg args[64];
    int chunk = n / num_threads;
    
    for(int t=0;t<num_threads;t++){
        args[t].data = data;
        args[t].start = t*chunk;
        args[t].end = (t==num_threads-1) ? n : (t+1)*chunk;
        pthread_create(&threads[t], NULL, mfr_worker, &args[t]);
    }
    long long total = 0;
    for(int t=0;t<num_threads;t++){
        pthread_join(threads[t], NULL);
        total += args[t].partial;
    }
    return total;
}
