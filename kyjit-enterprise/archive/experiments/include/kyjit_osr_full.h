#ifndef KYJIT_OSR_FULL_H
#define KYJIT_OSR_FULL_H
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    uint32_t bytecode_offset;
    uint32_t backedges;
    uint32_t threshold;
    void *osr_code;
    int active;
} KyOSRLoop;

typedef struct {
    KyOSRLoop *loops;
    uint32_t count;
} KyOSRFull;

static inline KyOSRFull *ky_osr_full_create(uint32_t max_loops) {
    KyOSRFull *o = calloc(1, sizeof(*o));
    if (!o) return NULL;
    o->loops = calloc(max_loops, sizeof(KyOSRLoop));
    return o;
}

static inline void ky_osr_full_register_loop(KyOSRFull *o, uint32_t offset, uint32_t threshold) {
    if (!o) return;
    o->loops[o->count].bytecode_offset = offset;
    o->loops[o->count].threshold = threshold;
    o->count++;
}

static inline void ky_osr_full_record_backedge(KyOSRFull *o, uint32_t offset) {
    if (!o) return;
    for (uint32_t i=0;i<o->count;i++) {
        if (o->loops[i].bytecode_offset == offset) {
            o->loops[i].backedges++;
            if (o->loops[i].backedges >= o->loops[i].threshold && o->loops[i].active == 0)
                o->loops[i].active = 1;
            return;
        }
    }
}

static inline int ky_osr_full_should_compile(KyOSRFull *o, uint32_t offset) {
    if (!o) return 0;
    for (uint32_t i=0;i<o->count;i++)
        if (o->loops[i].bytecode_offset == offset) return o->loops[i].active;
    return 0;
}
#endif
