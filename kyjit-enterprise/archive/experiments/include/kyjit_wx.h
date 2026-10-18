#ifndef KYJIT_WX_H
#define KYJIT_WX_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

typedef struct {
    void *base;
    size_t size;
    int writable;
} KyWXMemory;

static inline KyWXMemory *ky_wx_alloc(size_t size) {
    KyWXMemory *wx = calloc(1, sizeof(*wx));
    if (!wx) return NULL;
    wx->base = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (wx->base == MAP_FAILED) { free(wx); return NULL; }
    wx->size = size;
    wx->writable = 1;
    return wx;
}

static inline void ky_wx_make_executable(KyWXMemory *wx) {
    if (!wx) return;
    if (wx->writable) {
        mprotect(wx->base, wx->size, PROT_READ | PROT_EXEC);
        wx->writable = 0;
    }
}

static inline void ky_wx_make_writable(KyWXMemory *wx) {
    if (!wx) return;
    if (!wx->writable) {
        mprotect(wx->base, wx->size, PROT_READ | PROT_WRITE);
        wx->writable = 1;
    }
}

static inline void ky_wx_free(KyWXMemory *wx) {
    if (!wx) return;
    munmap(wx->base, wx->size);
    free(wx);
}
#endif
