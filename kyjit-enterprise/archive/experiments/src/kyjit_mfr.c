#include <pthread.h>
#include <immintrin.h>
#include "kyjit_mfr.h"

#define KYJIT_THREADS 16

typedef struct {
    int32_t *data;
    int start, end;
    long long partial;
} MFRArg;

static void* mfr_worker(void *arg) {
    MFRArg *a = (MFRArg*)arg;
    long long sum = 0;
    int i = a->start;
    // Process 8 elements at a time
    for(; i+8 <= a->end; i+=8) {
        __m256i v = _mm256_loadu_si256((__m256i*)&a->data[i]);
        __m256i doubled = _mm256_slli_epi32(v, 1);          // *2
        __m256i three = _mm256_set1_epi32(3);
        // Manual modulo: mod = d - (d/3)*3
        __m256i div3 = _mm256_srli_epi32(_mm256_mullo_epi32(doubled, _mm256_set1_epi32(0x55555556)), 1);
        __m256i mod = _mm256_sub_epi32(doubled, _mm256_mullo_epi32(div3, three));
        __m256i mask = _mm256_cmpeq_epi32(mod, _mm256_setzero_si256());
        __m256i filtered = _mm256_and_si256(doubled, mask);
        int tmp[8];
        _mm256_storeu_si256((__m256i*)tmp, filtered);
        for(int j=0;j<8;j++) sum += tmp[j];
    }
    // Tail
    for(; i<a->end; i++) {
        int v = a->data[i] * 2;
        if(v % 3 == 0) sum += v;
    }
    a->partial = sum;
    return NULL;
}

long long kyjit_mfr_exec(int32_t *data, int n) {
    pthread_t threads[KYJIT_THREADS];
    MFRArg args[KYJIT_THREADS];
    int chunk = n / KYJIT_THREADS;
    for(int t=0;t<KYJIT_THREADS;t++){
        args[t].data = data;
        args[t].start = t*chunk;
        args[t].end = (t==KYJIT_THREADS-1) ? n : (t+1)*chunk;
        pthread_create(&threads[t], NULL, mfr_worker, &args[t]);
    }
    long long total = 0;
    for(int t=0;t<KYJIT_THREADS;t++){
        pthread_join(threads[t], NULL);
        total += args[t].partial;
    }
    return total;
}
