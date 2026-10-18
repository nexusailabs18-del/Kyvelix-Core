#ifndef KYJIT_LOOP_DETECT_FULL_H
#define KYJIT_LOOP_DETECT_FULL_H
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    uint32_t header_offset;
    uint32_t backedges;
    uint32_t threshold;
    int active;
} KyLoopHeader;

typedef struct {
    KyLoopHeader *headers;
    uint32_t count;
} KyLoopDetector;

static inline KyLoopDetector *ky_loop_detect_create(uint32_t max_loops) {
    KyLoopDetector *d = calloc(1, sizeof(*d));
    if (!d) return NULL;
    d->headers = calloc(max_loops, sizeof(KyLoopHeader));
    return d;
}

static inline void ky_loop_detect_register(KyLoopDetector *d, uint32_t header_offset, uint32_t threshold) {
    if (!d) return;
    d->headers[d->count].header_offset = header_offset;
    d->headers[d->count].threshold = threshold;
    d->count++;
}

static inline void ky_loop_detect_record_backedge(KyLoopDetector *d, uint32_t target_offset) {
    if (!d) return;
    for (uint32_t i = 0; i < d->count; i++) {
        if (d->headers[i].header_offset == target_offset) {
            d->headers[i].backedges++;
            if (!d->headers[i].active && d->headers[i].backedges >= d->headers[i].threshold) {
                d->headers[i].active = 1;
            }
            return;
        }
    }
}

static inline int ky_loop_detect_should_osr(KyLoopDetector *d, uint32_t header_offset) {
    if (!d) return 0;
    for (uint32_t i = 0; i < d->count; i++) {
        if (d->headers[i].header_offset == header_offset) return d->headers[i].active;
    }
    return 0;
}
#endif
