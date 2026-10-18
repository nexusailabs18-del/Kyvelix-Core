#ifndef KYJIT_SERIALIZE_H
#define KYJIT_SERIALIZE_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint8_t *data;
    uint32_t size;
    uint32_t version;
    uint32_t checksum;
} KySerializedCode;

static inline uint32_t ky_serialize_checksum(const uint8_t *data, uint32_t size) {
    uint32_t sum = 0;
    for (uint32_t i = 0; i < size; i++) sum += data[i];
    return sum;
}

static inline KySerializedCode *ky_code_serialize(const uint8_t *code, uint32_t size, uint32_t version) {
    KySerializedCode *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    s->data = malloc(size);
    if (!s->data) { free(s); return NULL; }
    memcpy(s->data, code, size);
    s->size = size;
    s->version = version;
    s->checksum = ky_serialize_checksum(code, size);
    return s;
}

static inline int ky_code_deserialize(KySerializedCode *s, uint8_t *out, uint32_t max_size) {
    if (!s || !out || s->size > max_size) return -1;
    if (ky_serialize_checksum(s->data, s->size) != s->checksum) return -2;
    memcpy(out, s->data, s->size);
    return (int)s->size;
}

static inline void ky_serialize_free(KySerializedCode *s) {
    if (!s) return;
    free(s->data);
    free(s);
}
#endif
