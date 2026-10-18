#ifndef KYJIT_GC_H
#define KYJIT_GC_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include <pthread.h>

#define KYJIT_GC_YOUNG_SIZE     (4 * 1024 * 1024)
#define KYJIT_GC_OLD_INITIAL    (8 * 1024 * 1024)
#define KYJIT_GC_TLAB_SIZE      (64 * 1024)
#define KYJIT_GC_MAX_ROOTS      4096

typedef struct kyjit_gc_stats {
    uint64_t minor_gc_count;
    uint64_t major_gc_count;
    uint64_t bytes_allocated;
    uint64_t bytes_freed;
    uint64_t total_objects;
} kyjit_gc_stats_t;

typedef struct kyjit_gc_heap kyjit_gc_heap_t;

kyjit_gc_heap_t* kyjit_gc_create(void);
void kyjit_gc_destroy(kyjit_gc_heap_t* heap);
void* kyjit_gc_alloc(kyjit_gc_heap_t* heap, size_t size);
void kyjit_gc_collect(kyjit_gc_heap_t* heap, bool major);
void kyjit_gc_add_root(kyjit_gc_heap_t* heap, void** root);
void kyjit_gc_remove_root(kyjit_gc_heap_t* heap, void** root);
void kyjit_gc_write_barrier(kyjit_gc_heap_t* heap, void* obj, void* field);
void kyjit_gc_get_stats(kyjit_gc_heap_t* heap, kyjit_gc_stats_t* stats);
void kyjit_gc_print_stats(kyjit_gc_heap_t* heap);

#endif
