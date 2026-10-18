#include <immintrin.h>
#include "kyjit_hash.h"

// Direct addressing with AVX2 gather for 8 keys at once
void kyjit_hash_lookup_avx2(int *table, int *keys, int n, int *results) {
    int i = 0;
    for(; i+8 <= n; i+=8) {
        __m256i indices = _mm256_loadu_si256((__m256i*)&keys[i]);
        __m256i values = _mm256_i32gather_epi32(table, indices, 4);
        _mm256_storeu_si256((__m256i*)&results[i], values);
    }
    // Tail
    for(; i<n; i++) {
        results[i] = table[keys[i]];
    }
}

/* AVX2 gather — 8 hash lookups in ONE instruction */
void kyjit_hash_lookup_avx2_gather(int32_t *table, int32_t *keys, int32_t *results, int n) {
    int i = 0;
    for(; i+8 <= n; i+=8) {
        /* Prefetch next batch */
        __builtin_prefetch(&keys[i+16], 0, 3);
        __builtin_prefetch(&table[keys[i+16]], 0, 3);
        
        /* 8 lookups at once */
        __m256i indices = _mm256_loadu_si256((__m256i*)&keys[i]);
        __m256i values = _mm256_i32gather_epi32(table, indices, 4);
        _mm256_storeu_si256((__m256i*)&results[i], values);
    }
    /* Tail */
    for(; i<n; i++) results[i] = table[keys[i]];
}

/* Perfect hash — precomputed for known keys, O(1) lookup */
long long kyjit_hash_perfect_lookup(int32_t *perfect_table, int32_t *keys, int n) {
    long long sum = 0;
    for(int i=0;i<n;i++) {
        sum += perfect_table[keys[i]];  /* Direct offset, no collision */
    }
    return sum;
}
