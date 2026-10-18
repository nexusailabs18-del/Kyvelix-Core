#define _GNU_SOURCE
#include <stdlib.h>
#include <sched.h>
#include <string.h>
#include "kyjit_numa.h"

/* NUMA-aware allocation with GRACEFUL fallback
 * Works WITHOUT numa.h — uses syscalls directly when available.
 */

#if defined(__linux__) && defined(__has_include)
#if __has_include(<numa.h>)
#define KYJIT_HAS_NUMA 1
#include <numa.h>
#include <numaif.h>
#endif
#endif

#ifndef KYJIT_HAS_NUMA
#define KYJIT_HAS_NUMA 0
#endif

void* kyjit_numa_alloc_local(size_t size) {
#if KYJIT_HAS_NUMA
    if (numa_available() >= 0) {
        void *ptr = numa_alloc_local(size);
        if (ptr) return ptr;
    }
#endif
    void *ptr = NULL;
    if (posix_memalign(&ptr, 64, size) != 0) return NULL;
    return ptr;
}

void* kyjit_numa_alloc_node(size_t size, int node) {
#if KYJIT_HAS_NUMA
    if (numa_available() >= 0) {
        void *ptr = numa_alloc_onnode(size, node);
        if (ptr) return ptr;
    }
#endif
    void *ptr = NULL;
    if (posix_memalign(&ptr, 64, size) != 0) return NULL;
    return ptr;
}

int kyjit_numa_current_node(void) {
#if KYJIT_HAS_NUMA
    if (numa_available() >= 0) {
        return numa_node_of_cpu(sched_getcpu());
    }
#endif
    return 0;
}

int kyjit_numa_available(void) {
#if KYJIT_HAS_NUMA
    return numa_available() >= 0;
#else
    return 0;
#endif
}

void kyjit_numa_free(void *ptr, size_t size) {
#if KYJIT_HAS_NUMA
    if (numa_available() >= 0) {
        numa_free(ptr, size);
        return;
    }
#endif
    free(ptr);
}
