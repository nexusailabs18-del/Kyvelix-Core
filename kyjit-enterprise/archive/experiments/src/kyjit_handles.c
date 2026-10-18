#include "kyjit_handles.h"
#include <stdlib.h>
#include <string.h>
#include <pthread.h>

#define GEN_BITS   8
#define IDX_BITS   (32 - GEN_BITS)
#define GEN_MASK   ((1u << GEN_BITS) - 1)
#define IDX_MASK   ((1u << IDX_BITS) - 1)
#define MAX_HANDLES (1u << IDX_BITS)

typedef struct {
    void   *ptr;
    uint8_t generation;
    uint8_t used;
} HandleEntry;

struct KyjitHandleTable {
    HandleEntry    *entries;
    size_t          capacity;
    size_t          live_count;
    uint32_t        next_free;
    pthread_mutex_t lock;
};

static inline KyjitHandle make_handle(uint32_t index, uint8_t gen) {
    return (KyjitHandle)(((uint32_t)gen << IDX_BITS) | index);
}

KyjitHandleTable *kyjit_handle_table_create(size_t initial_capacity) {
    KyjitHandleTable *t = calloc(1, sizeof(*t));
    if (!t) return NULL;

    if (initial_capacity == 0) initial_capacity = 1024;
    if (initial_capacity > MAX_HANDLES) initial_capacity = MAX_HANDLES;

    t->entries = calloc(initial_capacity, sizeof(HandleEntry));
    if (!t->entries) {
        free(t);
        return NULL;
    }

    t->capacity  = initial_capacity;
    t->live_count = 0;
    t->next_free = 0;
    pthread_mutex_init(&t->lock, NULL);
    return t;
}

void *kyjit_handle_get(KyjitHandleTable *t, KyjitHandle handle) {
    if (!t || handle == KYJIT_INVALID_HANDLE) return NULL;

    uint32_t index = handle & IDX_MASK;
    uint8_t  gen   = (handle >> IDX_BITS) & GEN_MASK;

    pthread_mutex_lock(&t->lock);
    void *result = NULL;
    if (index < t->capacity &&
        t->entries[index].used &&
        t->entries[index].generation == gen) {
        result = t->entries[index].ptr;
    }
    pthread_mutex_unlock(&t->lock);
    return result;
}

KyjitHandle kyjit_handle_create(KyjitHandleTable *t, void *ptr) {
    if (!t) return KYJIT_INVALID_HANDLE;

    pthread_mutex_lock(&t->lock);

    uint32_t idx   = t->next_free;
    uint32_t start = idx;
    int found = 0;

    do {
        if (!t->entries[idx].used) {
            found = 1;
            break;
        }
        idx = (idx + 1) % t->capacity;
    } while (idx != start);

    if (!found) {
        if (t->capacity >= MAX_HANDLES) {
            pthread_mutex_unlock(&t->lock);
            return KYJIT_INVALID_HANDLE;
        }

        size_t new_cap = t->capacity * 2;
        if (new_cap > MAX_HANDLES) new_cap = MAX_HANDLES;

        HandleEntry *new_entries = realloc(t->entries,
                                           new_cap * sizeof(HandleEntry));
        if (!new_entries) {
            pthread_mutex_unlock(&t->lock);
            return KYJIT_INVALID_HANDLE;
        }

        memset(&new_entries[t->capacity], 0,
               (new_cap - t->capacity) * sizeof(HandleEntry));

        t->entries  = new_entries;
        idx         = t->capacity;
        t->capacity = new_cap;
        found = 1;
    }

    t->entries[idx].ptr  = ptr;
    t->entries[idx].used = 1;
    t->entries[idx].generation++;
    if (t->entries[idx].generation == 0)
        t->entries[idx].generation = 1;

    t->live_count++;
    t->next_free = (idx + 1) % t->capacity;

    KyjitHandle h = make_handle(idx, t->entries[idx].generation);
    pthread_mutex_unlock(&t->lock);
    return h;
}

void kyjit_handle_destroy(KyjitHandleTable *t, KyjitHandle handle) {
    if (!t || handle == KYJIT_INVALID_HANDLE) return;

    uint32_t index = handle & IDX_MASK;
    uint8_t  gen   = (handle >> IDX_BITS) & GEN_MASK;

    pthread_mutex_lock(&t->lock);
    if (index < t->capacity &&
        t->entries[index].used &&
        t->entries[index].generation == gen) {
        t->entries[index].used = 0;
        t->entries[index].ptr  = NULL;
        if (t->live_count > 0) t->live_count--;
        if (t->next_free > index) t->next_free = index;
    }
    pthread_mutex_unlock(&t->lock);
}

void kyjit_handle_clear(KyjitHandleTable *t) {
    if (!t) return;
    pthread_mutex_lock(&t->lock);
    memset(t->entries, 0, t->capacity * sizeof(HandleEntry));
    t->live_count = 0;
    t->next_free  = 0;
    pthread_mutex_unlock(&t->lock);
}

void kyjit_handle_table_free(KyjitHandleTable *t) {
    if (!t) return;
    pthread_mutex_destroy(&t->lock);
    free(t->entries);
    free(t);
}

size_t kyjit_handle_count(KyjitHandleTable *t) {
    if (!t) return 0;
    pthread_mutex_lock(&t->lock);
    size_t c = t->live_count;
    pthread_mutex_unlock(&t->lock);
    return c;
}
