#include "kyjit_utils.h"
#include <stdlib.h>
#include <string.h>

/* ── Power of 2 ── */

int kyjit_is_pow2(uint64_t x) {
    return x != 0 && (x & (x - 1)) == 0;
}

uint64_t kyjit_next_pow2(uint64_t x) {
    if (x <= 1) return 1;
    x--;
    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x |= x >> 16;
    x |= x >> 32;
    return x + 1;
}

uint64_t kyjit_round_up_pow2(uint64_t x, uint64_t alignment) {
    return (x + alignment - 1) & ~(alignment - 1);
}

/* ── Alignment ── */

uint64_t kyjit_align_up(uint64_t x, uint64_t alignment) {
    return (x + alignment - 1) & ~(alignment - 1);
}

uint64_t kyjit_align_down(uint64_t x, uint64_t alignment) {
    return x & ~(alignment - 1);
}

/* ── Hashing (FNV-1a) ── */

uint32_t kyjit_hash_bytes(const void *data, size_t len) {
    const uint8_t *bytes = (const uint8_t*)data;
    uint32_t hash = 2166136261u;
    for (size_t i = 0; i < len; i++) {
        hash ^= bytes[i];
        hash *= 16777619u;
    }
    return hash;
}

uint32_t kyjit_hash_string(const char *str) {
    return kyjit_hash_bytes(str, strlen(str));
}

/* ── String helpers ── */

int kyjit_str_equal(const char *a, const char *b) {
    return strcmp(a, b) == 0;
}

char *kyjit_str_dup(const char *str) {
    return strdup(str);
}

int kyjit_str_starts_with(const char *str, const char *prefix) {
    size_t str_len = strlen(str);
    size_t prefix_len = strlen(prefix);
    if (prefix_len > str_len) return 0;
    return memcmp(str, prefix, prefix_len) == 0;
}

int kyjit_str_ends_with(const char *str, const char *suffix) {
    size_t str_len = strlen(str);
    size_t suffix_len = strlen(suffix);
    if (suffix_len > str_len) return 0;
    return memcmp(str + str_len - suffix_len, suffix, suffix_len) == 0;
}

/* ── Bit manipulation ── */

int kyjit_popcount64(uint64_t x) {
    return __builtin_popcountll(x);
}

int kyjit_clz64(uint64_t x) {
    return __builtin_clzll(x);
}

int kyjit_ctz64(uint64_t x) {
    return __builtin_ctzll(x);
}

/* ── Memory ── */

void *kyjit_mem_dup(const void *data, size_t len) {
    void *copy = malloc(len);
    if (copy) memcpy(copy, data, len);
    return copy;
}
