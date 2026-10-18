#ifndef KYJIT_HUGEPAGES_H
#define KYJIT_HUGEPAGES_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque hugepage allocation handle */
typedef struct KyHugePages KyHugePages;

/* Allocate `size` bytes backed by huge pages if possible.
 * Falls back to normal mmap if huge pages are not available.
 * Returns NULL on failure.
 */
KyHugePages *kyjit_huge_alloc(size_t size);

/* Allocate `size` bytes explicitly using 1GB pages if available,
 * otherwise 2MB pages, otherwise regular pages.
 * Returns NULL on failure.
 */
KyHugePages *kyjit_huge_alloc_1gb(size_t size);

/* Get the base pointer of the allocated region. */
void *kyjit_huge_ptr(KyHugePages *h);

/* Get the actual size of the allocated region (page-aligned). */
size_t kyjit_huge_size(KyHugePages *h);

/* Free the allocation. */
void kyjit_huge_free(KyHugePages *h);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_HUGEPAGES_H */