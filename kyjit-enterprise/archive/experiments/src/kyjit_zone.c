#include "kyjit_zone.h"
#include <stdlib.h>
#include <string.h>

#define DEFAULT_ZONE_SIZE (64 * 1024)       /* 64 KB */
#define SEGMENT_ALIGN     16
#define MAX_SEGMENT_SIZE  (1u << 30)        /* 1 GB cap */

typedef struct ZoneSegment {
    unsigned char      *start;
    unsigned char      *current;
    unsigned char      *end;
    size_t              size;
    struct ZoneSegment *next;
} ZoneSegment;

struct KyjitZone {
    ZoneSegment *head;
    ZoneSegment *tail;
    size_t       total_capacity;
    size_t       total_used;
};

static unsigned char *align_ptr(unsigned char *p, size_t alignment) {
    uintptr_t v = (uintptr_t)p;
    uintptr_t aligned = (v + alignment - 1) & ~(alignment - 1);
    return (unsigned char *)aligned;
}

static ZoneSegment *segment_create(size_t size) {
    ZoneSegment *seg = malloc(sizeof(*seg));
    if (!seg) return NULL;

    seg->start = malloc(size);
    if (!seg->start) {
        free(seg);
        return NULL;
    }

    seg->current = seg->start;
    seg->end     = seg->start + size;
    seg->size    = size;
    seg->next    = NULL;
    return seg;
}

KyjitZone *kyjit_zone_create(size_t initial_size) {
    KyjitZone *z = calloc(1, sizeof(*z));
    if (!z) return NULL;

    if (initial_size == 0) initial_size = DEFAULT_ZONE_SIZE;

    z->head = segment_create(initial_size);
    if (!z->head) {
        free(z);
        return NULL;
    }

    z->tail = z->head;
    z->total_capacity = initial_size;
    z->total_used = 0;
    return z;
}

static ZoneSegment *zone_grow(KyjitZone *z, size_t min_size) {
    size_t new_size = z->tail->size * 2;
    if (new_size < min_size) new_size = min_size;
    if (new_size > MAX_SEGMENT_SIZE) new_size = MAX_SEGMENT_SIZE;
    if (new_size < min_size) return NULL;  /* can't satisfy */

    ZoneSegment *seg = segment_create(new_size);
    if (!seg) return NULL;

    z->tail->next = seg;
    z->tail = seg;
    z->total_capacity += new_size;
    return seg;
}

void *kyjit_zone_alloc_aligned(KyjitZone *z, size_t size, size_t alignment) {
    if (!z || size == 0) return NULL;
    if (alignment == 0) alignment = 8;

    /* Align current pointer */
    unsigned char *p = align_ptr(z->tail->current, alignment);

    if (p + size > z->tail->end) {
        /* Need new segment */
        if (!zone_grow(z, size + alignment)) return NULL;
        p = align_ptr(z->tail->current, alignment);
    }

    z->tail->current = p + size;
    z->total_used += size;
    return p;
}

void *kyjit_zone_alloc(KyjitZone *z, size_t size) {
    return kyjit_zone_alloc_aligned(z, size, 8);
}

void kyjit_zone_reset(KyjitZone *z) {
    if (!z) return;
    for (ZoneSegment *s = z->head; s; s = s->next) {
        s->current = s->start;
    }
    z->tail = z->head;
    z->total_used = 0;
}

void kyjit_zone_destroy(KyjitZone *z) {
    if (!z) return;
    ZoneSegment *s = z->head;
    while (s) {
        ZoneSegment *next = s->next;
        free(s->start);
        free(s);
        s = next;
    }
    free(z);
}

size_t kyjit_zone_used(KyjitZone *z) {
    return z ? z->total_used : 0;
}

size_t kyjit_zone_capacity(KyjitZone *z) {
    return z ? z->total_capacity : 0;
}
