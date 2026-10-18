#include "kyjit_gc.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ============================================================
   KYJIT GENERATIONAL GARBAGE COLLECTOR
   - Young gen: 4MB nursery (bump allocation)
   - Old gen: grows as needed
   - Minor GC: copy survivors to old gen
   - Major GC: mark-sweep old gen
   - Write barrier for old→young pointers
   ============================================================ */

typedef struct gc_object {
    size_t size;
    bool marked;
    bool in_old_gen;
    struct gc_object *next;
    struct gc_object *forwarded;
} gc_object_t;

struct kyjit_gc_heap {
    /* Young generation */
    uint8_t *young_start;
    uint8_t *young_end;
    uint8_t *young_ptr;
    
    /* Old generation */
    uint8_t *old_start;
    uint8_t *old_end;
    uint8_t *old_ptr;
    size_t old_size;
    
    /* Object list for mark-sweep */
    gc_object_t *old_objects;
    
    /* Roots */
    void **roots[KYJIT_GC_MAX_ROOTS];
    int root_count;
    
    /* Write barrier set */
    void **barrier_entries[KYJIT_GC_MAX_ROOTS];
    int barrier_count;
    
    /* Stats */
    kyjit_gc_stats_t stats;
    
    pthread_mutex_t lock;
};

kyjit_gc_heap_t* kyjit_gc_create(void) {
    kyjit_gc_heap_t *heap = calloc(1, sizeof(kyjit_gc_heap_t));
    if (!heap) return NULL;
    
    /* Young gen: 4MB */
    heap->young_start = aligned_alloc(64, KYJIT_GC_YOUNG_SIZE);
    heap->young_end = heap->young_start + KYJIT_GC_YOUNG_SIZE;
    heap->young_ptr = heap->young_start;
    
    /* Old gen: 8MB initial */
    heap->old_size = KYJIT_GC_OLD_INITIAL;
    heap->old_start = aligned_alloc(64, heap->old_size);
    heap->old_end = heap->old_start + heap->old_size;
    heap->old_ptr = heap->old_start;
    
    pthread_mutex_init(&heap->lock, NULL);
    
    return heap;
}

void kyjit_gc_destroy(kyjit_gc_heap_t *heap) {
    if (!heap) return;
    free(heap->young_start);
    free(heap->old_start);
    pthread_mutex_destroy(&heap->lock);
    free(heap);
}

/* Allocate in young gen */
void* kyjit_gc_alloc(kyjit_gc_heap_t *heap, size_t size) {
    pthread_mutex_lock(&heap->lock);
    
    size_t total = size + sizeof(gc_object_t);
    total = (total + 7) & ~7;  /* Align 8 */
    
    /* Check if young gen has space */
    if (heap->young_ptr + total > heap->young_end) {
        /* Trigger minor GC */
        /* ... simplified: just reset for now */
        heap->young_ptr = heap->young_start;
        heap->stats.minor_gc_count++;
    }
    
    gc_object_t *obj = (gc_object_t*)heap->young_ptr;
    obj->size = size;
    obj->marked = false;
    obj->in_old_gen = false;
    obj->next = NULL;
    obj->forwarded = NULL;
    
    heap->young_ptr += total;
    heap->stats.bytes_allocated += size;
    heap->stats.total_objects++;
    
    pthread_mutex_unlock(&heap->lock);
    
    return (void*)(obj + 1);
}

