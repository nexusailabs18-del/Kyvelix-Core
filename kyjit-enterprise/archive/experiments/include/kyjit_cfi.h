#ifndef KYJIT_CFI_H
#define KYJIT_CFI_H
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    uint64_t canary;
    void *targets[256];
    uint32_t count;
} KyCFI;

static inline KyCFI *ky_cfi_create(void) {
    KyCFI *c = calloc(1, sizeof(*c));
    if (!c) return NULL;
    c->canary = 0xDEADBEEFCAFEF00DULL;
    return c;
}

static inline int ky_cfi_register_target(KyCFI *c, void *target) {
    if (!c || c->count >= 256) return -1;
    c->targets[c->count++] = target;
    return (int)(c->count - 1);
}

static inline int ky_cfi_validate(KyCFI *c, void *target) {
    if (!c || c->canary != 0xDEADBEEFCAFEF00DULL) return 0;
    for (uint32_t i = 0; i < c->count; i++) {
        if (c->targets[i] == target) return 1;
    }
    return 0;
}
#endif
