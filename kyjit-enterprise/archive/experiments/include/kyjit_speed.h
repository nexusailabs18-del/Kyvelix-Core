#ifndef KYJIT_SPEED_H
#define KYJIT_SPEED_H

#include <immintrin.h>
#include <stdint.h>
#include <string.h>

/* Force inline for hot paths */
#define KYJIT_HOT __attribute__((always_inline, hot, flatten))
#define KYJIT_COLD __attribute__((cold))
#define KYJIT_LIKELY(x) __builtin_expect(!!(x), 1)
#define KYJIT_UNLIKELY(x) __builtin_expect(!!(x), 0)

/* Prefetch distances tuned for modern CPUs */
#define KYJIT_PREFETCH_L1(x) __builtin_prefetch((x), 0, 1)
#define KYJIT_PREFETCH_L2(x) __builtin_prefetch((x), 0, 2)
#define KYJIT_PREFETCH_L3(x) __builtin_prefetch((x), 0, 3)

/* 1. Super-fast INT8 FMA - process 512 bytes at once */
static inline void KYJIT_HOT
kyjit_int8_fma_super(const int8_t *a, const int8_t *b, int8_t *c, int n) {
    int i = 0;
    
    // Main loop - 512 bytes per iteration (16 AVX2 registers)
    for (; i + 512 <= n; i += 512) {
        // Prefetch next iteration
        KYJIT_PREFETCH_L1(&a[i + 512]);
        KYJIT_PREFETCH_L1(&b[i + 512]);
        
        // Process 16 vectors of 32 bytes each
        for (int j = 0; j < 512; j += 32) {
            __m256i va = _mm256_loadu_si256((__m256i*)&a[i + j]);
            __m256i vb = _mm256_loadu_si256((__m256i*)&b[i + j]);
            __m256i vc = _mm256_add_epi8(va, vb);
            _mm256_storeu_si256((__m256i*)&c[i + j], vc);
        }
    }
    
    // Handle remaining with 32-byte chunks
    for (; i + 32 <= n; i += 32) {
        __m256i va = _mm256_loadu_si256((__m256i*)&a[i]);
        __m256i vb = _mm256_loadu_si256((__m256i*)&b[i]);
        _mm256_storeu_si256((__m256i*)&c[i], _mm256_add_epi8(va, vb));
    }
    
    // Handle tail
    for (; i < n; i++) {
        c[i] = a[i] + b[i];
    }
}

/* 2. INT32 sum with 16 accumulators - maximum ILP */
static inline int KYJIT_HOT
kyjit_sum_i32_16acc(const int32_t *data, int n) {
    __m256i s0 = _mm256_setzero_si256();
    __m256i s1 = _mm256_setzero_si256();
    __m256i s2 = _mm256_setzero_si256();
    __m256i s3 = _mm256_setzero_si256();
    __m256i s4 = _mm256_setzero_si256();
    __m256i s5 = _mm256_setzero_si256();
    __m256i s6 = _mm256_setzero_si256();
    __m256i s7 = _mm256_setzero_si256();
    __m256i s8 = _mm256_setzero_si256();
    __m256i s9 = _mm256_setzero_si256();
    __m256i s10 = _mm256_setzero_si256();
    __m256i s11 = _mm256_setzero_si256();
    __m256i s12 = _mm256_setzero_si256();
    __m256i s13 = _mm256_setzero_si256();
    __m256i s14 = _mm256_setzero_si256();
    __m256i s15 = _mm256_setzero_si256();
    
    int i = 0;
    
    // Process 128 elements (512 bytes) per iteration
    for (; i + 128 <= n; i += 128) {
        // Prefetch
        KYJIT_PREFETCH_L2(&data[i + 256]);
        
        s0 = _mm256_add_epi32(s0, _mm256_loadu_si256((__m256i*)&data[i]));
        s1 = _mm256_add_epi32(s1, _mm256_loadu_si256((__m256i*)&data[i+8]));
        s2 = _mm256_add_epi32(s2, _mm256_loadu_si256((__m256i*)&data[i+16]));
        s3 = _mm256_add_epi32(s3, _mm256_loadu_si256((__m256i*)&data[i+24]));
        s4 = _mm256_add_epi32(s4, _mm256_loadu_si256((__m256i*)&data[i+32]));
        s5 = _mm256_add_epi32(s5, _mm256_loadu_si256((__m256i*)&data[i+40]));
        s6 = _mm256_add_epi32(s6, _mm256_loadu_si256((__m256i*)&data[i+48]));
        s7 = _mm256_add_epi32(s7, _mm256_loadu_si256((__m256i*)&data[i+56]));
        s8 = _mm256_add_epi32(s8, _mm256_loadu_si256((__m256i*)&data[i+64]));
        s9 = _mm256_add_epi32(s9, _mm256_loadu_si256((__m256i*)&data[i+72]));
        s10 = _mm256_add_epi32(s10, _mm256_loadu_si256((__m256i*)&data[i+80]));
        s11 = _mm256_add_epi32(s11, _mm256_loadu_si256((__m256i*)&data[i+88]));
        s12 = _mm256_add_epi32(s12, _mm256_loadu_si256((__m256i*)&data[i+96]));
        s13 = _mm256_add_epi32(s13, _mm256_loadu_si256((__m256i*)&data[i+104]));
        s14 = _mm256_add_epi32(s14, _mm256_loadu_si256((__m256i*)&data[i+112]));
        s15 = _mm256_add_epi32(s15, _mm256_loadu_si256((__m256i*)&data[i+120]));
    }
    
    // Combine all 16 accumulators (tree reduction)
    s0 = _mm256_add_epi32(s0, s1);
    s2 = _mm256_add_epi32(s2, s3);
    s4 = _mm256_add_epi32(s4, s5);
    s6 = _mm256_add_epi32(s6, s7);
    s8 = _mm256_add_epi32(s8, s9);
    s10 = _mm256_add_epi32(s10, s11);
    s12 = _mm256_add_epi32(s12, s13);
    s14 = _mm256_add_epi32(s14, s15);
    
    s0 = _mm256_add_epi32(s0, s2);
    s4 = _mm256_add_epi32(s4, s6);
    s8 = _mm256_add_epi32(s8, s10);
    s12 = _mm256_add_epi32(s12, s14);
    
    s0 = _mm256_add_epi32(s0, s4);
    s8 = _mm256_add_epi32(s8, s12);
    
    __m256i sum_vec = _mm256_add_epi32(s0, s8);
    
    // Horizontal sum
    int arr[8];
    _mm256_storeu_si256((__m256i*)arr, sum_vec);
    int sum = arr[0] + arr[1] + arr[2] + arr[3] + arr[4] + arr[5] + arr[6] + arr[7];
    
    // Handle remaining elements
    for (; i < n; i++) {
        sum += data[i];
    }
    
    return sum;
}

