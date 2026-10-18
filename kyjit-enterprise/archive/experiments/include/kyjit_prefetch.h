#ifndef KYJIT_PREFETCH_H
#define KYJIT_PREFETCH_H

#include <stddef.h>

static inline void kyjit_prefetch_l1(const void *ptr) {
    __builtin_prefetch(ptr, 0, 1);
}

static inline void kyjit_prefetch_l2(const void *ptr) {
    __builtin_prefetch(ptr, 0, 2);
}

static inline void kyjit_prefetch_l3(const void *ptr) {
    __builtin_prefetch(ptr, 0, 3);
}

static inline void kyjit_prefetch_ahead(const void *ptr, int distance) {
    __builtin_prefetch((char*)ptr + distance, 0, 3);
}

#endif
