#ifndef KYJIT_VERSION_H
#define KYJIT_VERSION_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    void *code;
    uint32_t version;
    uint32_t created_at;
    int active;
} KyCodeVersion;

typedef struct {
    KyCodeVersion *versions;
    uint32_t count;
    uint32_t capacity;
} KyVersionManager;

static inline KyVersionManager *ky_version_create(uint32_t capacity) {
    KyVersionManager *v = calloc(1, sizeof(*v));
    if (!v) return NULL;
    v->versions = calloc(capacity, sizeof(KyCodeVersion));
    v->capacity = capacity;
    return v;
}

static inline int ky_version_add(KyVersionManager *v, void *code) {
    if (!v || v->count >= v->capacity) return -1;
    v->versions[v->count].code = code;
    v->versions[v->count].version = v->count + 1;
    v->versions[v->count].created_at = 0;
    v->versions[v->count].active = 1;
    return (int)v->count++;
}

static inline int ky_version_activate(KyVersionManager *v, uint32_t idx) {
    if (!v || idx >= v->count) return -1;
    for (uint32_t i = 0; i < v->count; i++) v->versions[i].active = 0;
    v->versions[idx].active = 1;
    return 0;
}

static inline void *ky_version_get_active(KyVersionManager *v) {
    if (!v) return NULL;
    for (uint32_t i = 0; i < v->count; i++) {
        if (v->versions[i].active) return v->versions[i].code;
    }
    return NULL;
}
#endif
