#ifndef KYJIT_HASH_H
#define KYJIT_HASH_H
void kyjit_hash_lookup_avx2(int *table, int *keys, int n, int *results);
#endif
void kyjit_hash_lookup_avx2_gather(int32_t *table, int32_t *keys, int32_t *results, int n);
long long kyjit_hash_perfect_lookup(int32_t *perfect_table, int32_t *keys, int n);
