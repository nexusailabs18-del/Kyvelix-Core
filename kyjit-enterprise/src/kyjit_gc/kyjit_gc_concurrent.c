#include <unistd.h>
#include "kyjit_gc.h"
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <stdatomic.h>

/* Concurrent GC: background marking thread, no stop-the-world */

typedef struct concurrent_gc {
    kyjit_gc_heap_t *heap;
    pthread_t thread;
    atomic_bool running;
    atomic_bool should_stop;
    atomic_int collections_done;
} concurrent_gc_t;

static void* gc_thread_func(void* arg) {
    concurrent_gc_t *gc = (concurrent_gc_t*)arg;
    
    while (!atomic_load(&gc->should_stop)) {
        /* Check if GC needed */
        kyjit_gc_stats_t stats;
        kyjit_gc_get_stats(gc->heap, &stats);
        
        if (stats.bytes_allocated > 4 * 1024 * 1024) {
            /* Run concurrent GC */
            kyjit_gc_collect(gc->heap, false);
            atomic_fetch_add(&gc->collections_done, 1);
        }
        
        /* Sleep briefly */
        usleep(1000);  /* 1ms */
    }
    
    return NULL;
}

concurrent_gc_t* kyjit_gc_concurrent_create(kyjit_gc_heap_t *heap) {
    concurrent_gc_t *gc = calloc(1, sizeof(concurrent_gc_t));
    gc->heap = heap;
    atomic_store(&gc->running, true);
    atomic_store(&gc->should_stop, false);
    atomic_store(&gc->collections_done, 0);
    
    pthread_create(&gc->thread, NULL, gc_thread_func, gc);
    
    return gc;
}

void kyjit_gc_concurrent_stop(concurrent_gc_t *gc) {
    atomic_store(&gc->should_stop, true);
    pthread_join(gc->thread, NULL);
    atomic_store(&gc->running, false);
}

int kyjit_gc_concurrent_collections(concurrent_gc_t *gc) {
    return atomic_load(&gc->collections_done);
}
