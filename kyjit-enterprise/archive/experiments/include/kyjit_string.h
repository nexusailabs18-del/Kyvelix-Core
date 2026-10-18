#ifndef KYJIT_STRING_H
#define KYJIT_STRING_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))
#include <stddef.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KyString {
    uint32_t length;
    uint32_t hash;
    uint32_t flags;
    char data[];
} KyString;

#define KY_STRING_FLAG_ASCII 0x1

// FNV-1a hash + length in one pass
static inline uint32_t ky_string_hash_len(const char *str, uint32_t *len_out) {
    uint32_t hash = 2166136261u;
    uint32_t len = 0;
    while (str[len]) {
        hash ^= (uint8_t)str[len];
        hash *= 16777619u;
        len++;
    }
    if (len_out) *len_out = len;
    return hash;
}

KyString *ky_string_create(const char *str);
KyString *ky_string_create_hashed(const char *str, uint32_t len, uint32_t hash);
int ky_string_compare(KyString *a, KyString *b);
KyString *ky_string_concat(KyString *a, KyString *b);
KyString *ky_string_slice(KyString *src, uint32_t start, uint32_t end);

#ifdef __cplusplus
}
#endif

#endif
int kyjit_string_lowercase_avx2(const char *str, size_t len);
