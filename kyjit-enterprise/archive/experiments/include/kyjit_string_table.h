#ifndef KYJIT_STRING_TABLE_H
#define KYJIT_STRING_TABLE_H

#include <stdint.h>
#include <string.h>
#include "kyjit_string.h"

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)

typedef struct KyStringTable {
    uint32_t size;
    uint32_t mask;
    uint32_t count;
    KyString **buckets;
} KyStringTable;

static inline int ky_string_eq_fast(const KyString *s, const char *str, uint32_t len) {
    if (s->length != len) return 0;
    const char *ps = s->data;
    if (likely(len >= 8)) {
        uint64_t first8;
        memcpy(&first8, ps, 8);
        uint64_t first8str;
        memcpy(&first8str, str, 8);
        if (first8 != first8str) return 0;
        uint32_t i = 8;
        for (; i + 8 <= len; i += 8) {
            uint64_t a, b;
            memcpy(&a, ps + i, 8);
            memcpy(&b, str + i, 8);
            if (a != b) return 0;
        }
        for (; i < len; i++) if (ps[i] != str[i]) return 0;
        return 1;
    } else {
        switch (len) {
            case 0: return 1;
            case 1: return ps[0] == str[0];
            case 2: return *(uint16_t *)ps == *(uint16_t *)str;
            case 3: return (*(uint16_t *)ps == *(uint16_t *)str) && (ps[2] == str[2]);
            case 4: return *(uint32_t *)ps == *(uint32_t *)str;
            case 5: return (*(uint32_t *)ps == *(uint32_t *)str) && (ps[4] == str[4]);
            case 6: return (*(uint32_t *)ps == *(uint32_t *)str) && (*(uint16_t *)(ps+4) == *(uint16_t *)(str+4));
            case 7: return (*(uint32_t *)ps == *(uint32_t *)str) && (*(uint16_t *)(ps+4) == *(uint16_t *)(str+4)) && (ps[6] == str[6]);
            default: return memcmp(ps, str, len) == 0;
        }
    }
}

static inline KyString *ky_string_table_lookup_hashed(KyStringTable *table,
                                                      const char *str,
                                                      uint32_t len,
                                                      uint32_t hash) {
    if (unlikely(!table || !str)) return NULL;
    uint32_t idx = hash & table->mask;
    // Prefetch bucket pointer
    __builtin_prefetch(&table->buckets[idx], 0, 3);
    KyString *s = table->buckets[idx];
    while (s) {
        if (likely(s->hash == hash && ky_string_eq_fast(s, str, len))) {
            return s;
        }
        s = (KyString *)((uintptr_t)s->flags & ~0xF);
    }
    return NULL;
}

static inline KyString *ky_string_table_lookup(KyStringTable *table, const char *str) {
    uint32_t len;
    uint32_t hash = ky_string_hash_len(str, &len);
    return ky_string_table_lookup_hashed(table, str, len, hash);
}

KyStringTable *ky_string_table_create(uint32_t initial_size);
void ky_string_table_destroy(KyStringTable *table);
KyString *ky_string_table_intern(KyStringTable *table, const char *str);

#endif
