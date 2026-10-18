#include "kyjit_work_steal.h"
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

struct KyWorkSteal {
    size_t capacity;
    size_t mask;
    atomic_size_t top;
    atomic_size_t bottom;
    void **items;
};

KyWorkSteal *kyjit_ws_create(size_t capacity) {
    if (capacity == 0 || (capacity & (capacity - 1)) != 0)
        return NULL;
    KyWorkSteal *ws = (KyWorkSteal*)malloc(sizeof(KyWorkSteal));
    if (!ws) return NULL;
    ws->capacity = capacity;
    ws->mask = capacity - 1;
    ws->items = (void**)malloc(capacity * sizeof(void*));
    if (!ws->items) { free(ws); return NULL; }
    atomic_init(&ws->top, 0);
    atomic_init(&ws->bottom, 0);
    return ws;
}

int kyjit_ws_push(KyWorkSteal *ws, void *item) {
    if (!ws) return -1;
    size_t bottom = atomic_load_explicit(&ws->bottom, memory_order_relaxed);
    size_t top = atomic_load_explicit(&ws->top, memory_order_acquire);
    if (bottom - top >= ws->capacity) return -1;
    ws->items[bottom & ws->mask] = item;
    atomic_store_explicit(&ws->bottom, bottom + 1, memory_order_release);
    return 0;
}

int kyjit_ws_pop(KyWorkSteal *ws, void **item) {
    if (!ws || !item) return -1;
    size_t bottom = atomic_load_explicit(&ws->bottom, memory_order_relaxed);
    if (bottom == 0) return -1;
    bottom--;
    atomic_store_explicit(&ws->bottom, bottom, memory_order_release);
    size_t top = atomic_load_explicit(&ws->top, memory_order_acquire);
    if (top <= bottom) {
        *item = ws->items[bottom & ws->mask];
        if (top != bottom) return 0;
        size_t expected_top = top;
        if (!atomic_compare_exchange_weak_explicit(
                &ws->top, &expected_top, top + 1,
                memory_order_release, memory_order_relaxed)) {
            *item = NULL;
            return -1;
        }
        atomic_store_explicit(&ws->bottom, bottom + 1, memory_order_relaxed);
        return 0;
    }
    atomic_store_explicit(&ws->bottom, bottom + 1, memory_order_relaxed);
    *item = NULL;
    return -1;
}

int kyjit_ws_steal(KyWorkSteal *ws, void **item) {
    if (!ws || !item) return -1;
    size_t top = atomic_load_explicit(&ws->top, memory_order_acquire);
    size_t bottom = atomic_load_explicit(&ws->bottom, memory_order_acquire);
    if (top >= bottom) return -1;
    void *value = ws->items[top & ws->mask];
    if (!atomic_compare_exchange_weak_explicit(
            &ws->top, &top, top + 1,
            memory_order_release, memory_order_relaxed)) {
        return -1;
    }
    *item = value;
    return 0;
}

void kyjit_ws_destroy(KyWorkSteal *ws) {
    if (!ws) return;
    free(ws->items);
    free(ws);
}
