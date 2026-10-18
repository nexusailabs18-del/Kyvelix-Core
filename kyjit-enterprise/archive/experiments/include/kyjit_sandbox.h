#ifndef KYJIT_SANDBOX_H
#define KYJIT_SANDBOX_H

#include <stddef.h>

typedef struct kyjit_sandbox kyjit_sandbox_t;

kyjit_sandbox_t* kyjit_sandbox_create(size_t max_memory, double max_time_ms, int max_ops);
void* kyjit_sandbox_alloc(kyjit_sandbox_t *sb, size_t size);
int kyjit_sandbox_tick(kyjit_sandbox_t *sb);
int kyjit_sandbox_is_active(kyjit_sandbox_t *sb);
void kyjit_sandbox_free(kyjit_sandbox_t *sb, void *ptr, size_t size);
void kyjit_sandbox_destroy(kyjit_sandbox_t *sb);
int kyjit_sandbox_available(void);

#endif
