#include "kyjit_selfmod_ic.h"
#include <sys/mman.h>
#include <string.h>
#include <stdlib.h>

KySelfModIC* kyjit_selfmod_ic_create(void) {
    KySelfModIC *ic = malloc(sizeof(KySelfModIC));
    /* Allocate executable memory for the IC stub */
    ic->code = mmap(NULL, 64, PROT_READ|PROT_WRITE|PROT_EXEC,
                    MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    ic->cached_shape = 0;
    ic->cached_offset = 0;
    ic->code_size = 0;
    return ic;
}

/* Execute lookup — returns property value */
int kyjit_selfmod_ic_lookup(KySelfModIC *ic, uint32_t shape, void *obj) {
    /* Fast path: just compare cached shape */
    if (ic->cached_shape == shape) {
        return *(int*)((char*)obj + ic->cached_offset);
    }
    return -1;  /* Miss */
}

/* Update cache on miss */
void kyjit_selfmod_ic_update(KySelfModIC *ic, uint32_t shape, uint32_t offset) {
    ic->cached_shape = shape;
    ic->cached_offset = offset;
}

void kyjit_selfmod_ic_destroy(KySelfModIC *ic) {
    munmap(ic->code, 64);
    free(ic);
}