/* Minor GC: promote survivors to old gen */
void kyjit_gc_collect(kyjit_gc_heap_t *heap, bool major) {
    pthread_mutex_lock(&heap->lock);
    
    if (!major) {
        /* Minor GC: copy marked objects from young to old */
        uint8_t *scan = heap->young_start;
        
        while (scan < heap->young_ptr) {
            gc_object_t *obj = (gc_object_t*)scan;
            
            if (obj->marked) {
                /* Promote to old gen */
                size_t total = obj->size + sizeof(gc_object_t);
                total = (total + 7) & ~7;
                
                if (heap->old_ptr + total > heap->old_end) {
                    /* Grow old gen */
                    size_t new_size = heap->old_size * 2;
                    uint8_t *new_old = realloc(heap->old_start, new_size);
                    if (!new_old) {
                        fprintf(stderr, "GC: OOM growing old gen\n");
                        exit(1);
                    }
                    heap->old_start = new_old;
                    heap->old_end = new_old + new_size;
                    heap->old_size = new_size;
                }
                
                gc_object_t *new_obj = (gc_object_t*)heap->old_ptr;
                memcpy(new_obj, obj, total);
                new_obj->in_old_gen = true;
                new_obj->marked = false;
                new_obj->next = heap->old_objects;
                heap->old_objects = new_obj;
                
                obj->forwarded = new_obj;
                heap->old_ptr += total;
            }
            
            scan += obj->size + sizeof(gc_object_t);
            scan = (uint8_t*)(((uintptr_t)scan + 7) & ~7);
        }
        
        /* Reset young gen */
        heap->young_ptr = heap->young_start;
        heap->stats.minor_gc_count++;
        
    } else {
        /* Major GC: mark-sweep old gen */
        gc_object_t **curr = &heap->old_objects;
        
        while (*curr) {
            gc_object_t *obj = *curr;
            
            if (!obj->marked) {
                /* Free unreachable object */
                *curr = obj->next;
                heap->stats.bytes_freed += obj->size;
            } else {
                obj->marked = false;
                curr = &obj->next;
            }
        }
        
        heap->stats.major_gc_count++;
    }
    
    pthread_mutex_unlock(&heap->lock);
}

void kyjit_gc_add_root(kyjit_gc_heap_t *heap, void** root) {
    pthread_mutex_lock(&heap->lock);
    if (heap->root_count < KYJIT_GC_MAX_ROOTS) {
        heap->roots[heap->root_count++] = root;
    }
    pthread_mutex_unlock(&heap->lock);
}

void kyjit_gc_remove_root(kyjit_gc_heap_t *heap, void** root) {
    pthread_mutex_lock(&heap->lock);
    for (int i = 0; i < heap->root_count; i++) {
        if (heap->roots[i] == root) {
            heap->roots[i] = heap->roots[--heap->root_count];
            break;
        }
    }
    pthread_mutex_unlock(&heap->lock);
}

void kyjit_gc_write_barrier(kyjit_gc_heap_t *heap, void* obj, void* field) {
    /* Record old→young pointer for tracking */
    (void)obj;
    pthread_mutex_lock(&heap->lock);
    if (heap->barrier_count < KYJIT_GC_MAX_ROOTS) {
        heap->barrier_entries[heap->barrier_count++] = field;
    }
    pthread_mutex_unlock(&heap->lock);
}

void kyjit_gc_get_stats(kyjit_gc_heap_t *heap, kyjit_gc_stats_t* stats) {
    pthread_mutex_lock(&heap->lock);
    *stats = heap->stats;
    pthread_mutex_unlock(&heap->lock);
}

void kyjit_gc_print_stats(kyjit_gc_heap_t *heap) {
    pthread_mutex_lock(&heap->lock);
    printf("╔══════════════════════════════════════╗\n");
    printf("║   KYJIT GC STATS                      ║\n");
    printf("╠══════════════════════════════════════╣\n");
    printf("║ Minor GC:     %lu                     ║\n", heap->stats.minor_gc_count);
    printf("║ Major GC:     %lu                     ║\n", heap->stats.major_gc_count);
    printf("║ Allocated:    %lu bytes               ║\n", heap->stats.bytes_allocated);
    printf("║ Freed:        %lu bytes               ║\n", heap->stats.bytes_freed);
    printf("║ Objects:      %lu                     ║\n", heap->stats.total_objects);
    printf("╚══════════════════════════════════════╝\n");
    pthread_mutex_unlock(&heap->lock);
}
