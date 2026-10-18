#define _GNU_SOURCE
#include "kyjit_zero_copy.h"
#include <sys/mman.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>

struct KyZeroCopy {
    void  *ptr;
    size_t size;
    int    is_mapped;   /* 1 if we own mmap memory */
    int    is_malloc;   /* 1 if we own malloc'd memory */
};

/* page size cache */
static size_t page_size(void) {
    static size_t ps = 0;
    if (!ps) ps = (size_t)sysconf(_SC_PAGESIZE);
    return ps;
}

/* round up to page boundary */
static size_t round_up(size_t size, size_t align) {
    return (size + align - 1) & ~(align - 1);
}

KyZeroCopy *kyjit_zero_copy_create(size_t size) {
    if (size == 0) return NULL;

    KyZeroCopy *buf = (KyZeroCopy*)calloc(1, sizeof(KyZeroCopy));
    if (!buf) return NULL;

    if (size >= 1024 * 1024) {
        /* Large buffer: use mmap for page-aligned zero-copy */
        size_t aligned_size = round_up(size, page_size());
        void *ptr = mmap(NULL, aligned_size, PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (ptr == MAP_FAILED) {
            free(buf);
            return NULL;
        }
        buf->ptr = ptr;
        buf->size = aligned_size;
        buf->is_mapped = 1;
        buf->is_malloc = 0;
    } else {
        /* Small buffer: use posix_memalign for 64-byte alignment */
        void *ptr = NULL;
        if (posix_memalign(&ptr, 64, size) != 0) {
            free(buf);
            return NULL;
        }
        buf->ptr = ptr;
        buf->size = size;
        buf->is_mapped = 0;
        buf->is_malloc = 1;
    }
    return buf;
}

KyZeroCopy *kyjit_zero_copy_attach(void *ptr, size_t size) {
    if (!ptr || size == 0) return NULL;
    KyZeroCopy *buf = (KyZeroCopy*)calloc(1, sizeof(KyZeroCopy));
    if (!buf) return NULL;
    buf->ptr = ptr;
    buf->size = size;
    buf->is_mapped = 0;
    buf->is_malloc = 0;
    return buf;
}

void *kyjit_zero_copy_ptr(KyZeroCopy *buf) {
    return buf ? buf->ptr : NULL;
}

size_t kyjit_zero_copy_size(KyZeroCopy *buf) {
    return buf ? buf->size : 0;
}

void kyjit_zero_copy_hint_sequential(KyZeroCopy *buf) {
    if (!buf) return;
    if (buf->is_mapped) {
        madvise(buf->ptr, buf->size, MADV_SEQUENTIAL);
    }
}

void kyjit_zero_copy_hint_large_pages(KyZeroCopy *buf) {
    if (!buf) return;
    if (buf->is_mapped) {
#ifdef MADV_HUGEPAGE
        madvise(buf->ptr, buf->size, MADV_HUGEPAGE);
#endif
    }
}

void kyjit_zero_copy_destroy(KyZeroCopy *buf) {
    if (!buf) return;
    if (buf->is_mapped && buf->ptr) {
        munmap(buf->ptr, buf->size);
    } else if (buf->is_malloc && buf->ptr) {
        free(buf->ptr);
    }
    free(buf);
}