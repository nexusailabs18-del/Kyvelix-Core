#ifndef KYJIT_SELFMOD_IC_H
#define KYJIT_SELFMOD_IC_H
#include <stdint.h>

typedef struct {
    uint8_t *code;          /* Executable code */
    uint32_t cached_shape;  /* Last seen shape */
    uint32_t cached_offset; /* Cached property offset */
    int code_size;
} KySelfModIC;

KySelfModIC* kyjit_selfmod_ic_create(void);
int kyjit_selfmod_ic_lookup(KySelfModIC *ic, uint32_t shape, void *obj);
void kyjit_selfmod_ic_update(KySelfModIC *ic, uint32_t shape, uint32_t offset);
void kyjit_selfmod_ic_destroy(KySelfModIC *ic);

#endif
