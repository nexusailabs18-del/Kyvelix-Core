#include "kyjit_string_table.h"
#include <stdlib.h>

KyStringTable *ky_string_table_create(uint32_t initial_size) {
    KyStringTable *table = calloc(1, sizeof(KyStringTable));
    if (!table) return NULL;
    uint32_t size = 16;
    while (size < initial_size) size <<= 1;
    table->size = size;
    table->mask = size - 1;
    table->count = 0;
    table->buckets = calloc(size, sizeof(KyString *));
    if (!table->buckets) {
        free(table);
        return NULL;
    }
    return table;
}

void ky_string_table_destroy(KyStringTable *table) {
    if (!table) return;
    for (uint32_t i = 0; i < table->size; i++) {
        KyString *s = table->buckets[i];
        while (s) {
            KyString *next = (KyString *)((uintptr_t)s->flags & ~0xF);
            free(s);
            s = next;
        }
    }
    free(table->buckets);
    free(table);
}

KyString *ky_string_table_intern(KyStringTable *table, const char *str) {
    if (!table || !str) return NULL;

    // Compute hash + length once
    uint32_t len;
    uint32_t hash = ky_string_hash_len(str, &len);

    // Lookup with precomputed hash
    KyString *existing = ky_string_table_lookup_hashed(table, str, len, hash);
    if (existing) return existing;

    // Create new string using precomputed hash (avoid second hash)
    KyString *s = ky_string_create_hashed(str, len, hash);
    if (!s) return NULL;

    // Insert at head of bucket
    uint32_t idx = hash & table->mask;
    s->flags = (uint32_t)((uintptr_t)table->buckets[idx] & 0xFFFFFFF0) | (s->flags & 0xF);
    table->buckets[idx] = s;
    table->count++;
    return s;
}
