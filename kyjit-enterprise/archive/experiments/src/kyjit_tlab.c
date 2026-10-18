#include "kyjit_tlab.h"
#include <stdlib.h>
#include <string.h>

KyTLAB* kyjit_tlab_create(size_t size) {
    KyTLAB *t = malloc(sizeof(KyTLAB));
    t->start = malloc(size);
    t->current = t->start;
    t->end = t->start + size;
    return t;
}

/* Fast path: 2 instructions */
void* kyjit_tlab_alloc(KyTLAB *tlab, size_t size) {
    size = (size + 7) & ~7;  /* Align 8 */
    if (tlab->current + size > tlab->end) return NULL;
    void *p = tlab->current;
    tlab->current += size;    /* Increment pointer = allocation! */
    return p;
}

void kyjit_tlab_reset(KyTLAB *tlab) {
    tlab->current = tlab->start;
}

void kyjit_tlab_destroy(KyTLAB *tlab) {
    free(tlab->start);
    free(tlab);
}