/* 3. Blocked matrix multiply with AVX2 + FMA */
static inline void KYJIT_HOT
kyjit_matmul_blocked(const float *a, const float *b, float *c, int n) {
    const int BLOCK = 64;  // 64x64 block = 16KB, fits in L1
    
    memset(c, 0, n * n * sizeof(float));
    
    for (int i0 = 0; i0 < n; i0 += BLOCK) {
        for (int j0 = 0; j0 < n; j0 += BLOCK) {
            for (int k0 = 0; k0 < n; k0 += BLOCK) {
                
                int i_max = (i0 + BLOCK < n) ? i0 + BLOCK : n;
                int j_max = (j0 + BLOCK < n) ? j0 + BLOCK : n;
                int k_max = (k0 + BLOCK < n) ? k0 + BLOCK : n;
                
                for (int i = i0; i < i_max; i++) {
                    for (int j = j0; j < j_max; j++) {
                        __m256 sum = _mm256_setzero_ps();
                        int k = k0;
                        
                        // Vectorized inner loop with FMA
                        for (; k + 8 <= k_max; k += 8) {
                            __m256 va = _mm256_loadu_ps(&a[i*n + k]);
                            __m256 vb = _mm256_loadu_ps(&b[k*n + j]);
                            sum = _mm256_fmadd_ps(va, vb, sum);
                        }
                        
                        // Horizontal sum
                        float sum_arr[8];
                        _mm256_storeu_ps(sum_arr, sum);
                        float total = sum_arr[0] + sum_arr[1] + sum_arr[2] + sum_arr[3] +
                                     sum_arr[4] + sum_arr[5] + sum_arr[6] + sum_arr[7];
                        
                        // Handle remaining
                        for (; k < k_max; k++) {
                            total += a[i*n + k] * b[k*n + j];
                        }
                        
                        c[i*n + j] += total;
                    }
                }
            }
        }
    }
}

/* 4. Fast object processing with SoA layout */
typedef struct {
    int *x, *y, *z, *w;  // Structure of Arrays
} KyjitSoA;

static inline long long KYJIT_HOT
kyjit_process_soa(const KyjitSoA *soa, int n) {
    long long sum = 0;
    int i = 0;
    
    // Process 8 elements at a time
    for (; i + 8 <= n; i += 8) {
        __m256i vx = _mm256_loadu_si256((__m256i*)&soa->x[i]);
        __m256i vy = _mm256_loadu_si256((__m256i*)&soa->y[i]);
        __m256i vz = _mm256_loadu_si256((__m256i*)&soa->z[i]);
        __m256i vw = _mm256_loadu_si256((__m256i*)&soa->w[i]);
        
        __m256i vsum = _mm256_add_epi32(_mm256_add_epi32(vx, vy),
                                        _mm256_add_epi32(vz, vw));
        
        int arr[8];
        _mm256_storeu_si256((__m256i*)arr, vsum);
        sum += arr[0] + arr[1] + arr[2] + arr[3] + arr[4] + arr[5] + arr[6] + arr[7];
    }
    
    // Handle remaining
    for (; i < n; i++) {
        sum += soa->x[i] + soa->y[i] + soa->z[i] + soa->w[i];
    }
    
    return sum;
}

#endif // KYJIT_SPEED_H
