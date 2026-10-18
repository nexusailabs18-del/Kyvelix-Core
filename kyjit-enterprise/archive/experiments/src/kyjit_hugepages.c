#define _GNU_SOURCE
#include "kyjit_hugepages.h"
#include <sys/mman.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <stdio.h>

/* 2MB and 1GB page sizes (fallback if not known) */
#define HUGEPAGE_2MB (2UL * 1024 * 1024)
#define HUGEPAGE_1GB (1UL * 1024 * 1024 * 1024)

struct KyHugePages {
    void  *ptr;
    size_t size;
};

/* Try to allocate `size` bytes using MAP_HUGETLB with given page size.
 * On failure, try smaller page size, then normal mmap.
 */
static void *try_huge_alloc(size_t size, size_t huge_page_size, size_t *actual_size) {
    if (huge_page_size == 0) return NULL;

    /* Align size up to huge page size */
    size_t aligned = (size + huge_page_size - 1) & ~(huge_page_size - 1);

    void *ptr = mmap(NULL, aligned, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB,
                     -1, 0);
    if (ptr != MAP_FAILED) {
        if (actual_size) *actual_size = aligned;
        return ptr;
    }
    return NULL;
}

/* Fallback to normal mmap */
static void *fallback_alloc(size_t size, size_t *actual_size) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    size_t aligned = (size + page - 1) & ~(page - 1);

    void *ptr = mmap(NULL, aligned, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (ptr == MAP_FAILED) return NULL;
    if (actual_size) *actual_size = aligned;
    return ptr;
}

KyHugePages *kyjit_huge_alloc(size_t size) {
    if (size == 0) return NULL;

    KyHugePages *h = (KyHugePages*)calloc(1, sizeof(KyHugePages));
    if (!h) return NULL;

    size_t actual = 0;
    void *ptr = NULL;

    /* Try 2MB huge pages */
    ptr = try_huge_alloc(size, HUGEPAGE_2MB, &actual);
    if (!ptr) {
        /* Fallback to normal pages */
        ptr = fallback_alloc(size, &actual);
    }

    if (!ptr) {
        free(h);
        return NULL;
    }

    h->ptr = ptr;
    h->size = actual;
    return h;
}

KyHugePages *kyjit_huge_alloc_1gb(size_t size) {
    if (size == 0) return NULL;

    KyHugePages *h = (KyHugePages*)calloc(1, sizeof(KyHugePages));
    if (!h) return NULL;

    size_t actual = 0;
    void *ptr = NULL;

    /* Try 1GB pages */
    ptr = try_huge_alloc(size, HUGEPAGE_1GB, &actual);
    if (!ptr) {
        /* Try 2MB pages */
        ptr = try_huge_alloc(size, HUGEPAGE_2MB, &actual);
    }
    if (!ptr) {
        ptr = fallback_alloc(size, &actual);
    }

    if (!ptr) {
        free(h);
        return NULL;
    }

    h->ptr = ptr;
    h->size = actual;
    return h;
}

void *kyjit_huge_ptr(KyHugePages *h) {
    return h ? h->ptr : NULL;
}

size_t kyjit_huge_size(KyHugePages *h) {
    return h ? h->size : 0;
}

void kyjit_huge_free(KyHugePages *h) {
    if (!h) return;
    if (h->ptr) {
        munmap(h->ptr, h->size);
    }
    free(h);
}