#ifndef KYJIT_ZONE_H
#define KYJIT_ZONE_H

#include <stddef.h>
#include <stdint.h>

typedef struct KyjitZone KyjitZone;

/* Create a zone. initial_size may be 0 (default 64KB). */
KyjitZone *kyjit_zone_create(size_t initial_size);

/* Allocate aligned memory from the zone. Returns NULL if OOM.
 * Old pointers become invalid after kyjit_zone_reset().
 */
void *kyjit_zone_alloc(KyjitZone *z, size_t size);
void *kyjit_zone_alloc_aligned(KyjitZone *z, size_t size, size_t alignment);

/* Reset zone — all allocations invalid, memory reused. */
void kyjit_zone_reset(KyjitZone *z);

/* Free entire zone and all memory. */
void kyjit_zone_destroy(KyjitZone *z);

/* Stats */
size_t kyjit_zone_used(KyjitZone *z);
size_t kyjit_zone_capacity(KyjitZone *z);

#endif
