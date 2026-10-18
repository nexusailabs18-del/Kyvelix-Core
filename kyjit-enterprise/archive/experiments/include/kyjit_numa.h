#ifndef KYJIT_NUMA_H
#define KYJIT_NUMA_H
#include <stddef.h>

void* kyjit_numa_alloc_local(size_t size);
void* kyjit_numa_alloc_node(size_t size, int node);
int kyjit_numa_current_node(void);
int kyjit_numa_available(void);
void kyjit_numa_free(void *ptr, size_t size);

#endif
