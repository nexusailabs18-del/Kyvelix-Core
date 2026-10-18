#ifndef KYJIT_UTILS_H
#define KYJIT_UTILS_H

#include <stdint.h>
#include <stddef.h>

/* Power of 2 */
int      kyjit_is_pow2(uint64_t x);
uint64_t kyjit_next_pow2(uint64_t x);
uint64_t kyjit_round_up_pow2(uint64_t x, uint64_t alignment);

/* Alignment */
uint64_t kyjit_align_up(uint64_t x, uint64_t alignment);
uint64_t kyjit_align_down(uint64_t x, uint64_t alignment);

/* Hashing */
uint32_t kyjit_hash_string(const char *str);
uint32_t kyjit_hash_bytes(const void *data, size_t len);

/* String helpers */
int      kyjit_str_equal(const char *a, const char *b);
char    *kyjit_str_dup(const char *str);
int      kyjit_str_starts_with(const char *str, const char *prefix);
int      kyjit_str_ends_with(const char *str, const char *suffix);

/* Bit manipulation */
int      kyjit_popcount64(uint64_t x);
int      kyjit_clz64(uint64_t x);
int      kyjit_ctz64(uint64_t x);

/* Memory */
void    *kyjit_mem_dup(const void *data, size_t len);

#endif
