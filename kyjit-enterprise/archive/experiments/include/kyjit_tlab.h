#ifndef KYJIT_TLAB_H
#define KYJIT_TLAB_H
#include <stddef.h>

typedef struct {
    unsigned char *current;   /* Next free byte */
    unsigned char *end;       /* End of buffer */
    unsigned char *start;     /* For reset/free */
} KyTLAB;

KyTLAB* kyjit_tlab_create(size_t size);
void* kyjit_tlab_alloc(KyTLAB *tlab, size_t size);
void kyjit_tlab_reset(KyTLAB *tlab);
void kyjit_tlab_destroy(KyTLAB *tlab);

#endif
