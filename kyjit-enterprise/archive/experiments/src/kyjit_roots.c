#include "kyjit_roots.h"
#include <stdlib.h>
#include <string.h>
#include <pthread.h>

struct KyjitRootSet {
    void          **items;
    size_t          count;
    size_t          capacity;
    pthread_mutex_t lock;
};

KyjitRootSet *kyjit_roots_create(size_t initial_capacity) {
    KyjitRootSet *rs = calloc(1, sizeof(*rs));
    if (!rs) return NULL;

    if (initial_capacity == 0) initial_capacity = 64;
    rs->items = calloc(initial_capacity, sizeof(void*));
    if (!rs->items) {
        free(rs);
        return NULL;
    }
    rs->capacity = initial_capacity;
    rs->count = 0;
    pthread_mutex_init(&rs->lock, NULL);
    return rs;
}

static int roots_grow(KyjitRootSet *rs) {
    size_t new_cap = rs->capacity * 2;
    void **new_items = realloc(rs->items, new_cap * sizeof(void*));
    if (!new_items) return -1;
    rs->items = new_items;
    rs->capacity = new_cap;
    return 0;
}

int kyjit_roots_add(KyjitRootSet *rs, void *ptr) {
    if (!rs || !ptr) return -1;

    pthread_mutex_lock(&rs->lock);

    /* Avoid duplicates */
    for (size_t i = 0; i < rs->count; i++) {
        if (rs->items[i] == ptr) {
            pthread_mutex_unlock(&rs->lock);
            return 0;
        }
    }

    if (rs->count >= rs->capacity) {
        if (roots_grow(rs) != 0) {
            pthread_mutex_unlock(&rs->lock);
            return -1;
        }
    }

    rs->items[rs->count++] = ptr;
    pthread_mutex_unlock(&rs->lock);
    return 0;
}

int kyjit_roots_remove(KyjitRootSet *rs, void *ptr) {
    if (!rs || !ptr) return -1;

    pthread_mutex_lock(&rs->lock);
    int removed = -1;
    for (size_t i = 0; i < rs->count; i++) {
        if (rs->items[i] == ptr) {
            /* Swap with last and shrink count */
            rs->items[i] = rs->items[rs->count - 1];
            rs->items[rs->count - 1] = NULL;
            rs->count--;
            removed = 0;
            break;
        }
    }
    pthread_mutex_unlock(&rs->lock);
    return removed;
}

void kyjit_roots_clear(KyjitRootSet *rs) {
    if (!rs) return;
    pthread_mutex_lock(&rs->lock);
    memset(rs->items, 0, rs->count * sizeof(void*));
    rs->count = 0;
    pthread_mutex_unlock(&rs->lock);
}

size_t kyjit_roots_count(KyjitRootSet *rs) {
    if (!rs) return 0;
    pthread_mutex_lock(&rs->lock);
    size_t c = rs->count;
    pthread_mutex_unlock(&rs->lock);
    return c;
}

void *kyjit_roots_get(KyjitRootSet *rs, size_t index) {
    if (!rs) return NULL;
    pthread_mutex_lock(&rs->lock);
    void *ptr = (index < rs->count) ? rs->items[index] : NULL;
    pthread_mutex_unlock(&rs->lock);
    return ptr;
}

void kyjit_roots_free(KyjitRootSet *rs) {
    if (!rs) return;
    pthread_mutex_destroy(&rs->lock);
    free(rs->items);
    free(rs);
}
