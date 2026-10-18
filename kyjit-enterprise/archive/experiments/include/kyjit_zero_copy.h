#ifndef KYJIT_ZERO_COPY_H
#define KYJIT_ZERO_COPY_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque zero-copy buffer handle */
typedef struct KyZeroCopy KyZeroCopy;

/* Create a zero-copy buffer of `size` bytes.
 * Uses mmap for large allocations (page-aligned, no heap overhead).
 * For small sizes (< 1MB), falls back to aligned malloc.
 * Returns NULL on failure.
 */
KyZeroCopy *kyjit_zero_copy_create(size_t size);

/* Wrap existing memory without copying it.
 * The buffer does NOT own the memory; it must remain valid until detach.
 * Returns NULL on failure.
 */
KyZeroCopy *kyjit_zero_copy_attach(void *ptr, size_t size);

/* Get the pointer to the underlying data. */
void *kyjit_zero_copy_ptr(KyZeroCopy *buf);

/* Get the size of the buffer. */
size_t kyjit_zero_copy_size(KyZeroCopy *buf);

/* Hint that the buffer will be accessed sequentially (enables readahead). */
void kyjit_zero_copy_hint_sequential(KyZeroCopy *buf);

/* Hint that the buffer should use large pages (if supported). */
void kyjit_zero_copy_hint_large_pages(KyZeroCopy *buf);

/* Destroy the buffer and release resources.
 * If the buffer was created, memory is unmapped/freed.
 * If it was attached, only the handle is freed.
 */
void kyjit_zero_copy_destroy(KyZeroCopy *buf);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_ZERO_COPY_H */