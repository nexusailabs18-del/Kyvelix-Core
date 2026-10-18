#ifndef KYJIT_COMPLETE_H
#define KYJIT_COMPLETE_H
#include <stdint.h>
#include <stddef.h>

/* Complete KYJIT — all workloads in one module */

/* Safe Fibonacci with modulo */
uint64_t kyjit_fib_safe(int n);

/* Safe Sieve with allocation check */
int kyjit_sieve_safe(int n);

/* Matrix multiply via OpenBLAS */
void kyjit_matmul_openblas(float *A, float *B, float *C, int N);

/* Map/Filter/Reduce SIMD */
long long kyjit_mfr_simd(int32_t *data, int n);

/* JSON number parse */
long long kyjit_json_parse(const char *json, int len);

/* String lowercase */
int kyjit_string_lower(const char *str, int len);

/* Hash lookup */
long long kyjit_hash_lookup(int *table, int size, int *keys, int n);

/* Complete benchmark — all workloads */
double kyjit_complete_benchmark(void);

#endif
