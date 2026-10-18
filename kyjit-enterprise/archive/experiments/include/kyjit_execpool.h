#ifndef KYJIT_EXECPOOL_H
#define KYJIT_EXECPOOL_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

typedef struct {
    void *base;
    size_t size;
    size_t used;
} KyExecPool;

static inline KyExecPool *ky_execpool_create(size_t size) {
    KyExecPool *p = calloc(1, sizeof(*p));
    if (!p) return NULL;
    p->base = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p->base == MAP_FAILED) { free(p); return NULL; }
    p->size = size;
    p->used = 0;
    return p;
}

static inline void *ky_execpool_alloc(KyExecPool *p, size_t size) {
    if (!p || p->used + size > p->size) return NULL;
    void *ptr = (char *)p->base + p->used;
    p->used += size;
    return ptr;
}

static inline void ky_execpool_make_exec(KyExecPool *p) {
    if (!p) return;
    mprotect(p->base, p->size, PROT_READ | PROT_EXEC);
}

static inline void ky_execpool_free(KyExecPool *p) {
    if (!p) return;
    munmap(p->base, p->size);
    free(p);
}
#endif
