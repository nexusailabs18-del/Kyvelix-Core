#include "kyjit_string.h"
#include <stdlib.h>
#include <string.h>

KyString *ky_string_create(const char *str) {
    uint32_t len = 0;
    uint32_t hash = ky_string_hash_len(str, &len);
    return ky_string_create_hashed(str, len, hash);
}

KyString *ky_string_create_hashed(const char *str, uint32_t len, uint32_t hash) {
    KyString *s = (KyString *)malloc(sizeof(KyString) + len + 1);
    if (!s) return NULL;
    s->length = len;
    s->hash = hash;
    s->flags = KY_STRING_FLAG_ASCII; /* assume ASCII for now */
    memcpy(s->data, str, len);
    s->data[len] = '\0';
    return s;
}

int ky_string_compare(KyString *a, KyString *b) {
    if (a->hash != b->hash) return a->hash - b->hash;
    if (a->length != b->length) return a->length - b->length;
    return memcmp(a->data, b->data, a->length);
}

KyString *ky_string_concat(KyString *a, KyString *b) {
    uint32_t new_len = a->length + b->length;
    KyString *s = (KyString *)malloc(sizeof(KyString) + new_len + 1);
    if (!s) return NULL;
    s->length = new_len;
    memcpy(s->data, a->data, a->length);
    memcpy(s->data + a->length, b->data, b->length);
    s->data[new_len] = '\0';
    /* compute new hash */
    uint32_t hash = 2166136261u;
    for (uint32_t i = 0; i < new_len; i++) {
        hash ^= (uint8_t)s->data[i];
        hash *= 16777619u;
    }
    s->hash = hash;
    s->flags = a->flags & b->flags; /* inherit ASCII flag if both ASCII */
    return s;
}

KyString *ky_string_slice(KyString *src, uint32_t start, uint32_t end) {
    if (start > end || end > src->length) return NULL;
    uint32_t len = end - start;
    KyString *s = (KyString *)malloc(sizeof(KyString) + len + 1);
    if (!s) return NULL;
    s->length = len;
    memcpy(s->data, src->data + start, len);
    s->data[len] = '\0';
    /* compute hash */
    uint32_t hash = 2166136261u;
    for (uint32_t i = 0; i < len; i++) {
        hash ^= (uint8_t)s->data[i];
        hash *= 16777619u;
    }
    s->hash = hash;
    s->flags = src->flags;
    return s;
}
